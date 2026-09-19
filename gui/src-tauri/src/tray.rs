use tauri::{
    menu::{CheckMenuItemBuilder, Menu, MenuBuilder, MenuEvent, SubmenuBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, Runtime, WindowEvent,
};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_notification::NotificationExt as _;

use crate::profiles::{with_store, ProfileStoreState};

pub(crate) const MAIN_WINDOW_LABEL: &str = "main";
const TRAY_ID: &str = "main-tray";
const OPEN_ID: &str = "open";
const QUIT_ID: &str = "quit";
const MASK_CLIPBOARD_ID: &str = "mask_clipboard";
const TOGGLE_AUTOSTART_ID: &str = "toggle_autostart";
const PROFILE_MENU_ID_PREFIX: &str = "profile:";

/// トレイアイコン+メニューを構築する(モックアップ「9_常駐トレイメニュー」の
/// 「開く」「終了」に加え、クリップボードからの直接マスク・プロファイル切り替え・
/// 自動起動を追加)。プロファイル一覧・アクティブ状態・自動起動の有効状態は
/// アプリ起動時点では確定していない(ストアはフロントエンド起動後に開かれる)ため、
/// 実際のメニュー内容は`refresh_menu`が担い、ここでは器(TrayIcon)を作るだけ。
pub fn setup<R: Runtime>(app: &tauri::App<R>) -> tauri::Result<()> {
    let menu = build_menu(app.handle())?;

    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        // 右クリックでのメニュー表示はOS標準のまま。左クリックは自前でウィンドウ復帰に
        // 割り当てる(モックアップの「Windows/macOSは左クリックで復帰」に合わせるため)。
        .show_menu_on_left_click(false)
        .on_menu_event(handle_menu_event)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } =
                event
            {
                show_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }

    tray.build(app)?;
    Ok(())
}

/// プロファイル一覧・アクティブプロファイル・自動起動の有効状態が変わりうる操作
/// (ストアを開く/初期化する、プロファイルの作成・更新・削除・切り替え、自動起動の
/// トグル)の後に呼び、トレイメニュー全体を最新の状態で作り直す。Tauriのトレイ
/// メニューはネイティブに構築された静的な構造であり、開かれる直前に内容を問い合わせる
/// フックが無いため、状態が変わるたびにメニュー全体を再構築して`set_menu`で
/// 差し替える方式を取る。メニュー再構築自体の失敗はUI同期の失敗に留まり、呼び出し元の
/// 本来の操作(プロファイル作成等)を失敗させるべきではないため、エラーは握りつぶす。
pub fn refresh_menu<R: Runtime>(app: &AppHandle<R>) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let Ok(menu) = build_menu(app) else { return };
    let _ = tray.set_menu(Some(menu));
}

/// プロファイル一覧からアクティブなものを1件探す(高々1件という前提はDB側の
/// `upsert_active_profile_name`が保証する)。`build_menu`(チェック付与・有効/無効判定)と
/// `mask_clipboard`(対象プロファイルの決定)の両方で同じ判定を使うための共有ロジック。
fn find_active_profile(profiles: &[profile_store::ProfileSummary]) -> Option<&profile_store::ProfileSummary> {
    profiles.iter().find(|p| p.is_active)
}

fn build_menu<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let profiles_state = app.state::<ProfileStoreState>();
    // 未初期化(セットアップ未完了)の間はlist_profilesがErrになる。トレイの起動時点
    // (フロントエンドがストアを開く前)は常にこの状態を通るため、エラーではなく
    // 「プロファイル無し」として扱う。
    let (profiles, has_active_profile) = with_store(&profiles_state, |store| store.list_profiles())
        .map(|profiles| {
            let has_active = find_active_profile(&profiles).is_some();
            (profiles, has_active)
        })
        .unwrap_or_default();

    let profile_submenu = SubmenuBuilder::new(app, "プロファイル切り替え").enabled(!profiles.is_empty());
    let profile_items: Vec<_> = profiles
        .iter()
        .map(|p| {
            CheckMenuItemBuilder::with_id(format!("{PROFILE_MENU_ID_PREFIX}{}", p.name), &p.name)
                .checked(p.is_active)
                .build(app)
        })
        .collect::<tauri::Result<_>>()?;
    let profile_submenu =
        profile_items.iter().fold(profile_submenu, |submenu, item| submenu.item(item)).build()?;

    let autostart_enabled = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart_item = CheckMenuItemBuilder::with_id(TOGGLE_AUTOSTART_ID, "自動起動")
        .checked(autostart_enabled)
        .build(app)?;

    MenuBuilder::new(app)
        .text(OPEN_ID, "開く")
        .text(MASK_CLIPBOARD_ID, "クリップボードをマスク")
        .item(&profile_submenu)
        .item(&autostart_item)
        .separator()
        .text(QUIT_ID, "終了")
        .build()
        .map(|menu| {
            // メニュー全体のenabledは無いため、依存する項目のみ個別に無効化する。
            let _ = menu
                .get(MASK_CLIPBOARD_ID)
                .and_then(|item| item.as_menuitem().cloned())
                .map(|item| item.set_enabled(has_active_profile));
            menu
        })
}

fn handle_menu_event<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    let id = event.id().as_ref();
    if let Some(name) = id.strip_prefix(PROFILE_MENU_ID_PREFIX) {
        switch_active_profile(app, name);
        return;
    }
    match id {
        OPEN_ID => show_main_window(app),
        QUIT_ID => {
            // JS側のクリップボード自動クリアタイマーはプロセス終了と共に消えるため、
            // コピー直後に終了されると保留中のクリアが実行されないままパスフレーズが
            // 残り続ける。終了前に一度だけ、追跡している値のクリアを試みる。
            crate::clipboard::clear_pending_on_exit(app, &app.state::<crate::clipboard::ClipboardState>());
            app.exit(0);
        }
        MASK_CLIPBOARD_ID => mask_clipboard(app),
        TOGGLE_AUTOSTART_ID => toggle_autostart(app),
        _ => {}
    }
}

fn switch_active_profile<R: Runtime>(app: &AppHandle<R>, name: &str) {
    let profiles_state = app.state::<ProfileStoreState>();
    match with_store(&profiles_state, |store| store.set_active_profile(name)) {
        Ok(()) => {
            let _ = app.emit("profiles-changed", ());
        }
        // ストア未初期化・対象プロファイルが既に削除済み等。mask_clipboard/toggle_autostart
        // と同様、無反応に見えないようエラーを通知する。
        Err(_) => notify_error(app, "プロファイルを切り替えられませんでした"),
    }
    // 失敗時もメニューを最新の状態に作り直す。チェックが付いたままにしないため。
    refresh_menu(app);
}

fn toggle_autostart<R: Runtime>(app: &AppHandle<R>) {
    let autolaunch = app.autolaunch();
    let currently_enabled = autolaunch.is_enabled().unwrap_or(false);
    let result = if currently_enabled { autolaunch.disable() } else { autolaunch.enable() };
    if result.is_err() {
        notify_error(app, "自動起動の設定を変更できませんでした");
    }
    refresh_menu(app);
}

/// アクティブプロファイルでクリップボードの内容をマスクし、結果をクリップボードへ
/// 書き戻す。パスフレーズ等の秘匿情報とは異なり、マスク済みテキストは外部LLMへ
/// 貼り付けることが目的の通常の出力のため、`clipboard::ClipboardState`による
/// 自動クリア対象にはしない(そのまま普通にクリップボードへ残す)。
fn mask_clipboard<R: Runtime>(app: &AppHandle<R>) {
    let profiles_state = app.state::<ProfileStoreState>();
    let active = with_store(&profiles_state, |store| {
        let profiles = store.list_profiles()?;
        let Some(summary) = find_active_profile(&profiles) else {
            return Ok(None);
        };
        let profile = store.get_profile(&summary.name)?;
        Ok(Some((summary.id, profile)))
    });

    let (profile_id, profile) = match active {
        Ok(Some(pair)) => pair,
        Ok(None) => {
            notify_error(app, "アクティブなプロファイルが設定されていません");
            return;
        }
        Err(_) => {
            notify_error(app, "プロファイルを読み込めませんでした");
            return;
        }
    };

    let Ok(text) = app.clipboard().read_text() else {
        notify_error(app, "クリップボードからテキストを読み取れませんでした");
        return;
    };

    let masking_state = app.state::<crate::masking::MaskingState>();
    let masked = crate::masking::mask_text_for_tray(&masking_state, profile_id.to_string(), &profile, &text);

    let clipboard_state = app.state::<crate::clipboard::ClipboardState>();
    if crate::clipboard::write_untracked(app, &clipboard_state, &masked).is_err() {
        notify_error(app, "マスク結果をクリップボードへ書き込めませんでした");
    }
}

fn notify_error<R: Runtime>(app: &AppHandle<R>, message: &str) {
    let _ = app.notification().builder().title("SensitiveMasker").body(message).show();
}

fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        return;
    };
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

/// メイン画面の「閉じる」(Xボタン)ではアプリを終了せずトレイに格納する。
/// 実際の終了はトレイメニューの「終了」からのみ行う。
/// on_window_eventはBuilder全体に登録され全ウィンドウへ適用されるため、
/// メインウィンドウ以外(復帰手段を持たない)を誤って隠さないようlabelで絞る。
pub fn handle_window_event<R: Runtime>(window: &tauri::Window<R>, event: &WindowEvent) {
    if window.label() != MAIN_WINDOW_LABEL {
        return;
    }
    if let WindowEvent::CloseRequested { api, .. } = event {
        api.prevent_close();
        let _ = window.hide();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(name: &str, is_active: bool) -> profile_store::ProfileSummary {
        profile_store::ProfileSummary {
            id: 1,
            name: name.to_string(),
            rule_count: 0,
            enabled_rule_count: 0,
            is_favorite: false,
            is_active,
            updated_at: String::new(),
            tags: Vec::new(),
        }
    }

    #[test]
    fn find_active_profile_returns_none_for_an_empty_list() {
        assert!(find_active_profile(&[]).is_none());
    }

    #[test]
    fn find_active_profile_returns_none_when_no_profile_is_active() {
        let profiles = vec![summary("a", false), summary("b", false)];
        assert!(find_active_profile(&profiles).is_none());
    }

    #[test]
    fn find_active_profile_finds_the_one_active_profile_among_others() {
        let profiles = vec![summary("a", false), summary("b", true), summary("c", false)];
        assert_eq!(find_active_profile(&profiles).map(|p| p.name.as_str()), Some("b"));
    }
}
