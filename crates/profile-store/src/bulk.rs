//! エクスポート/インポートの転送用フォーマット(単一プロファイル/全体共通)と、
//! 全体インポート時の名前衝突解決ロジック。

use std::collections::HashSet;

use masking_core::RuleProfile;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

pub const CURRENT_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExportPayload {
    Single { format_version: u32, profile: ExportedProfile },
    All { format_version: u32, active_profile_name: Option<String>, profiles: Vec<ExportedProfile> },
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
