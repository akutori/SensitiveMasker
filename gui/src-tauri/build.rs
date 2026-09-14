fn main() {
    // アプリ独自コマンドはACLマニフェスト(__app-acl__)を生成しない限りACLの対象外で、
    // capabilities/default.jsonへの記載の有無に関わらず無条件に呼べてしまう。
    // ここで全コマンドを明示列挙しallow-$command/deny-$commandを自動生成させることで、
    // capabilities側での許可を必須(既定deny)にする。
    //
    // 注意: tauri.e2e.conf.json(--configで指定するオーバーレイ)は手書きしない。
    // Tauriの設定マージはJSON Merge Patchで配列がパッチ側にあれば丸ごと置換されるため、
    // security.capabilitiesを手書きすると本体側(このtauri.conf.json)の変更に追従できず
    // 権限が黙って落ちる。gui/scripts/generate-e2e-config.tsが本体のcapabilitiesから
    // 動的に生成する(bun run e2e:buildが自動実行する)。
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&[
            "mask_text",
            "clear_mappings",
            "is_store_initialized",
            "open_store",
            "initialize_store",
            "list_profiles",
            "get_profile",
            "create_profile",
            "update_profile",
            "delete_profile",
            "set_active_profile",
            "set_favorite",
            "list_tags",
            "create_tag",
            "rename_tag",
            "delete_tag",
            "set_profile_tags",
            "export_profile_to_file",
            "export_all_to_file",
            "preview_import",
            "commit_pending_import",
            "clear_pending_import",
            "write_clipboard_text",
            "write_clipboard_text_untracked",
            "clear_clipboard_if_matches",
            "read_text_file",
            "write_text_file",
            "preview_env_import",
        ])),
    )
    .expect("Tauriのビルド設定(ACLマニフェスト生成)に失敗しました");
}
