//! ルールプロファイルの適用パイプライン。
//!
//! 各有効ルールを元テキスト全体に対して独立に走らせてマッチ候補を収集する(テキストの分割は
//! 行わない)。候補同士が重なる場合、「勝者だけ採用し敗者は棄却する」のではなく、関係する
//! 候補全ての範囲を1つのクラスタとして結合し、そのクラスタ全体を優先度最上位(範囲が最長、
//! 同じ長さならルール順序が先)の候補のダミー生成方式で1回だけ置換する。クラスタは常に、
//! それを構成した全候補の範囲を完全に包含するよう拡張されるため、ある候補の検出範囲の
//! 一部だけが別候補に覆われず素通りする、という漏れが構造的に起きない。

use std::collections::{BTreeMap, HashMap};

use crate::matcher::{compile_rule_pattern, find_matches_compiled};
use crate::models::{Mode, Rule, RuleProfile};

/// 元の値 -> ダミー値の対応表と、連番モード用のprefixごとのカウンタ。
/// 呼び出し側(profile-store/cli/gui)が保持・受け渡しする。masking-core内にグローバル状態は持たない。
///
/// **不変条件(呼び出し側が守る責務)**: 同名のルールは、この`MappingStore`に対して行う
/// 全ての`apply_profile`呼び出しを通じて常に同じ設定(mode/fixed_value/prefix)であること。
/// `RuleProfile::new`は1プロファイル内でのルール名の重複を拒否するが、これは1プロファイル
/// 内に限った保証であり、1つの`MappingStore`を複数の`RuleProfile`(または再読み込みで内容が
/// 変わった同名プロファイル)に跨いで使い回す場合はこの範囲外になる。跨いで使い回す場合、
/// 呼び出し側がこの不変条件を保つ必要がある(例: プロファイルを切り替えたら新しい
/// `MappingStore`を使う)。破ると、先に適用されたルールの置換値が後のルールの設定を無視して
/// 再利用される。masking-core側は呼び出し側のMappingStoreライフサイクル方針を知り得ないため、
/// この違反はここでは検出できない。
#[derive(Debug, Default, Clone)]
pub struct MappingStore {
    // キーは(ルール名, 元の値)。ルール名を含めないと、異なるルールが偶然同じ文字列値に
    // マッチした際に片方のルールの置換値を誤って再利用してしまう。
    mapping: HashMap<(String, String), String>,
    counters: HashMap<String, u64>,
}

impl MappingStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_or_create(&mut self, original: &str, rule: &Rule) -> String {
        let key = (rule.name().to_string(), original.to_string());
        if let Some(existing) = self.mapping.get(&key) {
            return existing.clone();
        }
        let dummy = self.generate_dummy(rule);
        self.mapping.insert(key, dummy.clone());
        dummy
    }

    fn generate_dummy(&mut self, rule: &Rule) -> String {
        match rule.mode() {
            Mode::Fixed => rule
                .fixed_value()
                .expect("guaranteed by Rule constructor")
                .to_string(),
            Mode::Sequential => {
                // カウンタはrule名ではなくprefix単位で共有する(同じprefixを使う複数ルールで連番を分けない)。
                let prefix = rule.prefix().expect("guaranteed by Rule constructor");
                let counter = self.counters.entry(prefix.to_string()).or_insert(0);
                *counter += 1;
                format!("{prefix}{counter}__")
            }
        }
    }
}

/// 1回の`apply_profile`(または`apply_compiled_profile`)実行で、有効な各ルールが元テキストに
/// マッチした延べ回数。マッチした範囲は必ずどこかのクラスタに吸収されて実際にマスクされる
/// (クラスタ結合方式により、検出されたのに素通りする候補は存在しない)。無効なルールは
/// 実行されないため一覧に現れない(0件として報告しない)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleMatchCount {
    pub rule_name: String,
    pub count: usize,
}

struct CompiledEntry<'a> {
    rule: &'a Rule,
    rule_index: usize,
    compiled: regex::Regex,
}

/// `RuleProfile`の全ての有効ルールの正規表現を1回だけコンパイルしてキャッシュしたもの。
///
/// 同じプロファイルを多数のテキスト(例: `--stream`で1行ごとに呼ばれるケース)に繰り返し
/// 適用する場合、これを1回だけ作って`apply_compiled_profile`に渡すことで、呼び出しごとの
/// 再コンパイルを避けられる。1回限りの利用には`apply_profile`を使えばよい。
///
/// **不変条件(呼び出し側が守る責務)**: 元の`RuleProfile`の内容(ルールの追加/削除/変更、
/// アクティブプロファイルの切り替え等)が変わったら、古い`CompiledProfile`を使い続けず
/// 新しく`compile`し直すこと。コンパイラは古い`RuleProfile`への参照を検知できないため、
/// 怠ると古いルール集合のままマスキングが無警告で継続する。
pub struct CompiledProfile<'a> {
    entries: Vec<CompiledEntry<'a>>,
}

impl<'a> CompiledProfile<'a> {
    pub fn compile(profile: &'a RuleProfile) -> Self {
        let entries = profile
            .rules()
            .iter()
            .enumerate()
            .filter(|(_, rule)| rule.enabled())
            .map(|(rule_index, rule)| CompiledEntry {
                rule,
                rule_index,
                compiled: compile_rule_pattern(rule),
            })
            .collect();
        Self { entries }
    }
}

#[derive(Clone, Copy)]
struct Candidate {
    start: usize,
    end: usize,
    rule_index: usize,
}

/// クラスタの優先度キー: 範囲が長い方が勝ち、同じ長さならプロファイル内で先にあるルールが勝つ。
/// `true`は「aの方がbより優先度が高い」ことを意味する。
fn is_higher_priority(a_len: usize, a_rule_index: usize, b_len: usize, b_rule_index: usize) -> bool {
    a_len > b_len || (a_len == b_len && a_rule_index < b_rule_index)
}

/// 1回限りの利用向けの簡易API。プロファイルをその場でコンパイルしてから適用する。
/// 同じプロファイルを多数のテキストに繰り返し適用する場合は[`CompiledProfile::compile`]と
/// [`apply_compiled_profile`]を使い、呼び出しごとの再コンパイルを避けること。
pub fn apply_profile(text: &str, profile: &RuleProfile, store: &mut MappingStore) -> (String, Vec<RuleMatchCount>) {
    apply_compiled_profile(text, &CompiledProfile::compile(profile), store)
}

pub fn apply_compiled_profile(
    text: &str,
    compiled: &CompiledProfile,
    store: &mut MappingStore,
) -> (String, Vec<RuleMatchCount>) {
    let mut candidates: Vec<Candidate> = Vec::new();
    for entry in &compiled.entries {
        for m in find_matches_compiled(text, &entry.compiled, entry.rule.name()) {
            candidates.push(Candidate { start: m.start, end: m.end, rule_index: entry.rule_index });
        }
    }

    // 長さ降順、同じ長さならルール順序、さらに同じならテキスト中の開始位置で処理する。
    // 優先度の高い候補から先に処理することで、クラスタの「勝者」は常にそのクラスタに
    // 合流してくる新しい候補より優先度が高いか同等になる。
    let mut sorted = candidates.clone();
    sorted.sort_by(|a, b| {
        let len_a = a.end - a.start;
        let len_b = b.end - b.start;
        len_b.cmp(&len_a).then(a.rule_index.cmp(&b.rule_index)).then(a.start.cmp(&b.start))
    });

    // start -> (end, クラスタの勝者rule_index, 勝者の(長さ, rule_index))。
    // クラスタ同士は互いに重ならないという不変条件を維持する。
    let mut clusters: BTreeMap<usize, (usize, usize, (usize, usize))> = BTreeMap::new();

    for candidate in &sorted {
        let mut merge_start = candidate.start;
        let mut merge_end = candidate.end;
        let mut winner_index = candidate.rule_index;
        let mut winner_len = candidate.end - candidate.start;

        // 新しい候補が既存クラスタと重なる限り結合するloop。クラスタ同士は互いに重ならない
        // という不変条件があるため、重なりうるクラスタは「merge_startより手前で最後(直前)の
        // 1つ」と「[merge_start, merge_end)の範囲内に開始位置を持つもの全て」の2種類だけで
        // 尽くされ、1回のiterationで漏れなく吸収できる(実際には毎回1周で不動点に達する)。
        // before側をmerge_start未満(排他)、within側をmerge_start以上(包含)にすることで、
        // 同じクラスタを両方から二重に検出しないようにしている。範囲の下限を指定せず
        // 全クラスタを毎回舐めるとO(N^2)になるため、この境界の絞り込みが性能上重要。
        //
        // 数珠つなぎの橋渡し(A-B-Cが順に部分重複するケース)は、この内側のloopではなく
        // 外側の`for candidate in &sorted`が候補を1つずつ処理する過程で実現される。
        loop {
            let mut overlapping: Vec<usize> = Vec::new();
            if let Some((&s, &(end, _, _))) = clusters.range(..merge_start).next_back() {
                if end > merge_start {
                    overlapping.push(s);
                }
            }
            overlapping.extend(clusters.range(merge_start..merge_end).map(|(&s, _)| s));
            if overlapping.is_empty() {
                break;
            }
            for s in overlapping {
                let (end, w_idx, (w_len, w_rule_index)) = clusters.remove(&s).unwrap();
                merge_start = merge_start.min(s);
                merge_end = merge_end.max(end);
                if is_higher_priority(w_len, w_rule_index, winner_len, winner_index) {
                    winner_index = w_idx;
                    winner_len = w_len;
                }
            }
        }

        clusters.insert(merge_start, (merge_end, winner_index, (winner_len, winner_index)));
    }

    let rule_by_index: HashMap<usize, &Rule> = compiled.entries.iter().map(|e| (e.rule_index, e.rule)).collect();

    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    for (&start, &(end, winner_index, _)) in &clusters {
        out.push_str(&text[cursor..start]);
        let rule = *rule_by_index
            .get(&winner_index)
            .expect("winner_indexはcompiled.entriesから採取したものなので必ず存在する");
        let original = &text[start..end];
        out.push_str(&store.get_or_create(original, rule));
        cursor = end;
    }
    out.push_str(&text[cursor..]);

    // クラスタ結合方式では、検出された候補が最終的に棄却されることは無い(必ずどこかの
    // クラスタに吸収される)ため、ルールごとの件数は単純に元々の候補数を数えればよい。
    let mut counts_by_index: HashMap<usize, usize> = HashMap::new();
    for candidate in &candidates {
        *counts_by_index.entry(candidate.rule_index).or_insert(0) += 1;
    }
    let match_counts = compiled
        .entries
        .iter()
        .map(|entry| RuleMatchCount {
            rule_name: entry.rule.name().to_string(),
            count: counts_by_index.get(&entry.rule_index).copied().unwrap_or(0),
        })
        .collect();

    (out, match_counts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::PatternType;

    fn sequential_rule(name: &str, pattern: &str, prefix: &str) -> Rule {
        Rule::new(name, PatternType::Regex, pattern, Mode::Sequential, None, Some(prefix.into()), true, None)
            .unwrap()
    }

    fn fixed_rule(name: &str, pattern_type: PatternType, pattern: &str, fixed_value: &str) -> Rule {
        Rule::new(name, pattern_type, pattern, Mode::Fixed, Some(fixed_value.into()), None, true, None).unwrap()
    }

    #[test]
    fn single_rule_replaces_match_and_reports_count() {
        let profile =
            RuleProfile::new("p", None, vec![sequential_rule("ip", r"\d+\.\d+\.\d+\.\d+", "__MASK_IP_")]).unwrap();
        let mut store = MappingStore::new();
        let (masked, counts) = apply_profile("connect to 10.0.0.1 now", &profile, &mut store);
        assert_eq!(masked, "connect to __MASK_IP_1__ now");
        assert_eq!(counts, vec![RuleMatchCount { rule_name: "ip".into(), count: 1 }]);
    }

    #[test]
    fn narrow_rule_before_wide_rule_no_longer_leaks_the_remainder() {
        // 4桁数字ルールが電話番号ルールより先にあっても、電話番号全体が正しく
        // 1つのクラスタとして優先され、断片が漏れない。
        let profile = RuleProfile::new(
            "p",
            None,
            vec![
                fixed_rule("generic_4digit", PatternType::Regex, r"\d{4}", "XXXX"),
                sequential_rule("phone", r"\d{2,4}-\d{2,4}-\d{3,4}", "__MASK_PHONE_"),
            ],
        )
        .unwrap();
        let mut store = MappingStore::new();
        let (masked, counts) = apply_profile("call 090-1234-5678 now", &profile, &mut store);
        assert_eq!(masked, "call __MASK_PHONE_1__ now");
        // generic_4digitは"1234"と"5678"の2箇所にマッチするが、どちらもphoneのクラスタに
        // 吸収されるため出力には影響しない(検出はしたが単独では採用されなかった)。
        assert_eq!(counts[0].count, 2);
        assert_eq!(counts[1].count, 1);
    }

    #[test]
    fn rule_order_does_not_affect_the_result_when_matches_overlap() {
        let profile = RuleProfile::new(
            "p",
            None,
            vec![
                sequential_rule("phone", r"\d{2,4}-\d{2,4}-\d{3,4}", "__MASK_PHONE_"),
                fixed_rule("generic_4digit", PatternType::Regex, r"\d{4}", "XXXX"),
            ],
        )
        .unwrap();
        let mut store = MappingStore::new();
        let (masked, counts) = apply_profile("call 090-1234-5678 now", &profile, &mut store);
        assert_eq!(masked, "call __MASK_PHONE_1__ now");
        assert_eq!(counts[0].count, 1);
        assert_eq!(counts[1].count, 2);
    }

    #[test]
    fn crossing_overlap_masks_the_full_union_without_leaking_the_remainder() {
        // 入れ子(完全包含)ではなく交差する部分重複の場合でも、勝者に負けた側の
        // 「重ならない残り部分」が漏れてはいけない。
        let profile = RuleProfile::new(
            "p",
            None,
            vec![
                fixed_rule("first", PatternType::Literal, "abcdef", "FIRST"),
                fixed_rule("second", PatternType::Literal, "cdefgh", "SECOND"),
            ],
        )
        .unwrap();
        let mut store = MappingStore::new();
        let (masked, counts) = apply_profile("abcdefgh", &profile, &mut store);
        // "gh"が平文のまま残ってはいけない。
        assert_eq!(masked, "FIRST");
        assert_eq!(counts[0].count, 1);
        assert_eq!(counts[1].count, 1, "secondも検出はしているので件数には現れる");
    }

    #[test]
    fn chained_crossing_overlaps_merge_into_one_cluster() {
        // A-B-Cが数珠つなぎに部分重複する場合、橋渡しの結果Aだけでは重ならなかったCとも
        // 最終的に1つのクラスタに結合される必要がある。
        let profile = RuleProfile::new(
            "p",
            None,
            vec![
                fixed_rule("a", PatternType::Literal, "abcd", "X"), // [0,4)
                fixed_rule("b", PatternType::Literal, "cdef", "X"), // [2,6), aとのみ直接重なる
                fixed_rule("c", PatternType::Literal, "efgh", "X"), // [4,8), aとは重ならずbとのみ直接重なる
            ],
        )
        .unwrap();
        let mut store = MappingStore::new();
        let (masked, counts) = apply_profile("abcdefgh", &profile, &mut store);
        assert_eq!(masked, "X", "a/b/cが橋渡しで1つのクラスタに結合され、全体が1回だけ置換されるはず");
        assert_eq!(counts.iter().map(|c| c.count).collect::<Vec<_>>(), vec![1, 1, 1]);
    }

    #[test]
    fn equal_length_overlapping_matches_are_resolved_by_rule_order() {
        let profile = RuleProfile::new(
            "p",
            None,
            vec![
                fixed_rule("first", PatternType::Literal, "TOKEN", "FIRST_WINS"),
                fixed_rule("second", PatternType::Literal, "TOKEN", "SECOND_WINS"),
            ],
        )
        .unwrap();
        let mut store = MappingStore::new();
        let (masked, counts) = apply_profile("see TOKEN here", &profile, &mut store);
        assert_eq!(masked, "see FIRST_WINS here");
        assert_eq!(counts[0].count, 1);
        assert_eq!(counts[1].count, 1, "secondも同じ範囲を検出しているので件数には現れる");
    }

    #[test]
    fn non_overlapping_matches_from_different_rules_coexist() {
        let profile = RuleProfile::new(
            "p",
            None,
            vec![
                sequential_rule("ip", r"\d+\.\d+\.\d+\.\d+", "__MASK_IP_"),
                fixed_rule("pw", PatternType::Regex, r"(?i)password=\S+", "password=__MASK_REDACTED__"),
            ],
        )
        .unwrap();
        let mut store = MappingStore::new();
        let (masked, _) = apply_profile("10.0.0.1 password=abc123", &profile, &mut store);
        assert_eq!(masked, "__MASK_IP_1__ password=__MASK_REDACTED__");
    }

    #[test]
    fn a_rule_matching_text_that_only_appears_inside_another_rules_dummy_value_is_not_triggered() {
        // 別ルールの置換後の値(ダミー値)に含まれる文字列は、元テキストそのものには存在しない限り
        // 決してマッチしない(常に分割前の元テキストだけを見るため、構造的に安全)。
        let profile = RuleProfile::new(
            "p",
            None,
            vec![
                sequential_rule("ip", r"\d+\.\d+\.\d+\.\d+", "__MASK_IP_"),
                fixed_rule("literal_mask", PatternType::Literal, "MASK", "SHOULD_NOT_APPEAR"),
            ],
        )
        .unwrap();
        let mut store = MappingStore::new();
        let (masked, counts) = apply_profile("ip is 10.0.0.1 here", &profile, &mut store);
        assert_eq!(masked, "ip is __MASK_IP_1__ here");
        assert_eq!(counts[1].count, 0, "元テキストに'MASK'は存在しないのでマッチしないはず");
    }

    #[test]
    fn same_original_value_reuses_same_dummy_in_sequential_mode() {
        let profile =
            RuleProfile::new("p", None, vec![sequential_rule("ip", r"\d+\.\d+\.\d+\.\d+", "__MASK_IP_")]).unwrap();
        let mut store = MappingStore::new();
        let (masked, _) = apply_profile("10.0.0.1 talks to 10.0.0.1 again", &profile, &mut store);
        assert_eq!(masked, "__MASK_IP_1__ talks to __MASK_IP_1__ again");
    }

    #[test]
    fn distinct_values_increment_counter_and_different_rules_share_prefix_counter() {
        let profile = RuleProfile::new(
            "p",
            None,
            vec![
                sequential_rule("ip_a", r"10\.0\.0\.\d+", "__MASK_IP_"),
                sequential_rule("ip_b", r"192\.168\.0\.\d+", "__MASK_IP_"),
            ],
        )
        .unwrap();
        let mut store = MappingStore::new();
        let (masked, _) = apply_profile("10.0.0.1 and 192.168.0.5", &profile, &mut store);
        // 別ルールでも同じprefixを使えば連番カウンタを共有する(prefix単位)。
        assert_eq!(masked, "__MASK_IP_1__ and __MASK_IP_2__");
    }

    #[test]
    fn fixed_mode_always_uses_the_same_value() {
        let profile = RuleProfile::new(
            "p",
            None,
            vec![fixed_rule("pw", PatternType::Regex, r"(?i)password=\S+", "password=__MASK_REDACTED__")],
        )
        .unwrap();
        let mut store = MappingStore::new();
        let (masked, _) = apply_profile("password=abc123 and password=def456", &profile, &mut store);
        assert_eq!(masked, "password=__MASK_REDACTED__ and password=__MASK_REDACTED__");
    }

    #[test]
    fn disabled_rule_is_skipped_and_omitted_from_match_counts() {
        let mut disabled = sequential_rule("ip", r"\d+\.\d+\.\d+\.\d+", "__MASK_IP_");
        disabled = Rule::new(
            disabled.name(),
            disabled.pattern_type(),
            disabled.pattern(),
            disabled.mode(),
            disabled.fixed_value().map(String::from),
            disabled.prefix().map(String::from),
            false,
            None,
        )
        .unwrap();
        let profile = RuleProfile::new("p", None, vec![disabled]).unwrap();
        let mut store = MappingStore::new();
        let (masked, counts) = apply_profile("10.0.0.1 stays as is", &profile, &mut store);
        assert_eq!(masked, "10.0.0.1 stays as is");
        assert!(counts.is_empty(), "無効ルールはmatch_countsに現れないはず");
    }

    #[test]
    fn text_not_matching_any_rule_is_left_untouched() {
        let profile =
            RuleProfile::new("p", None, vec![sequential_rule("ip", r"\d+\.\d+\.\d+\.\d+", "__MASK_IP_")]).unwrap();
        let mut store = MappingStore::new();
        let (masked, counts) = apply_profile("no sensitive data here", &profile, &mut store);
        assert_eq!(masked, "no sensitive data here");
        assert_eq!(counts[0].count, 0);
    }

    #[test]
    fn empty_profile_leaves_text_unchanged() {
        let profile = RuleProfile::new("p", None, vec![]).unwrap();
        let mut store = MappingStore::new();
        let (masked, counts) = apply_profile("anything at all", &profile, &mut store);
        assert_eq!(masked, "anything at all");
        assert!(counts.is_empty());
    }

    #[test]
    fn different_rules_matching_the_identical_value_get_independent_dummies() {
        let profile = RuleProfile::new(
            "p",
            None,
            vec![
                sequential_rule("anchored", r"^TOKEN", "A_"),
                sequential_rule("unanchored", r"TOKEN", "B_"),
            ],
        )
        .unwrap();
        let mut store = MappingStore::new();
        let (masked, counts) = apply_profile("TOKEN abc TOKEN", &profile, &mut store);
        assert_eq!(masked, "A_1__ abc B_1__");
        // unanchoredは2箇所にマッチする: 1つめは"anchored"との同じ範囲・同じ長さの重なりで
        // ルール順序により負けるが検出はしている、2つめは単独でマッチして採用される。
        assert_eq!(counts[1].count, 2);
    }

    #[test]
    fn duplicate_rule_names_are_rejected_at_profile_construction() {
        let dup_a = sequential_rule("dup", r"^SECRET", "A_");
        let dup_b = sequential_rule("dup", r"SECRET", "B_");
        let err = RuleProfile::new("p", None, vec![dup_a, dup_b]).expect_err("ルール名の重複は拒否されるはず");
        assert!(matches!(err, crate::models::RuleProfileError::DuplicateRuleName { .. }));
    }

    #[test]
    fn multibyte_japanese_text_round_trips_correctly() {
        let profile = RuleProfile::new(
            "p",
            None,
            vec![sequential_rule("phone", r"\d{2,4}-\d{2,4}-\d{3,4}", "__MASK_PHONE_")],
        )
        .unwrap();
        let mut store = MappingStore::new();
        let (masked, _) = apply_profile("担当者の電話番号は090-1234-5678です。", &profile, &mut store);
        assert_eq!(masked, "担当者の電話番号は__MASK_PHONE_1__です。");
    }

    #[test]
    fn compiled_profile_can_be_reused_across_multiple_texts() {
        // --streamのように、1つのプロファイルを多数の行に繰り返し適用するユースケースの検証。
        let profile =
            RuleProfile::new("p", None, vec![sequential_rule("ip", r"\d+\.\d+\.\d+\.\d+", "__MASK_IP_")]).unwrap();
        let compiled = CompiledProfile::compile(&profile);
        let mut store = MappingStore::new();

        let (line1, _) = apply_compiled_profile("first line 10.0.0.1", &compiled, &mut store);
        let (line2, _) = apply_compiled_profile("second line 10.0.0.1 and 192.168.0.1", &compiled, &mut store);

        assert_eq!(line1, "first line __MASK_IP_1__");
        // storeを跨いで共有しているので、同じ値には同じダミーが割り当てられる。
        assert_eq!(line2, "second line __MASK_IP_1__ and __MASK_IP_2__");
    }
}
