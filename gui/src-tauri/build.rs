fn main() {
    // Windows(MSVC)のデバッグビルドは、comctl32 v6のマニフェストを、tauri-buildのリソースでなく、リンカーで
    // 埋め込む(理由はembeds_manifest_by_linker)。
    let manifest_by_linker = embeds_manifest_by_linker();
    let windows_attributes = if manifest_by_linker {
        tauri_build::WindowsAttributes::new_without_app_manifest()
    } else {
        tauri_build::WindowsAttributes::new()
    };

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
        tauri_build::Attributes::new().windows_attributes(windows_attributes).app_manifest(tauri_build::AppManifest::new().commands(&[
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

    if manifest_by_linker {
        embed_comctl32_v6_manifest();
    }
}

/// Windows(MSVC)のデバッグビルドかどうか。この場合だけ、comctl32 v6のマニフェストを、tauri-buildのリソースでなく、
/// リンカーで、全てのターゲットへ埋め込む。
///
/// tauri::testでアプリを組み立てるテストは、comctl32のv6専用の関数(TaskDialogIndirect)をリンクするため、
/// 実行ファイルにマニフェストが無いと、起動時にSTATUS_ENTRYPOINT_NOT_FOUNDで落ちる。tauri-buildが埋め込む
/// リソースは、バイナリ(bins)にだけ付き、ライブラリ(gui_lib)の単体テストの実行ファイルには付かない。
/// 両方から埋め込むと、バイナリでマニフェストが重複する(リンカーのCVT1100)ため、デバッグビルドでは、
/// tauri-buildの側を止めて、リンカーの側に一本化する。
/// リリースビルドは、tauri-buildの既定のまま(配布物のマニフェストの埋め込み方は変えない)。そのため、
/// Windowsでは、リリースプロファイルでのcargo testで、tauri::testを使うテストは起動しない。
fn embeds_manifest_by_linker() -> bool {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let profile = std::env::var("PROFILE").unwrap_or_default();
    target_os == "windows" && target_env == "msvc" && profile == "debug"
}

/// comctl32 v6への依存だけを宣言する、アプリケーションマニフェスト(tauri-buildの既定のマニフェストと同じ内容)。
const COMCTL32_V6_MANIFEST: &str = r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*"
      />
    </dependentAssembly>
  </dependency>
</assembly>
"#;

/// COMCTL32_V6_MANIFESTを、リンカーで、全てのターゲット(バイナリ・ライブラリの単体テストの実行ファイル等)へ
/// 埋め込む。リンカーが既定で足す実行レベル(trustInfo)は、tauri-buildのマニフェストと同じ内容にするため、外す。
fn embed_comctl32_v6_manifest() {
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIRが設定されていません"));
    let manifest_path = out_dir.join("comctl32-v6.manifest");
    std::fs::write(&manifest_path, COMCTL32_V6_MANIFEST).expect("マニフェストを書き出せませんでした");
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTUAC:NO");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest_path.display());
}
