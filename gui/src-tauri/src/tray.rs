use tauri::{
    menu::MenuBuilder,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, Runtime, WindowEvent,
};

const MAIN_WINDOW_LABEL: &str = "main";

/// トレイアイコン+メニューを構築する。メニューは「開く」「終了」の2項目のみ
/// (モックアップ「9_常駐トレイメニュー」通り)。
pub fn setup<R: Runtime>(app: &tauri::App<R>) -> tauri::Result<()> {
    let menu = MenuBuilder::new(app).text("open", "開く").text("quit", "終了").build()?;

    let mut tray = TrayIconBuilder::new()
        .menu(&menu)
        // 右クリックでのメニュー表示はOS標準のまま。左クリックは自前でウィンドウ復帰に
        // 割り当てる(モックアップの「Windows/macOSは左クリックで復帰」に合わせるため)。
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
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
