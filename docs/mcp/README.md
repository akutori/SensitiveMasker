# masker-mcp ツール一覧

`masker-mcp`はstdioトランスポートのMCPサーバー。`masking-core`/`profile-store`には依存せず、対象コマンドの出力を`masker mask --stream`にパイプするだけの薄い層として実装されている(理由は[CLAUDE.md](../../CLAUDE.md#アーキテクチャ原則疎結合)を参照)。提供するツールは`run_masked_command`のみ。

## run_masked_command

指定したシェルコマンドを実行し、その標準出力を`masker mask --stream`経由でマスクしてから返す。`terraform`・`asterisk -rx`・`fs_cli`・`tail -f`等、機微情報が出力に混じりうるコマンドの実行を想定している。

| 引数 | 必須 | 内容 |
|---|---|---|
| `command` | 必須 | 実行するシェルコマンド。空文字列・改行を含む文字列は拒否される(`cmd.exe /C`が埋め込まれた改行以降を無警告で無視するため) |
| `timeout_seconds` | 省略可 | 収集を打ち切るまでの秒数(既定300秒。3600秒を超える値は3600秒に丸められる) |
| `profile` | 省略可 | 使用するプロファイル名(省略時はアクティブプロファイル) |
| `encoding` | 省略可 | 対象コマンドの出力のエンコーディング([WHATWGラベル名](https://encoding.spec.whatwg.org/#names-and-labels)、例: `shift-jis`。省略時はUTF-8) |

**実行の仕組み**: `command`をOSのシェル(Unix: `sh -c`、Windows: `cmd /C`)経由で起動し、その標準出力のみをパイプで`masker mask --stream`(+`--profile`/`--encoding`)へ直接接続する。標準エラー出力は対象外(マスクされず、呼び出し元にも返されない)。

**制限事項**:

- 出力は10MiBを超えると打ち切られる(末尾に`[masker-mcp: 出力が大きすぎるため打ち切りました]`を付与)
- タイムアウトに達すると対象コマンドとその子孫プロセスを含めて強制終了する(末尾に`[masker-mcp: タイムアウトにより打ち切りました]`を付与)
- `masker`側が異常終了した場合(プロファイル不在等)、標準エラー出力の内容を含むエラー結果を返す

## セットアップ

MCPクライアントの設定に`masker-mcp`実行ファイルの絶対パスを登録する。`masker`実行ファイル自体は、`masker-mcp`と同じディレクトリに置くか、PATHに追加しておく必要がある(検索は`masker-mcp`自身と同じディレクトリ→PATHの順。GUIインストーラー同梱の場合は同じディレクトリに配置されるため追加の設定は不要。個別ダウンロードの場合は[README.mdのPATH追加手順](../../README.md#cliをターミナルから呼べるようにするpathへの追加)を参照)。

設定例(多くのMCPクライアントに共通する`mcpServers`形式):

```json
{
  "mcpServers": {
    "masker-mcp": {
      "command": "/absolute/path/to/masker-mcp"
    }
  }
}
```

Claude Codeの場合はCLIから登録できる。

```bash
claude mcp add masker-mcp -- /absolute/path/to/masker-mcp
```

## 制約

プロファイル・ルールの作成/編集/削除はできない(CLI/GUIから行う)。`masking-core`/`profile-store`に依存しない設計のため、これらの操作を追加するにはCLIのサブコマンド(`masker profile ...`)を新たにサブプロセスとして呼び出す形になる。
