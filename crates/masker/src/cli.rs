//! コマンドライン引数定義(clap)。

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "masker", about = "機微情報をローカルでマスキングするCLI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// 鍵ファイル+DBを初期化する
    Init,
    /// テキストをマスキングする
    Mask(MaskArgs),
    /// プロファイルを管理する
    Profile {
        #[command(subcommand)]
        action: ProfileAction,
    },
}

#[derive(Args, Debug)]
pub struct MaskArgs {
    /// 使用するプロファイル名(省略時はアクティブプロファイル)
    #[arg(long)]
    pub profile: Option<String>,
    /// 入力ファイル(--batch指定時は入力ディレクトリ)
    #[arg(long)]
    pub input: Option<PathBuf>,
    /// 出力ファイル(--batch指定時は出力ディレクトリ)
    #[arg(long)]
    pub output: Option<PathBuf>,
    /// --input/--outputをディレクトリとして扱い、内部の全ファイルを処理する
    #[arg(long)]
    pub batch: bool,
    /// --batch時、ファイルごとにMappingStore(値の対応表)をリセットする(既定は全ファイルで共有)
    #[arg(long)]
    pub reset_mapping_per_file: bool,
    /// stdinを1行ずつ読み、都度マスクして即座にstdoutへ書き出す(tail -f等の長時間コマンド向け)
    #[arg(long)]
    pub stream: bool,
    /// 入力のエンコーディング(WHATWG Encoding Standardのラベル名、例: "shift-jis")。
    /// 省略時はUTF-8として扱う(非UTF-8バイト列はU+FFFDに置き換える)
    #[arg(long)]
    pub encoding: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum ProfileAction {
    /// プロファイル一覧を表示する
    List,
    /// アクティブプロファイルを切り替える
    Use {
        name: String,
    },
    /// 新規プロファイルを作成する
    Create {
        name: String,
        /// ルール定義(Rule配列と同じJSON形式)を読み込むファイル。省略時はルール0件で作成する
        #[arg(long)]
        from_json: Option<PathBuf>,
    },
    /// プロファイルを削除する(アクティブなプロファイルは拒否される)
    Delete {
        name: String,
    },
    /// プロファイルをパスフレーズで暗号化してファイルへ書き出す
    /// (パスフレーズは実行時にプロンプトで入力する。コマンドライン引数では渡せない)
    Export {
        name: String,
        #[arg(long)]
        output: PathBuf,
    },
    /// エクスポートされたファイルからプロファイルを取り込む
    /// (パスフレーズは実行時にプロンプトで入力する。コマンドライン引数では渡せない)
    Import {
        #[arg(long)]
        input: PathBuf,
    },
}
