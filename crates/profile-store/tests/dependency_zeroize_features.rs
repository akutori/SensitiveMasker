//! `chacha20poly1305`のzeroize featureは自身とchacha20へは転送するが、内部で使う
//! poly1305へは転送しない(`chacha20poly1305`のCargo.tomlで`zeroize = ["dep:zeroize",
//! "chacha20/zeroize"]`となっている)。そのため`crates/profile-store/Cargo.toml`側で
//! poly1305へ直接`features = ["zeroize"]`を指定し、バージョン一致によるCargoの
//! feature unificationに頼って有効化している。この一致はコンパイルエラーにはならずに
//! 崩れうる(chacha20poly1305が将来poly1305のメジャーバージョンを上げた場合等)ため、
//! 実際に解決されたfeatureを`cargo tree`で確認し、崩れていたらテストを失敗させる。

#[test]
fn poly1305_zeroize_feature_stays_unified_with_chacha20poly1305s_dependency() {
    // cargo treeの出力の飾り(枝の記号・色)は、環境変数(CARGO_TERM_COLOR=alwaysをCIのRustのセットアップが設定する。
    // CARGO_TERM_UNICODEは枝の記号の文字種を変える)で変わり、行頭の照合が外れるため、飾りの無い形で出力させる。
    let output = std::process::Command::new(env!("CARGO"))
        .args(["tree", "--prefix", "none", "--color", "never"])
        .args(["-p", "chacha20poly1305@0.11.0", "--depth", "1", "-f", "{p} {f}", "-e", "normal"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo treeの実行に失敗した");
    assert!(
        output.status.success(),
        "cargo treeが失敗した(chacha20poly1305のバージョンが変わった場合はこのテスト自体の\
         バージョン指定も更新すること): {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let poly1305_line = stdout
        .lines()
        .find(|line| line.starts_with("poly1305 v"))
        .unwrap_or_else(|| panic!("poly1305がchacha20poly1305の直接依存として見つからない:\n{stdout}"));

    assert!(
        poly1305_line.contains("zeroize"),
        "poly1305のzeroize featureが有効化されていない(バージョンずれでfeature unification\
         が崩れた可能性がある): {poly1305_line}"
    );
}
