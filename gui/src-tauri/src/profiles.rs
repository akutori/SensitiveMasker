use std::sync::Mutex;

use masking_core::RuleProfile;
use profile_store::{AppPaths, ProfileStore, ProfileStoreError, ProfileSummary};
use tauri::Emitter;

#[derive(Default)]
pub struct ProfileStoreState(Mutex<Option<ProfileStore>>);

impl ProfileStoreState {
    // export_import.rsのテストが実ストア入りの状態を組み立てるためのもの
    // (フィールド自体は非公開のため)。
    #[cfg(test)]
    pub(crate) fn with_store_for_test(store: ProfileStore) -> Self {
        Self(Mutex::new(Some(store)))
    }
}

/// SENSITIVEMASKER_DATA_DIRが設定されていればそこを、無ければOS標準の
/// データディレクトリを使う。E2Eテストが実ユーザーの鍵/DBを書き換えないようにする
/// ためのdebug build専用の迂回路(release buildではこの分岐自体が存在しない)。
/// std::env::set_varはプロセス全体に影響しテスト間で競合しうるため、実際の環境変数
/// 読み取りとロジック本体を分離し、後者だけを引数渡しでテストできるようにする
/// (with_storeをtauri::State非依存にしたのと同じ方針)。
#[cfg(debug_assertions)]
fn resolve_paths_with_override(override_dir: Option<String>) -> Result<AppPaths, String> {
    match override_dir {
        Some(dir) => Ok(AppPaths::at(dir)),
        None => AppPaths::resolve().map_err(|e| e.to_string()),
    }
}

#[cfg(debug_assertions)]
pub(crate) fn resolve_paths() -> Result<AppPaths, String> {
    resolve_paths_with_override(std::env::var("SENSITIVEMASKER_DATA_DIR").ok())
}

#[cfg(not(debug_assertions))]
pub(crate) fn resolve_paths() -> Result<AppPaths, String> {
    AppPaths::resolve().map_err(|e| e.to_string())
}

// tauri::Stateに依存しない形にして単体テスト可能にする(masking.rsのmask_text_with_stores
// と同じ方針)。呼び出し側は&tauri::State<'_, ProfileStoreState>のままDeref経由で渡せる。
pub(crate) fn with_store<T>(
    state: &ProfileStoreState,
    f: impl FnOnce(&mut ProfileStore) -> Result<T, ProfileStoreError>,
) -> Result<T, String> {
    let mut guard = state.0.lock().unwrap_or_else(|e| e.into_inner());
    let store = guard.as_mut().ok_or_else(|| ProfileStoreError::NotInitialized.to_string())?;
    f(store).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn is_store_initialized() -> Result<bool, String> {
    let paths = resolve_paths()?;
    profile_store::is_initialized_at(&paths).map_err(|e| e.to_string())
}

fn open_into_state(state: &ProfileStoreState) -> Result<(), String> {
    let paths = resolve_paths()?;
    let store = ProfileStore::open_at(&paths).map_err(|e| e.to_string())?;
    *state.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(store);
    Ok(())
}

/// 2回目以降の起動用。ディスク上は既に初期化済みだが、このプロセスの
/// ProfileStoreStateはプロセス起動ごとに空(None)から始まるため、
/// 起動直後に一度だけ呼んでメモリ上の状態を実体化する。
#[tauri::command]
pub async fn open_store(state: tauri::State<'_, ProfileStoreState>) -> Result<(), String> {
    open_into_state(&state)
}

/// 初回セットアップ(「始める」)用。鍵/DBの新規作成に続けて、開いたストアを
/// そのままProfileStoreStateに格納する(open_storeを別途呼ぶ必要はない)。
#[tauri::command]
pub async fn initialize_store(state: tauri::State<'_, ProfileStoreState>) -> Result<(), String> {
    let paths = resolve_paths()?;
    profile_store::init_at(&paths).map_err(|e| e.to_string())?;
    open_into_state(&state)
}

#[tauri::command]
pub async fn list_profiles(state: tauri::State<'_, ProfileStoreState>) -> Result<Vec<ProfileSummary>, String> {
    with_store(&state, |store| store.list_profiles())
}

#[tauri::command]
pub async fn get_profile(
    state: tauri::State<'_, ProfileStoreState>,
    name: String,
) -> Result<RuleProfile, String> {
    with_store(&state, |store| store.get_profile(&name))
}

#[tauri::command]
pub async fn create_profile(
    app: tauri::AppHandle,
    state: tauri::State<'_, ProfileStoreState>,
    profile: RuleProfile,
) -> Result<i64, String> {
    let id = with_store(&state, |store| store.create_profile(&profile))?;
    let _ = app.emit("profiles-changed", ());
    Ok(id)
}

fn rules_match_content(a: &masking_core::Rule, b: &masking_core::Rule) -> bool {
    b.enabled()
        && b.pattern_type() == a.pattern_type()
        && b.pattern() == a.pattern()
        && b.mode() == a.mode()
        && b.fixed_value() == a.fixed_value()
        && b.prefix() == a.prefix()
}

// nameではなく内容(pattern等5項目)の多重集合として比較する。名前を手がかりにすると、
// マスク挙動に影響しないリネームだけで無力化と誤判定してしまうため。beforeで有効
// だったルールと同じ内容を持つ有効なルールがafterにまだ(名前を問わず)残っていれば、
// そのルールを1件だけ消費して「無力化されていない」とみなす(重複した内容が複数
// あった場合に、1件消えても他が残っていれば実質的な影響は無いと扱うため)。
// 有効ルール数という集計値だけでは、件数を変えない入れ替え(あるルールを無効化しつつ
// 別のルールを有効化する)を見逃すため、ルール単位で比較する。
//
// 消費順は2巡に分ける: 1巡目は同名かつ内容一致のみを消費し、2巡目で残った分だけ
// 名前を問わず内容一致を試みる。内容が重複する複数のルールが存在し片方だけが
// 消えた場合に、1巡限りだと消費順によって「実際には変わっていない方」が消えた
// 側として報告されうる(名前を優先して自分自身に一致させることでこれを防ぐ)。
fn weakened_rule_names(before: &RuleProfile, after: &RuleProfile) -> Vec<String> {
    let mut remaining: Vec<&masking_core::Rule> = after.rules().iter().filter(|r| r.enabled()).collect();
    let enabled_before: Vec<&masking_core::Rule> = before.rules().iter().filter(|r| r.enabled()).collect();

    let mut unresolved: Vec<&masking_core::Rule> = Vec::new();
    for old_rule in &enabled_before {
        let same_name_idx = remaining
            .iter()
            .position(|new_rule| new_rule.name() == old_rule.name() && rules_match_content(old_rule, new_rule));
        match same_name_idx {
            Some(idx) => {
                remaining.remove(idx);
            }
            None => unresolved.push(old_rule),
        }
    }

    unresolved
        .into_iter()
        .filter(|old_rule| match remaining.iter().position(|new_rule| rules_match_content(old_rule, new_rule)) {
            Some(idx) => {
                remaining.remove(idx);
                false
            }
            None => true,
        })
        .map(|old_rule| old_rule.name().to_string())
        .collect()
}

/// マスクルールが利用者に気付かれないまま無力化される(update_profileを直接叩く
/// 悪意あるスクリプトによる場合を含む)ことへの簡易な手がかりとして、保存対象が
/// アクティブプロファイルの場合に、無力化されたルール名を通知内容として組み立てる。
/// 正規の編集操作でも表示されるが、無言での無力化を防ぐ安価な仕組みとして機能する
/// (改ざん耐性のある記録ではなく、その場でユーザーに見える通知であることが
/// 重要なため、この関数は判定のみを行いイベント送出はコマンド本体側の責務とする)。
fn rule_weakening_notice(
    active_before_update: Option<&RuleProfile>,
    old_name: &str,
    new_profile: &RuleProfile,
) -> Option<serde_json::Value> {
    let active = active_before_update.filter(|active| active.profile_name() == old_name)?;
    let weakened = weakened_rule_names(active, new_profile);
    (!weakened.is_empty())
        .then(|| serde_json::json!({ "profileName": new_profile.profile_name(), "weakenedRuleNames": weakened }))
}

#[tauri::command]
pub async fn update_profile(
    app: tauri::AppHandle,
    state: tauri::State<'_, ProfileStoreState>,
    old_name: String,
    profile: RuleProfile,
) -> Result<(), String> {
    // アクティブプロファイルの読み取りと保存を同一のwith_store呼び出し(=同一のmutex
    // クリティカルセクション)内で行う。2回に分けると、その間に他のコマンド呼び出し
    // (set_active_profileによる切り替え等)が割り込み、判定時点と保存完了時点とで
    // 「どのプロファイルがアクティブか」がずれるTOCTOUが生じるため。
    let notice = with_store(&state, |store| {
        let active_before_update = store.active_profile()?;
        let notice = rule_weakening_notice(active_before_update.as_ref(), &old_name, &profile);
        store.update_profile(&old_name, &profile)?;
        Ok(notice)
    })?;
    let _ = app.emit("profiles-changed", ());
    if let Some(payload) = notice {
        let _ = app.emit("active-profile-rules-weakened", payload);
    }
    Ok(())
}

#[tauri::command]
pub async fn delete_profile(
    app: tauri::AppHandle,
    state: tauri::State<'_, ProfileStoreState>,
    name: String,
) -> Result<(), String> {
    with_store(&state, |store| store.delete_profile(&name))?;
    let _ = app.emit("profiles-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn set_active_profile(
    app: tauri::AppHandle,
    state: tauri::State<'_, ProfileStoreState>,
    name: String,
) -> Result<(), String> {
    with_store(&state, |store| store.set_active_profile(&name))?;
    let _ = app.emit("profiles-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn set_favorite(
    app: tauri::AppHandle,
    state: tauri::State<'_, ProfileStoreState>,
    name: String,
    is_favorite: bool,
) -> Result<(), String> {
    with_store(&state, |store| store.set_favorite(&name, is_favorite))?;
    let _ = app.emit("profiles-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn list_tags(state: tauri::State<'_, ProfileStoreState>) -> Result<Vec<String>, String> {
    with_store(&state, |store| store.list_tags())
}

#[tauri::command]
pub async fn create_tag(
    app: tauri::AppHandle,
    state: tauri::State<'_, ProfileStoreState>,
    name: String,
) -> Result<(), String> {
    with_store(&state, |store| store.create_tag(&name))?;
    let _ = app.emit("tags-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn rename_tag(
    app: tauri::AppHandle,
    state: tauri::State<'_, ProfileStoreState>,
    old_name: String,
    new_name: String,
) -> Result<(), String> {
    with_store(&state, |store| store.rename_tag(&old_name, &new_name))?;
    let _ = app.emit("tags-changed", ());
    // タグ名はプロファイル一覧のtagsフィールドにも反映されているため、両方に通知する。
    let _ = app.emit("profiles-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn delete_tag(
    app: tauri::AppHandle,
    state: tauri::State<'_, ProfileStoreState>,
    name: String,
) -> Result<(), String> {
    with_store(&state, |store| store.delete_tag(&name))?;
    let _ = app.emit("tags-changed", ());
    let _ = app.emit("profiles-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn set_profile_tags(
    app: tauri::AppHandle,
    state: tauri::State<'_, ProfileStoreState>,
    profile_name: String,
    tags: Vec<String>,
) -> Result<(), String> {
    with_store(&state, |store| store.set_profile_tags(&profile_name, &tags))?;
    let _ = app.emit("tags-changed", ());
    let _ = app.emit("profiles-changed", ());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use masking_core::{Mode, PatternType, Rule};

    fn rule(name: &str, pattern: &str, enabled: bool) -> Rule {
        Rule::new(name, PatternType::Literal, pattern, Mode::Fixed, Some("x".to_string()), None, enabled, None)
            .unwrap()
    }

    fn weakened_names(notice: &serde_json::Value) -> Vec<String> {
        notice["weakenedRuleNames"]
            .as_array()
            .expect("weakenedRuleNamesは配列のはず")
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn rule_weakening_notice_fires_when_an_active_profiles_enabled_rule_is_disabled() {
        let before = RuleProfile::new("work", None, vec![rule("a", "v", true), rule("b", "v", true)]).unwrap();
        let after = RuleProfile::new("work", None, vec![rule("a", "v", true), rule("b", "v", false)]).unwrap();

        let notice = rule_weakening_notice(Some(&before), "work", &after).expect("bが無効化されたので通知されるはず");
        assert_eq!(weakened_names(&notice), vec!["b"]);
    }

    #[test]
    fn rule_weakening_notice_fires_when_an_active_profiles_enabled_rule_is_removed() {
        let before = RuleProfile::new("work", None, vec![rule("a", "v", true), rule("b", "v", true)]).unwrap();
        let after = RuleProfile::new("work", None, vec![rule("a", "v", true)]).unwrap();

        let notice = rule_weakening_notice(Some(&before), "work", &after).expect("bが削除されたので通知されるはず");
        assert_eq!(weakened_names(&notice), vec!["b"]);
    }

    #[test]
    fn rule_weakening_notice_fires_when_an_enabled_rules_pattern_changes_while_it_stays_enabled() {
        // 有効ルール数は前後で1のまま変わらない。件数だけを見る判定では見逃すケース。
        let before = RuleProfile::new("work", None, vec![rule("a", "090-", true)]).unwrap();
        let after = RuleProfile::new("work", None, vec![rule("a", "nonsense", true)]).unwrap();

        let notice =
            rule_weakening_notice(Some(&before), "work", &after).expect("有効なままpatternが変わったので通知されるはず");
        assert_eq!(weakened_names(&notice), vec!["a"]);
    }

    #[test]
    fn rule_weakening_notice_fires_when_a_rule_is_disabled_while_another_is_enabled_to_keep_the_count_unchanged() {
        // 有効ルール数は前後で1のまま。異なる内容を持つaが無効化されbが有効化される
        // 「入れ替え」で件数ベースの判定を回避しようとしても、aのpattern("090-")を
        // 拾えるルールがafterに残っていないため、内容ベースでは検知できるはず。
        let before = RuleProfile::new("work", None, vec![rule("a", "090-", true), rule("b", "other", false)]).unwrap();
        let after = RuleProfile::new("work", None, vec![rule("a", "090-", false), rule("b", "other", true)]).unwrap();

        let notice = rule_weakening_notice(Some(&before), "work", &after)
            .expect("有効ルール数は変わらないがaのpatternを拾うルールが無くなったので通知されるはず");
        assert_eq!(weakened_names(&notice), vec!["a"]);
    }

    #[test]
    fn rule_weakening_notice_blames_the_rule_that_actually_disappeared_not_a_content_duplicate() {
        // aとspareは内容が完全に重複している。実際に削除されたのはaのみで、spareは
        // 手つかずのまま残る。消費順に引きずられてspareの方が消えたと誤報告しては
        // いけない(自分自身の名前に一致するものを優先して消費するはず)。
        let before = RuleProfile::new("work", None, vec![rule("a", "090-", true), rule("spare", "090-", true)]).unwrap();
        let after = RuleProfile::new("work", None, vec![rule("spare", "090-", true)]).unwrap();

        let notice =
            rule_weakening_notice(Some(&before), "work", &after).expect("aが削除されたので通知されるはず");
        assert_eq!(weakened_names(&notice), vec!["a"], "変化していないspareではなく、実際に消えたaを報告すべき");
    }

    #[test]
    fn rule_weakening_notice_is_silent_when_a_rule_is_only_renamed() {
        // 内容(pattern等)は変わらず名前だけが変わるリネームは、マスク挙動に
        // 影響しないため無力化として扱わないはず。
        let before = RuleProfile::new("work", None, vec![rule("phone", "090-", true)]).unwrap();
        let after = RuleProfile::new("work", None, vec![rule("phone_number", "090-", true)]).unwrap();

        let notice = rule_weakening_notice(Some(&before), "work", &after);
        assert!(notice.is_none(), "内容が同じままのリネームは通知しないはず");
    }

    #[test]
    fn rule_weakening_notice_is_silent_when_the_updated_profile_is_not_the_active_one() {
        // アクティブなのは"other"であり、更新対象の"work"では無い。
        let active_is_someone_else = RuleProfile::new("other", None, vec![rule("a", "v", true)]).unwrap();
        let after = RuleProfile::new("work", None, vec![]).unwrap();

        let notice = rule_weakening_notice(Some(&active_is_someone_else), "work", &after);
        assert!(notice.is_none(), "アクティブなのは別プロファイルなので通知しないはず");
    }

    #[test]
    fn rule_weakening_notice_is_silent_when_no_previously_enabled_rule_changed() {
        // bは新規追加。既存の有効ルールaはそのままなので、追加だけでは通知しないはず。
        let before = RuleProfile::new("work", None, vec![rule("a", "v", true)]).unwrap();
        let after = RuleProfile::new("work", None, vec![rule("a", "v", true), rule("b", "v", true)]).unwrap();

        let notice = rule_weakening_notice(Some(&before), "work", &after);
        assert!(notice.is_none(), "既存の有効ルールが変わっていないので通知しないはず");
    }

    #[test]
    fn rule_weakening_notice_is_silent_when_there_is_no_active_profile() {
        let after = RuleProfile::new("work", None, vec![]).unwrap();
        assert!(rule_weakening_notice(None, "work", &after).is_none());
    }

    #[test]
    fn with_store_fails_clearly_before_initialize_store_has_run() {
        let state = ProfileStoreState::default();
        let err = with_store(&state, |store| store.list_profiles()).expect_err("未初期化のはず");
        assert_eq!(err, ProfileStoreError::NotInitialized.to_string());
    }

    #[test]
    fn resolve_paths_with_override_uses_given_directory_when_present() {
        let paths = resolve_paths_with_override(Some("e2e-test-data".to_string()))
            .expect("overrideありなら常に成功するはず");
        let expected_base = std::path::Path::new("e2e-test-data");
        assert_eq!(paths.key_path, expected_base.join("key.bin"));
        assert_eq!(paths.db_path, expected_base.join("profiles.db"));
    }

    #[test]
    fn resolve_paths_with_override_falls_back_to_os_data_dir_when_absent() {
        let paths =
            resolve_paths_with_override(None).expect("OS標準パスの解決自体は失敗しないはず");
        assert!(paths.key_path.ends_with("key.bin"));
        assert!(paths.db_path.ends_with("profiles.db"));
    }
}
