mod masking;
mod profiles;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(masking::MaskingState::default())
        .manage(profiles::ProfileStoreState::default());

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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
