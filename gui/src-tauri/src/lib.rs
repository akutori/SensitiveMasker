mod masking;
mod profiles;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(masking::MaskingState::default())
        .manage(profiles::ProfileStoreState::default())
        .invoke_handler(tauri::generate_handler![
            greet,
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
