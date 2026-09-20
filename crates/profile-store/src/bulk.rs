//! エクスポート/インポートの転送用フォーマット(単一プロファイル/全体共通)と、
//! 全体インポート時の名前衝突解決ロジック。

use std::collections::HashSet;

use masking_core::{Rule, RuleProfile, RuleProfileError};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

pub const CURRENT_FORMAT_VERSION: u32 = 1;

// 直列化だけを派生する。復号した平文の読み取りは、ExportPayload::from_json_slice(下記)で行う。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExportedProfile {
    pub is_favorite: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(flatten)]
    pub profile: RuleProfile,
}

impl Zeroize for ExportedProfile {
    fn zeroize(&mut self) {
        self.profile.zeroize();
        self.tags.zeroize();
    }
}

/// 復号したペイロードのプロファイルを、どの経路で捨てても(名前の重複などのエラーで、確認画面へ進まずに捨てる場合を含む)、
/// 内容が消去されるようにする。(Dropを持つため、フィールドをムーブして取り出す書き方はできない。借用で使う。)
impl Drop for ExportedProfile {
    fn drop(&mut self) {
        self.zeroize();
    }
}

// kindタグによるserdeの内部タグ付きenumにより、復号後のJSONの中身だけで
// 単一/全体を自動判別できる(手動での形状判定が不要)。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExportPayload {
    Single { format_version: u32, profile: ExportedProfile },
    All { format_version: u32, active_profile_name: Option<String>, profiles: Vec<ExportedProfile> },
}

impl ExportPayload {
    /// 復号した平文のJSONを読み取る。`Deserialize`の派生(内部タグ付きenum・flatten)は、serdeが、値を一度、内部のバッファへ
    /// 複製して読み直すため、エスケープが要る文字(`"`・`\`・制御文字)を含む値の、消去されない複製が残る。ここでは、種別だけを
    /// 先に読み(他の値は、読み飛ばす)、その種別に対応する型へ、そのまま読む(内部のバッファを使わない)。
    pub fn from_json_slice(json: &[u8]) -> Result<Self, serde_json::Error> {
        #[derive(Deserialize)]
        struct KindProbe {
            kind: String,
        }

        match serde_json::from_slice::<KindProbe>(json)?.kind.as_str() {
            "single" => {
                let read: SinglePayloadRead = serde_json::from_slice(json)?;
                Ok(ExportPayload::Single { format_version: read.format_version, profile: read.profile.0 })
            }
            "all" => {
                let read: AllPayloadRead = serde_json::from_slice(json)?;
                Ok(ExportPayload::All {
                    format_version: read.format_version,
                    active_profile_name: read.active_profile_name,
                    profiles: read.profiles.into_iter().map(|checked| checked.0).collect(),
                })
            }
            other => Err(<serde_json::Error as serde::de::Error>::unknown_variant(other, &["single", "all"])),
        }
    }
}

// 読み取り用の型(from_json_slice専用)。プロファイルの欄は、flattenを使わず、同じ階層の欄として読む。
#[derive(Deserialize)]
struct SinglePayloadRead {
    format_version: u32,
    profile: ExportedProfileChecked,
}

#[derive(Deserialize)]
struct AllPayloadRead {
    format_version: u32,
    active_profile_name: Option<String>,
    profiles: Vec<ExportedProfileChecked>,
}

// プロファイルの検証(RuleProfile::new。名前・ルール名の重複)を、読み取りの途中で必ず通すための橋渡し。
#[derive(Deserialize)]
#[serde(try_from = "ExportedProfileRead")]
struct ExportedProfileChecked(ExportedProfile);

#[derive(Deserialize)]
struct ExportedProfileRead {
    is_favorite: bool,
    #[serde(default)]
    tags: Vec<String>,
    profile_name: String,
    description: Option<String>,
    #[serde(default)]
    rules: Vec<Rule>,
}

impl TryFrom<ExportedProfileRead> for ExportedProfileChecked {
    type Error = RuleProfileError;

    // 拒否したとき、名前・説明・ルールは、RuleProfile::newが消去する。タグは、ここで消去する。
    fn try_from(read: ExportedProfileRead) -> Result<Self, Self::Error> {
        let ExportedProfileRead { is_favorite, mut tags, profile_name, description, rules } = read;
        match RuleProfile::new(profile_name, description, rules) {
            Ok(profile) => Ok(Self(ExportedProfile { is_favorite, tags, profile })),
            Err(error) => {
                tags.zeroize();
                Err(error)
            }
        }
    }
}

/// 全体インポートでの名前解決。`name`が`taken`に無ければそのまま採用し、
/// あれば`"{name} (インポート)"`、それも取られていれば`"{name} (インポート 2)"`
/// ...の順に試す。呼び出し側は解決した名前を都度`taken`に積んでいくことで、
/// 同一インポート内の複数エントリが互いに衝突しないようにする。
pub fn resolve_name(name: &str, taken: &mut HashSet<String>) -> (String, bool) {
    if taken.insert(name.to_string()) {
        return (name.to_string(), false);
    }

    let first_retry = format!("{name} (インポート)");
    if taken.insert(first_retry.clone()) {
        return (first_retry, true);
    }

    let mut n = 2;
    loop {
        let candidate = format!("{name} (インポート {n})");
        if taken.insert(candidate.clone()) {
            return (candidate, true);
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- ExportPayload::from_json_slice ----

    const SINGLE_JSON: &str = r#"{"kind":"single","format_version":1,"profile":{"is_favorite":true,"tags":["t1","t2"],"profile_name":"work","description":"説明","rules":[{"name":"r1","pattern_type":"literal","pattern":"a\"b","mode":"fixed","fixed_value":"X","prefix":null,"enabled":true,"description":null}]}}"#;
    const ALL_JSON: &str = r#"{"kind":"all","format_version":1,"active_profile_name":"home","profiles":[{"is_favorite":false,"tags":[],"profile_name":"home","description":null,"rules":[]},{"is_favorite":true,"tags":["t"],"profile_name":"work","description":null,"rules":[]}]}"#;

    #[test]
    fn a_single_payload_is_read_as_serialized_and_writes_back_to_the_same_json() {
        let payload = ExportPayload::from_json_slice(SINGLE_JSON.as_bytes()).unwrap();

        let ExportPayload::Single { format_version, profile } = &payload else { panic!("単一のはず") };
        assert_eq!(*format_version, 1);
        assert!(profile.is_favorite);
        assert_eq!(profile.tags, vec!["t1".to_string(), "t2".to_string()]);
        assert_eq!(profile.profile.profile_name(), "work");
        assert_eq!(profile.profile.description(), Some("説明"));
        assert_eq!(profile.profile.rules()[0].pattern(), "a\"b");
        assert_eq!(serde_json::to_string(&payload).unwrap(), SINGLE_JSON);
    }

    #[test]
    fn an_all_payload_is_read_as_serialized_and_writes_back_to_the_same_json() {
        let payload = ExportPayload::from_json_slice(ALL_JSON.as_bytes()).unwrap();

        let ExportPayload::All { active_profile_name, profiles, .. } = &payload else { panic!("全体のはず") };
        assert_eq!(active_profile_name.as_deref(), Some("home"));
        assert_eq!(profiles.len(), 2);
        assert_eq!(serde_json::to_string(&payload).unwrap(), ALL_JSON);
    }

    // 従来どおり: 余分なキーは無視し、省略できる欄(tags・description・rules)は省略できる。
    #[test]
    fn unknown_keys_are_ignored_and_optional_fields_may_be_omitted() {
        let json = r#"{"kind":"single","format_version":1,"extra":{"a":[1,2]},"profile":{"is_favorite":false,"profile_name":"p","surprise":"x"}}"#;

        let payload = ExportPayload::from_json_slice(json.as_bytes()).unwrap();

        let ExportPayload::Single { profile, .. } = &payload else { panic!("単一のはず") };
        assert!(profile.tags.is_empty());
        assert_eq!(profile.profile.description(), None);
        assert!(profile.profile.rules().is_empty());
    }

    #[test]
    fn a_payload_that_is_not_well_formed_is_rejected_with_the_reason() {
        let cases: [(&str, &str, &str); 6] = [
            ("kindが無い", r#"{"format_version":1}"#, "kind"),
            ("kindが未知", r#"{"kind":"weird","format_version":1}"#, "unknown variant"),
            ("kindが文字列でない", r#"{"kind":7}"#, "invalid type"),
            ("is_favoriteが無い", r#"{"kind":"single","format_version":1,"profile":{"profile_name":"p"}}"#, "is_favorite"),
            ("プロファイル名が無い", r#"{"kind":"single","format_version":1,"profile":{"is_favorite":false}}"#, "profile_name"),
            ("JSONではない", "not json", "expected"),
        ];
        for (label, json, reason) in cases {
            let err = ExportPayload::from_json_slice(json.as_bytes()).expect_err(label);
            assert!(err.to_string().contains(reason), "{label}: 理由に「{reason}」が含まれるはず: {err}");
        }
    }

    // 読み取りの途中でも、プロファイルの検証(名前・ルール名の重複)を通す(RuleProfile::new)。
    #[test]
    fn a_profile_with_a_duplicate_rule_name_or_an_invalid_name_is_rejected_while_reading() {
        let rule = |name: &str| format!(r#"{{"name":"{name}","pattern_type":"literal","pattern":"a","mode":"fixed","fixed_value":"X"}}"#);
        let duplicate = format!(
            r#"{{"kind":"single","format_version":1,"profile":{{"is_favorite":false,"profile_name":"p","rules":[{},{}]}}}}"#,
            rule("dup"),
            rule("dup")
        );
        // 双方向書式文字(U+202E)を含む名前は、表示名として不正(RuleProfile::new)。
        let invalid_name = r#"{"kind":"single","format_version":1,"profile":{"is_favorite":false,"profile_name":"p\u202E"}}"#;

        let duplicate_err = ExportPayload::from_json_slice(duplicate.as_bytes()).expect_err("重複するルール名は、拒否されるはず");
        let name_err = ExportPayload::from_json_slice(invalid_name.as_bytes()).expect_err("不正なプロファイル名は、拒否されるはず");

        assert!(duplicate_err.to_string().contains("重複"), "{duplicate_err}");
        assert!(name_err.to_string().contains("プロファイル名"), "{name_err}");
    }

    #[test]
    fn keeps_the_original_name_when_not_taken() {
        let mut taken = HashSet::new();
        let (name, renamed) = resolve_name("work", &mut taken);
        assert_eq!(name, "work");
        assert!(!renamed);
    }

    #[test]
    fn appends_a_marker_on_first_collision() {
        let mut taken = HashSet::from(["work".to_string()]);
        let (name, renamed) = resolve_name("work", &mut taken);
        assert_eq!(name, "work (インポート)");
        assert!(renamed);
    }

    #[test]
    fn appends_a_counter_on_repeated_collisions() {
        let mut taken = HashSet::from(["work".to_string(), "work (インポート)".to_string()]);
        let (name, renamed) = resolve_name("work", &mut taken);
        assert_eq!(name, "work (インポート 2)");
        assert!(renamed);
    }

    #[test]
    fn skips_over_multiple_existing_counters() {
        let mut taken = HashSet::from([
            "work".to_string(),
            "work (インポート)".to_string(),
            "work (インポート 2)".to_string(),
            "work (インポート 3)".to_string(),
        ]);
        let (name, renamed) = resolve_name("work", &mut taken);
        assert_eq!(name, "work (インポート 4)");
        assert!(renamed);
    }

    #[test]
    fn marks_each_resolved_name_as_taken_so_siblings_in_the_same_batch_do_not_collide() {
        // 同じインポートファイル内に同名が複数あった場合、2件目以降は1件目が
        // 確保した名前とは別の名前に解決されるはず。
        let mut taken = HashSet::from(["work".to_string()]);
        let (first, _) = resolve_name("work", &mut taken);
        let (second, _) = resolve_name("work", &mut taken);
        assert_ne!(first, second);
    }

    #[test]
    fn different_names_do_not_affect_each_other() {
        let mut taken = HashSet::new();
        let (a, renamed_a) = resolve_name("work", &mut taken);
        let (b, renamed_b) = resolve_name("personal", &mut taken);
        assert_eq!(a, "work");
        assert_eq!(b, "personal");
        assert!(!renamed_a);
        assert!(!renamed_b);
    }
}
