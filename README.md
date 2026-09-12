# SensitiveMasker

電話番号・パスワード・IPアドレス等の機微情報を含む任意のテキスト(ログ・コンソール出力等)を、
外部LLMに貼り付ける前にローカルで自動マスキングするツールです。

GUI・CLI・MCPサーバーの3つのインターフェースを提供し、マスキングルールとロジックは全てから共有されます。
ルールプロファイルはローカルのSQLiteに暗号化して保存され、外部への通信は一切行いません。

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

## MCPサーバー

`masker-mcp`はstdioトランスポートのMCPサーバーです。ビルドした実行ファイルをMCPクライアントの設定に登録して使います。

```bash
cargo build -p masker-mcp --release
```

提供するツールは`run_masked_command`(任意のシェルコマンドを実行し、標準出力を`masker`でマスクしてから返す)のみです。

## 開発

```bash
# ワークスペース全体のビルド・テスト
cargo build --workspace
cargo test --workspace

# フロントエンドの型チェック
cd gui && bunx tsc --noEmit

# E2Eテスト(WebDriver。e2e-testing featureでdebugビルドしてから実行する)
cd gui && bun run e2e:build && bun run e2e
```

アーキテクチャ・技術スタック・セキュリティ設計・開発方針の詳細は [CLAUDE.md](CLAUDE.md) を参照してください。
