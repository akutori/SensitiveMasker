# SensitiveMasker

<div align="center">
  <img src="gui/src-tauri/icons/icon.png" width="128" height="128" alt="アプリアイコン">
</div>

電話番号・パスワード・IPアドレス等の機微情報を含む任意のテキスト(ログ・コンソール出力等)を、
外部LLMに貼り付ける前にローカルで自動マスキングするデスクトップツールです(Windows/macOS/Linux対応)。

GUI・CLI・MCPサーバーの3つのインターフェースを提供し、マスキングルールとロジックは全てから共有されます。
ルールプロファイルはローカルのSQLiteに暗号化して保存され、外部への通信は一切行いません。

アーキテクチャや開発方針の詳細は [CLAUDE.md](CLAUDE.md) を参照してください。

## ダウンロード(配布版)

開発環境なしで使いたい場合は、[GitHub Releases](https://github.com/akutori/SensitiveMasker/releases/latest)
から各OS向けのファイルをダウンロードしてください。

| ファイル | 内容 |
|---|---|
| `SensitiveMasker_*-setup.exe` / `*.msi`(Windows) | GUIインストーラー(`masker`/`masker-mcp`同梱) |
| `SensitiveMasker_*_universal.dmg`(macOS) | GUIインストーラー(Universal Binary、`masker`/`masker-mcp`同梱) |
| `SensitiveMasker_*.deb` / `*.AppImage`(Linux) | GUIインストーラー / 単独実行ファイル(`masker`/`masker-mcp`同梱) |
| `masker-<OS>`(`.exe`はWindowsのみ) | CLI単独実行ファイル |
| `masker-mcp-<OS>`(`.exe`はWindowsのみ) | MCPサーバー単独実行ファイル |

GUIインストーラーには`masker`/`masker-mcp`も同梱されており、インストール先ディレクトリに
そのまま配置されます。GUIを使わずCLI/MCPサーバーだけ欲しい場合は、`masker-<OS>`/`masker-mcp-<OS>`を
個別にダウンロードしてもそのまま実行できます(ビルド不要)。

### CLIをターミナルから呼べるようにする(PATHへの追加)

インストールしただけでは`masker`コマンドはターミナルのどこからでも呼べません(MCPサーバーの設定は
絶対パス指定が前提のため、`masker-mcp`はこの対応は不要です)。既定のインストール先を前提にした
手順は以下の通りです。

**Windows**

インストーラーの種類によって既定のインストール先が異なります。

- `*-setup.exe`(既定、管理者権限不要): `%LOCALAPPDATA%\SensitiveMasker\`
- `*.msi`: `C:\Program Files\SensitiveMasker\`

PowerShellで(setup.exe版の場合、管理者権限不要):

```powershell
[Environment]::SetEnvironmentVariable("Path", "$env:LOCALAPPDATA\SensitiveMasker;" + [Environment]::GetEnvironmentVariable("Path", "User"), "User")
```

GUIから設定する場合は「設定 → システム → バージョン情報 → システムの詳細設定 → 環境変数」で
ユーザー環境変数の`Path`に上記いずれかのフォルダを追加してください。設定後はターミナルの再起動が必要です。

**macOS**

`/Applications`にドラッグした場合、`masker`は`/Applications/SensitiveMasker.app/Contents/MacOS/masker`
に配置されます。`~/.zshrc`等に追加してください。

```bash
echo 'export PATH="/Applications/SensitiveMasker.app/Contents/MacOS:$PATH"' >> ~/.zshrc
```

**Linux(.deb / .rpm)**

`masker`/`masker-mcp`は`/usr/bin/`に直接インストールされ、標準で全ユーザーのPATHに含まれるため、
追加の設定は不要です。

## インターフェース

| インターフェース | 用途 |
|---|---|
| GUI (`gui/`) | プロファイル・ルールの作成/編集、テキストの貼り付けマスク、常駐トレイからのクリップボード直接マスク |
| CLI (`masker`) | `cat file.log \| masker mask` のようなパイプ処理、バッチ/ストリーム処理、CI等での自動化 |
| MCPサーバー (`masker-mcp`) | `terraform`・`asterisk -rx`・`tail -f`等、機微情報が出力に混じりうるコマンドをMCPクライアント(Claude Code等)経由で安全に実行する |

## セットアップ

初回はいずれかのインターフェースから鍵とデータベースを初期化します(GUIは初回起動画面から、CLIは`masker init`)。
同じ鍵・データベースをGUI/CLI/MCPサーバーの全てで共有します。

## CLI

```bash
# ビルド(以降 target/release/masker、またはcargo run -p masker --release -- で実行)
cargo build -p masker --release

masker init

# アクティブプロファイルでマスクして標準出力へ
cat app.log | masker mask

# プロファイル管理
masker profile list
masker profile use <name>
```

コマンドの詳細は [docs/cli/README.md](docs/cli/README.md) を参照してください。

## GUI

```bash
cd gui
bun install
bun run tauri dev
```

配布用ビルドは `bun run tauri build`。フロントエンドはReact + TypeScript + Vite、コンポーネントは
Storybook(`bun run storybook`)で個別に確認できます。

画面・機能の詳細は [docs/gui/README.md](docs/gui/README.md) を参照してください。

## MCPサーバー

`masker-mcp`はstdioトランスポートのMCPサーバーです。ビルドした実行ファイルをMCPクライアントの設定に登録して使います。

```bash
cargo build -p masker-mcp --release
```

提供するツールは`run_masked_command`(任意のシェルコマンドを実行し、標準出力を`masker`でマスクしてから返す)のみです。
セットアップ方法・引数の詳細は [docs/mcp/README.md](docs/mcp/README.md) を参照してください。

## 開発

```bash
# ワークスペース全体のビルド・テスト
cargo build --workspace
cargo test --workspace

# フロントエンドの型チェック
cd gui && bunx tsc --noEmit

# フロントエンドのテスト(純関数の単体テストとStorybookのplayテスト。それぞれ単独でも実行できる)
# playテストはPlaywrightのChromiumを使う。未導入なら`bunx playwright install chromium`が必要
cd gui && bun run test
cd gui && bun run test:unit
cd gui && bun run test:stories

# E2Eテスト(WebDriver。e2e-testing featureでdebugビルドしてから実行する)
cd gui && bun run e2e:build && bun run e2e

# 配布用のフロントエンドに、E2E専用のコードが混入していないことの確認(先に配布用のビルドが必要。
# e2e:buildの後のdistは、E2E専用のコードを含むため、このコマンドは失敗する)
cd gui && bun run build && bun run check:dist
```

タグ(`v*`)をpushすると、GitHub Actions(`.github/workflows/release.yml`)が、先に検証
(`.github/workflows/verify.yml`: フロントエンドの型検査・単体テスト・Storybookのplayテスト、
E2E専用コードの混入確認、`cargo test --workspace --locked`)を行い、通った場合に限り、Windows/macOS/Linux向けの
GUIインストーラーとCLI/MCPサーバーの実行ファイルをビルドし、GitHub Releasesに公開します。
検証だけを、タグを打つ前に、GitHubのActions画面(Verify)から手動で実行することもできます。

## ディレクトリ構成

```
crates/
  masking-core/   # 副作用のないマスキングロジック(Functional Core)
  profile-store/  # SQLite永続化・暗号化・鍵管理・export/import(Imperative Shell)
  masker/         # clap CLI(バイナリ名 masker)
  masker-mcp/     # MCPサーバー(run_masked_commandツール)
gui/              # Tauri + React GUI
  src/            # フロントエンド(コンポーネント・ルート・状態管理)
  src-tauri/      # Tauriコマンド・トレイ・クリップボード連携
  e2e/            # WebDriverベースのE2Eテスト
docs/cli/         # masker CLIのコマンド仕様
docs/gui/         # GUIの画面・機能一覧
docs/mcp/         # masker-mcpのツール仕様
poc/              # 使い捨てのPoC用(workspaceのmemberに含めない)
```

アーキテクチャ・技術スタック・セキュリティ設計・開発方針の詳細は [CLAUDE.md](CLAUDE.md) を参照してください。
