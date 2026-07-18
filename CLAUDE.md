# CLAUDE.md

このファイルはClaude Codeがこのリポジトリで作業する際に従うべき方針をまとめたものです。

## プロジェクト概要

**アプリ名: SensitiveMasker**

SIP/FreeSWITCHログに限らず、電話番号・パスワード・IPアドレス等の機微情報を含む任意のテキスト
(ログ・コンソール出力等)を、Claudeなど外部に貼り付ける前にローカルで自動マスキングするための
Windowsデスクトップツール。GUI(tkinter)とCLIの両方を提供し、コアのマスキングロジックは両者から共有される。

**重要**: このツール自体が「機微情報を外部に漏らさないため」に作られている。実装・テストの過程で
本物のログや実データを一切使わないこと(後述の「テストデータポリシー」を厳守)。

## 技術スタック

- Python 3.12+
- **uv**: パッケージ管理・実行(`uv sync`, `uv run ...`)。venvの手動管理はしない
- **pydantic**: ルール定義(`Rule`, `RuleProfile`)のスキーマ定義とバリデーション
- **pytest**: テスト実行
- **tkinter**: GUI(標準ライブラリ、追加依存なし)
- **argparse**: CLI
- **pywinauto**: GUIの黒箱スモークテスト(dev依存, Windows専用。詳細は「開発手法」の`gui`項目を参照)

## ディレクトリ構成

```
sensitive_masker/
  pyproject.toml
  CLAUDE.md
  README.md
  src/
    masking_core/
      __init__.py
      models.py        # pydanticモデル(Rule, RuleProfile)
      matcher.py        # literal/regexマッチング(純粋関数)
      masker.py          # ルール適用パイプライン, MappingStore
      profile_io.py       # プロファイルJSONの読み込み/保存
    cli/
      __init__.py
      main.py            # argparseエントリポイント
    gui/
      __init__.py
      app.py              # tkinterエントリポイント(ウィジェット/ダイアログ)
      settings.py         # 表示ラベル・テンプレート定義(純粋データ、tkinter非依存)
  tests/
    test_models.py
    test_matcher.py
    test_masker.py
    test_profile_io.py
    test_cli.py
    test_gui_templates.py  # gui/settings.pyのテンプレートデータ検証(表示なしで実行可能)
    fixtures/
      synthetic_logs.py  # 合成ダミーログ(実ログ使用禁止)
  rules/
    .gitkeep               # ユーザーが保存/インポートするプロファイルの置き場(既定では空)
  packaging/
    run_cli.py / run_gui.py                       # PyInstallerエントリスクリプト
    SensitiveMasker.spec / SensitiveMaskerCLI.spec  # PyInstaller onefileビルド設定
  scripts/
    generate_icon.py       # アプリアイコン生成(devのみ、Pillow使用)
  assets/
    icon.ico                # 生成済みアプリアイコン
```

## アーキテクチャ原則(疎結合)

依存の方向は常に「外側 → masking_core」の一方向。`masking_core`はGUI/CLIの存在を一切知らない。

```mermaid
flowchart TD
    G[gui: tkinter<br>薄いUI層] --> CORE[masking_core<br>純粋関数のみ]
    C[cli: argparse<br>薄いI-O層] --> CORE
    CORE --> M[Rule / RuleProfile<br>pydanticモデル]
```

- `masking_core`は**Functional Core**: 副作用(ファイルI/O、クリップボード、標準入出力、GUI描画)を持たない
- ファイルの読み書きや標準入出力は`cli`/`gui`側(**Imperative Shell**)の責務
- 対応表(`MappingStore`: 元の値→ダミー値)は`masking_core`内で状態として保持せず、呼び出し側から渡す/受け取る形にする(グローバル状態を作らない)。これによりバッチ変換で複数ファイルにまたがって同じ`MappingStore`を使い回すか、ファイルごとにリセットするかを呼び出し側で自由に選べる
- `pydantic`モデル(`Rule`, `RuleProfile`)は`masking_core.models`にのみ定義し、GUI/CLIはこのモデルを介してのみデータをやり取りする(GUI/CLI側で独自にdictを組み立てて渡さない)
- GUIとCLIの間には依存関係を作らない(どちらかがもう一方を呼び出すことはしない)

## 開発手法

- **masking_core**: 通常のTDD(Red-Green-Refactor)。厳密に行う
  - 肯定テストだけでなく、**否定テスト**(マスクされてはいけない箇所が誤って消えていないか)を必ず対にする
  - ルール適用順序による多重マスクや、`pattern_type="literal"`での正規表現特殊文字の扱いなど、境界条件を重点的にテストする
- **cli**: I/O中心の結合テスト(標準入出力・ファイル入出力・複数ファイルのバッチ処理)
- **gui**: 2階層の自動テスト + 手動確認チェックリストを併用する
  - **インプロセスTk駆動テスト**(`tests/test_gui_app.py`, 通常の`uv run pytest`で実行): 実際の
    `SensitiveMaskerApp`(`tk.Tk`サブクラス)をプロセス内に生成し、`.insert()`/`event_generate()`/
    ボタンのコールバック直接呼び出し等でユーザー操作相当の入力を与え、`.get()`/`tag_ranges()`等で
    実ウィジェットの状態を検証する(tkinter自体はモックしない。実ウィジェットをマウス/キーボード
    ではなくコードで駆動するだけ)。`messagebox`/`simpledialog`/`filedialog`など実際にブロッキング
    ダイアログを開く呼び出しは必ず`unittest.mock.patch`で差し替え、テストスイートが止まらないよう
    にする。app.pyの回帰バグ(例: 末尾行パディングのゼロ幅タグ範囲バグ)は、まずこの階層で再現
    テストを書いてから修正する
  - **pywinauto黒箱スモークテスト**(`tests/test_gui_smoke_pywinauto.py`, `pywinauto`マーカー付き。
    `uv run pytest -m pywinauto`で明示的に実行。デフォルトの`uv run pytest`では除外される):
    ビルド/起動した実プロセスをWindows UI Automation経由でOS外部から操作する、Playwrightに最も近い
    黒箱テスト層。**重要な既知の制約**: tkinter/ttkウィジェットはWindowsのアクセシビリティ情報を
    ほとんど公開しないため、ボタン等をラベル文字列で検索することはできない(`win32`バックエンドは
    どの子ウィンドウにも`GetWindowText()`が空、`uia`バックエンドでも各ttkウィジェットは無名の
    `Pane`としてしか見えない)。そのためレイアウト上の相対位置での特定や、クリック後に開く実ネイ
    ティブダイアログ(`messagebox`等、これは通常のWin32ウィンドウなので文字列が読める)を介した
    間接検証に頼る。実運用に耐えるヘッドレスな実行環境(対話セッションのないCIランナー等)ではまだ
    動作未検証のため、当面は開発者が手元のWindowsデスクトップで明示的に実行する用途に限定する
  - 表示ラベルやテンプレート定義など`app.py`から切り出した純粋データ(`gui/settings.py`)は、
    tkinter非依存のため上記2階層とは別に通常のpytestで自動テストしてよい(`tests/test_gui_templates.py`)
  - 上記の自動テストではカバーしきれない領域(モーダルダイアログの`grab_set()`/`wait_window()`を
    伴う`RuleEditDialog`/`RuleListEditorDialog`/`TemplatePickerDialog`の詳細な操作フロー、実際の
    見た目のレイアウト崩れ等)は、引き続き機能実装のたびに手動確認チェックリストを実施する
    (チェックリストの内容は実装が進む都度、該当機能に合わせて提示する)
- **コミット粒度**: 固定ルールは設けず、実装の区切りごとにClaudeが適切な粒度を提案する

## テストデータポリシー(厳守)

- テストコード・fixtureに**実際のSIPログ、実際の電話番号、実際のIPアドレス等を一切含めない**
- テストデータは`tests/fixtures/synthetic_logs.py`に架空の合成データとしてのみ定義する
  (例: 電話番号は`0120XXXXXX`のような明らかにダミーとわかる値、IPは`203.0.113.0/24`等のドキュメント用予約アドレス帯を使う)
- 新しいテストを追加する際、実データっぽい値をそのままコピー&ペーストしていないか毎回確認する

## コマンド一覧

```bash
uv sync --extra dev              # 依存関係のインストール(dev依存含む)
uv run pytest                    # 全テスト実行(pywinauto黒箱テストは既定で除外される)
uv run pytest tests/test_masker.py -v  # 個別テスト実行
uv run pytest -m pywinauto -v    # pywinauto黒箱GUIスモークテストのみ実行(実デスクトップ上で)
uv run python -m cli.main --profile my_profile.json --input in.log --output out.log
uv run python -m gui.app         # GUI起動
```

## 実装時の注意点

- ルール(`Rule`)の`mode="fixed"`時は`fixed_value`必須、`mode="random"`時は`prefix`必須。
  この整合性チェックは`pydantic`の`model_validator`でモデル定義時点に持たせ、
  呼び出し側(GUI/CLI)でif分岐による二重チェックをしない
- ランダムモードのダミー値プレフィックスは、元ログ中に出現しないことが保証された文字列にする
  (例: `__MASK_PHONE_1__`のような衝突しにくい形式)
- バッチ変換時、`MappingStore`をファイル間で共有するかリセットするかは呼び出し側(cli/gui)の判断とし、
  `masking_core`側では強制しない

## リリース手順

GitHubリポジトリ: https://github.com/akutori/SensitiveMasker (public, デフォルトブランチ`main`)

`v*`タグをpushすると`.github/workflows/release.yml`が自動起動し、`uv sync --extra dev` →
`pytest` → GUI/CLI両exeをビルド(`packaging/*.spec`)→ GitHub Releaseに`SensitiveMasker.exe`/
`SensitiveMaskerCLI.exe`を添付、まで自動で行われる。手順は以下の通り:

```bash
# 1. pyproject.toml の version を更新してから uv.lock も追従させる
uv sync --extra dev

# 2. バージョン更新をコミット
git add pyproject.toml uv.lock
git commit -m "chore: バージョンを X.Y.Z に更新"
git push origin main

# 3. タグを作成してpush -> ここでReleaseビルドが走る
git tag -a vX.Y.Z -m "vX.Y.Z"
git push origin vX.Y.Z

# 4. 進捗確認(任意)
gh run list --limit 3
gh release view vX.Y.Z
```

- 既存タグへの再アタッチ(ビルドやり直し)は`workflow_dispatch`(Actionsタブから手動実行、
  `tag_name`にvX.Y.Zを入力)で可能
- ワークフロー定義は[QR-Barcode-GUI](../QR-Barcode-GUI)の`.github/workflows/release.yml`を
  参考にした構成
