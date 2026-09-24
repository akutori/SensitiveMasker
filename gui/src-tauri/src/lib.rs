mod clipboard;
mod env_import;
mod export_import;
mod instance_settings;
mod masking;
mod profiles;
mod text_file_io;
mod tray;
#[cfg(windows)]
mod webview_setup;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default();

    // Single Instanceプラグインは最初に登録しないと正しく機能しない(公式ドキュメントの制約)。
    // トレイメニューの「複数起動を許可」がOFF(既定)の場合のみ登録し、2つ目のプロセスが
    // 起動されたら1つ目のウィンドウを前面に出す(2つ目自身はプラグインが自動的に終了させる)。
    // mobileではCargo.toml側で依存自体がビルド対象に含まれないため#[cfg(desktop)]で囲む。
    // e2e-testing featureでは常に登録しない: このプラグインのOS側の識別(Windowsの名前付き
    // ミューテックス・LinuxのD-Bus名・macOSのソケットパス)はtauri.conf.jsonのidentifier由来で
    // E2E用configでも変わらない。開発機で実ユーザーのインスタンスが常駐している間にE2Eを
    // 起動すると、識別が衝突し、E2E側は新規ウィンドウを作らず実インスタンスへフォーカスを
    // 譲って即終了してしまう(WebDriverのアタッチ先が無くなる)。
    #[cfg(all(desktop, not(feature = "e2e-testing")))]
    {
        let allow_multiple_instances = instance_settings::InstanceSettingsPath::resolve()
            .map(|settings| settings.allow_multiple_instances())
            .unwrap_or(false);
        if !allow_multiple_instances {
            builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
                tray::show_main_window(app);
            }));
        }
    }

    builder = builder
        .plugin(tauri_plugin_dialog::init())
        // フロントエンドからはこのプラグイン自身のコマンド(plugin:clipboard-manager|*)を
        // 一切invokeしない(clipboard::write_clipboard_text/clear_clipboard_if_matchesの
        // 内部でRustから直接呼ぶのみ)。そのためcapabilitiesにclipboard-manager:*の許可は
        // 不要(ACLはinvoke経由の呼び出しのみを対象とするため)。
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        // 第2引数(自動起動時の追加コマンドライン引数)は不要なのでNone。
        // MacosLauncherはWindows/Linuxでは無視されるが型としては要求される。
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .manage(masking::MaskingState::default())
        .manage(profiles::ProfileStoreState::default())
        .manage(export_import::PendingImportState::default())
        .manage(clipboard::ClipboardState::default())
        .setup(|app| {
            tray::setup(app)?;
            #[cfg(windows)]
            webview_setup::disable_browser_accelerator_keys(app);
            Ok(())
        })
        // ページを読み込み直すと、確認画面が持つ保留の識別子が失われるため、Rust側の保留を全て破棄する
        // (判定は、discard_pending_imports_on_page_load)。
        .on_page_load(|webview, payload| {
            export_import::discard_pending_imports_on_page_load(
                &webview.state::<export_import::PendingImportState>(),
                webview.label(),
                payload.event(),
            );
        })
        .on_window_event(tray::handle_window_event);

    // e2e-testing feature + debug buildの両方を要求する(release buildでは
    // featureを指定しても依存自体がコンパイル対象に入らないためこのブロック自体が
    // 存在しなくなるが、万一の指定漏れに備えdebug_assertionsも二重に要求する)。
    // tauri-plugin-wdio-webdriverは認証無しのWebDriverサーバーを起動するため、
    // 配布用ビルドに含めてはならない。
    #[cfg(all(debug_assertions, feature = "e2e-testing"))]
    {
        builder = builder
            .plugin(tauri_plugin_wdio::init())
            .plugin(tauri_plugin_wdio_webdriver::init());
    }

    builder
        .invoke_handler(tauri::generate_handler![
            masking::mask_text,
            masking::clear_mappings,
            profiles::is_store_initialized,
            profiles::open_store,
            profiles::initialize_store,
            profiles::list_profiles,
            profiles::get_profile,
            profiles::create_profile,
            profiles::update_profile,
            profiles::delete_profile,
            profiles::set_active_profile,
            profiles::set_favorite,
            profiles::list_tags,
            profiles::create_tag,
            profiles::rename_tag,
            profiles::delete_tag,
            profiles::set_profile_tags,
            export_import::export_profile_to_file,
            export_import::export_all_to_file,
            export_import::export_profile_with_key_file,
            export_import::export_all_with_key_file,
            export_import::detect_import_method,
            export_import::preview_import_with_key_file,
            export_import::preview_import,
            export_import::commit_pending_import,
            export_import::clear_pending_import,
            clipboard::write_clipboard_text,
            clipboard::write_clipboard_text_untracked,
            clipboard::clear_clipboard_if_matches,
            text_file_io::read_text_file,
            text_file_io::write_text_file,
            env_import::preview_env_import,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // クリップボードの後始末(clipboard::clear_pending_on_exit)は、ここではなく、トレイの「終了」が、終了する前に
            // 行う(終了の通知の時点では、クリップボードのプラグインが、自身のクリップボードを手放していて、操作できない)。
            export_import::discard_pending_imports_on_run_event(
                &app.state::<export_import::PendingImportState>(),
                &event,
            );
        });
}
