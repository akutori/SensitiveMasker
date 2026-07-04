# SensitiveMasker

SIP/FreeSWITCHログに限らず、電話番号・パスワード・IPアドレス等の機微情報を含む任意のテキストを、
外部LLMに貼り付ける前にローカルで自動マスキングするWindowsデスクトップツールです。
GUI(tkinter)とCLIの両方を提供し、コアのマスキングロジックは両者から共有されます。

アーキテクチャや開発方針の詳細は [CLAUDE.md](CLAUDE.md) を参照してください。

## ダウンロード(配布版)

Python環境なしで使いたい場合は、[GitHub Releases](https://github.com/akutori/SensitiveMasker/releases/latest)
から`SensitiveMasker.exe`(GUI)または`SensitiveMaskerCLI.exe`(CLI)をダウンロードしてください。
どちらも単独の実行ファイルで、ダブルクリック/コマンド実行だけで動作します。

## セットアップ

```powershell
uv sync --extra dev
```

## テスト実行

```powershell
uv run pytest
uv run pytest tests/test_masker.py -v   # 個別テスト実行
```

GUI(`src/gui/app.py`)は自動テストを持ちません。実装計画中の手動確認チェックリストに沿って
`uv run python -m gui.app` で起動し、都度手動で確認してください。

## CLIの使い方

`--profile`にはルールプロファイルのJSONファイルを指定します。プロファイルはGUIの
「新規作成」「テンプレートから作成」→「保存」で作成するか、`masking_core.models.RuleProfile`
のスキーマに沿って手書きしてください(フィールドは[CLAUDE.md](CLAUDE.md)参照)。

```powershell
# ファイル指定
uv run python -m cli.main --profile my_profile.json --input in.log --output out.log

# stdin/stdout
Get-Content in.log | uv run python -m cli.main --profile my_profile.json > out.log

# 複数ファイルのバッチ処理(デフォルトはMappingStoreを全ファイルで共有)
uv run python -m cli.main --profile my_profile.json --batch a.log b.log --output-dir masked/

# ファイルごとにMappingStoreをリセットしたい場合
uv run python -m cli.main --profile my_profile.json --batch a.log b.log --output-dir masked/ --reset-mapping-per-file
```

## GUIの起動

```powershell
uv run python -m gui.app
```

## PyInstallerによる単独exe化

GUI・CLIそれぞれ独立した`--onefile`実行ファイルとしてビルドできます。
アイコン(`assets/icon.ico`)はPillow(devのみ)でビルド時に生成済みで、両方のビルドに埋め込まれます。

```powershell
uv sync --extra dev
uv run python scripts/generate_icon.py   # アイコンを再生成したい場合のみ
uv run pyinstaller packaging/SensitiveMaskerCLI.spec --distpath dist --workpath build --noconfirm
uv run pyinstaller packaging/SensitiveMasker.spec --distpath dist --workpath build --noconfirm
```

生成物: `dist/SensitiveMaskerCLI.exe`(コンソールあり)、`dist/SensitiveMasker.exe`(ウィンドウのみ)。
どちらもPythonやuvのインストールなしに単独で動作します。

## ディレクトリ構成

```
src/
  masking_core/   # 副作用のないマスキングロジック(Functional Core)
  cli/            # argparse CLI(Imperative Shell)
  gui/            # tkinter GUI(Imperative Shell)
tests/            # pytest(masking_core/cliの自動テスト)
rules/            # 作成したルールプロファイルの置き場(既定では空、.gitkeepのみ)
packaging/        # PyInstaller用エントリスクリプトと.specファイル
scripts/          # 開発用スクリプト(アイコン生成など)
assets/           # アプリアイコン等
```
