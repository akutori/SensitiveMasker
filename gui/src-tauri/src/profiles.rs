use std::sync::Mutex;

use masking_core::RuleProfile;
use profile_store::{ProfileStore, ProfileStoreError, ProfileSummary};
use tauri::Emitter;

#[derive(Default)]
pub struct ProfileStoreState(Mutex<Option<ProfileStore>>);

// tauri::Stateに依存しない形にして単体テスト可能にする(masking.rsのmask_text_with_stores
// と同じ方針)。呼び出し側は&tauri::State<'_, ProfileStoreState>のままDeref経由で渡せる。
fn with_store<T>(
    state: &ProfileStoreState,
    f: impl FnOnce(&mut ProfileStore) -> Result<T, ProfileStoreError>,
) -> Result<T, String> {
    let mut guard = state.0.lock().unwrap_or_else(|e| e.into_inner());
    let store = guard.as_mut().ok_or_else(|| ProfileStoreError::NotInitialized.to_string())?;
    f(store).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn is_store_initialized() -> Result<bool, String> {
    profile_store::is_initialized().map_err(|e| e.to_string())
}

fn open_into_state(state: &ProfileStoreState) -> Result<(), String> {
    let store = ProfileStore::open().map_err(|e| e.to_string())?;
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
    profile_store::init().map_err(|e| e.to_string())?;
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

#[tauri::command]
pub async fn update_profile(
    app: tauri::AppHandle,
    state: tauri::State<'_, ProfileStoreState>,
    old_name: String,
    profile: RuleProfile,
) -> Result<(), String> {
    with_store(&state, |store| store.update_profile(&old_name, &profile))?;
    let _ = app.emit("profiles-changed", ());
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

    #[test]
    fn with_store_fails_clearly_before_initialize_store_has_run() {
        let state = ProfileStoreState::default();
        let err = with_store(&state, |store| store.list_profiles()).expect_err("未初期化のはず");
        assert_eq!(err, ProfileStoreError::NotInitialized.to_string());
    }
}
