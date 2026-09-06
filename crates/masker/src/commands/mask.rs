//! `masker mask`の実行ロジック。
//!
//! 入出力の指定方法によって4つの実行モードに分かれる(組み合わせ不可)。フラグの組み合わせが
//! 不正な場合は`resolve_mask_mode`がエラーを返し、実際のI/Oには一切触れない。
//!
//! 対象はログ・コンソール出力であり、必ずしもUTF-8とは限らない(例: 日本語Windows環境の
//! Shift-JIS出力)。`--encoding`を明示的に指定しない場合、既定はUTF-8前提のlossy変換のみ
//! (非UTF-8バイト列はU+FFFDに置き換えて処理を継続する。`--stream`は`tail -f`等の長時間
//! コマンド向けであり、1バイトのエンコーディング不整合で監視全体が停止するのは実害が大きい
//! ため)。`--encoding`を明示的に指定した場合のみ、`encoding_rs`でそのエンコーディングとして
//! 実際にデコードする(自動検出はしない。短い1行ごとの推測は誤判定のリスクがあり、値が
//! 「それらしいが実は誤った」文字列に化ける方が今のlossy方式より危険なため)。
//! デコードで置き換えが起きた場合、機微情報のパターンが本来のバイト列と一致しなくなり
//! マスクされない可能性があるため、`warn`(通常は標準エラー出力)に必ず警告を出す
//! (exit 0の「成功」に見えて実は一部がマスクされていない、という状態を無警告にしない)。

use std::fs;
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};

use encoding_rs::Encoding;
use masking_core::{apply_compiled_profile, CompiledProfile, MappingStore};
use profile_store::ProfileStore;

use crate::cli::MaskArgs;
use crate::error::CliError;

#[derive(Debug, PartialEq, Eq)]
enum MaskMode {
    OneShotStdio,
    File { input: PathBuf, output: PathBuf },
    Batch { input_dir: PathBuf, output_dir: PathBuf, reset_mapping_per_file: bool },
    Stream,
}

fn resolve_mask_mode(args: &MaskArgs) -> Result<MaskMode, CliError> {
    if args.stream {
        if args.batch || args.input.is_some() || args.output.is_some() || args.reset_mapping_per_file {
            return Err(CliError::InvalidMaskArgs(
                "--streamは--batch/--input/--output/--reset-mapping-per-fileと同時に指定できません".to_string(),
            ));
        }
        return Ok(MaskMode::Stream);
    }

    if args.batch {
        let (Some(input_dir), Some(output_dir)) = (args.input.clone(), args.output.clone()) else {
            return Err(CliError::InvalidMaskArgs(
                "--batchには--inputと--output(いずれもディレクトリ)の両方が必要です".to_string(),
            ));
        };
        return Ok(MaskMode::Batch {
            input_dir,
            output_dir,
            reset_mapping_per_file: args.reset_mapping_per_file,
        });
    }

    if args.reset_mapping_per_file {
        return Err(CliError::InvalidMaskArgs(
            "--reset-mapping-per-fileは--batchと同時に指定してください".to_string(),
        ));
    }

    match (args.input.clone(), args.output.clone()) {
        (Some(input), Some(output)) => Ok(MaskMode::File { input, output }),
        (None, None) => Ok(MaskMode::OneShotStdio),
        _ => Err(CliError::InvalidMaskArgs("--inputと--outputは両方指定してください".to_string())),
    }
}

/// `--encoding`のラベル文字列(WHATWG Encoding Standard準拠)をencoding_rsの`Encoding`に解決する。
fn resolve_encoding(label: &str) -> Result<&'static Encoding, CliError> {
    Encoding::for_label(label.as_bytes())
        .ok_or_else(|| CliError::InvalidMaskArgs(format!("未知のエンコーディング名です: {label}")))
}

fn mask_text(text: &str, compiled: &CompiledProfile, store: &mut MappingStore) -> String {
    apply_compiled_profile(text, compiled, store).0
}

/// `encoding`が指定されている場合のみそのエンコーディングとしてデコードする。指定が無い
/// 場合(既定)は、encoding_rsのBOM検出等を持ち込まず、これまでと同じUTF-8前提のlossy変換の
/// みを行う(`--encoding`を明示していないのに挙動が変わることを避けるため意図的に分けている)。
fn decode(bytes: &[u8], encoding: Option<&'static Encoding>) -> (String, bool) {
    match encoding {
        None => {
            let had_errors = std::str::from_utf8(bytes).is_err();
            (String::from_utf8_lossy(bytes).into_owned(), had_errors)
        }
        Some(encoding) => {
            let (text, had_errors) = encoding.decode_without_bom_handling(bytes);
            (text.into_owned(), had_errors)
        }
    }
}

fn read_decoded(path: &Path, encoding: Option<&'static Encoding>) -> Result<(String, bool), CliError> {
    let bytes = fs::read(path).map_err(|source| CliError::IoAt { path: path.to_path_buf(), source })?;
    Ok(decode(&bytes, encoding))
}

fn write_at(path: &Path, content: &str) -> Result<(), CliError> {
    fs::write(path, content).map_err(|source| CliError::IoAt { path: path.to_path_buf(), source })
}

/// `--input`と`--output`が(シンボリックリンク解決後に)同一の場所を指すかどうか。
/// どちらかが未存在の場合は`canonicalize`が失敗するが、その場合は「まだ存在しない別の場所」
/// であり同一ではあり得ないため`false`として扱う。
fn paths_refer_to_the_same_location(a: &Path, b: &Path) -> bool {
    matches!((fs::canonicalize(a), fs::canonicalize(b)), (Ok(a), Ok(b)) if a == b)
}

fn warn_decode_errors(warn: &mut impl Write, context: &str, encoding: Option<&'static Encoding>) -> Result<(), CliError> {
    let encoding_name = encoding.map(|e| e.name()).unwrap_or("UTF-8");
    writeln!(
        warn,
        "警告: {context}を{encoding_name}として解釈できないバイト列が含まれていたため、該当部分を\
         置き換えて処理しました(その部分に含まれるはずの機微情報がマスクされない可能性があります)"
    )?;
    Ok(())
}

fn run_one_shot(
    reader: &mut impl Read,
    writer: &mut impl Write,
    warn: &mut impl Write,
    encoding: Option<&'static Encoding>,
    compiled: &CompiledProfile,
    store: &mut MappingStore,
) -> Result<(), CliError> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    let (text, had_errors) = decode(&bytes, encoding);
    if had_errors {
        warn_decode_errors(warn, "入力", encoding)?;
    }
    let masked = mask_text(&text, compiled, store);
    writer.write_all(masked.as_bytes())?;
    Ok(())
}

fn run_stream(
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    warn: &mut impl Write,
    encoding: Option<&'static Encoding>,
    compiled: &CompiledProfile,
    store: &mut MappingStore,
) -> Result<(), CliError> {
    let mut buf = Vec::new();
    // 行ごとに警告すると、入力全体が対応外のエンコーディングの場合に警告だけで
    // 標準エラー出力が埋まってしまうため、最初の1回だけ通知する。
    let mut already_warned = false;
    loop {
        buf.clear();
        let bytes_read = reader.read_until(b'\n', &mut buf)?;
        if bytes_read == 0 {
            break;
        }
        while matches!(buf.last(), Some(b'\n' | b'\r')) {
            buf.pop();
        }
        let (line, had_errors) = decode(&buf, encoding);
        if had_errors && !already_warned {
            warn_decode_errors(warn, "入力(以降同種の行があっても再通知しません)", encoding)?;
            already_warned = true;
        }
        let masked = mask_text(&line, compiled, store);
        // tail -f等、終了しないコマンドをパイプする用途のため、行ごとに即座にflushする
        // (バッファリングされたままだと出力が溜まり続け、ライブ監視の用途を果たせない)。
        writeln!(writer, "{masked}")?;
        writer.flush()?;
    }
    Ok(())
}

fn run_file(
    input: &Path,
    output: &Path,
    warn: &mut impl Write,
    encoding: Option<&'static Encoding>,
    compiled: &CompiledProfile,
    store: &mut MappingStore,
) -> Result<(), CliError> {
    let (text, had_errors) = read_decoded(input, encoding)?;
    if had_errors {
        warn_decode_errors(warn, &input.display().to_string(), encoding)?;
    }
    let masked = mask_text(&text, compiled, store);
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|source| CliError::IoAt { path: parent.to_path_buf(), source })?;
    }
    write_at(output, &masked)
}

fn run_batch(
    input_dir: &Path,
    output_dir: &Path,
    reset_mapping_per_file: bool,
    warn: &mut impl Write,
    encoding: Option<&'static Encoding>,
    compiled: &CompiledProfile,
    store: &mut MappingStore,
) -> Result<(), CliError> {
    // 入力ディレクトリが読めることを先に確認する(出力ディレクトリの作成より前に行い、
    // 入力側が不正な場合に出力側だけが副作用として作られてしまうのを避ける)。
    let mut entries: Vec<fs::DirEntry> = fs::read_dir(input_dir)
        .map_err(|source| CliError::IoAt { path: input_dir.to_path_buf(), source })?
        .collect::<Result<_, _>>()
        .map_err(|source| CliError::IoAt { path: input_dir.to_path_buf(), source })?;
    // read_dirの列挙順はOS依存のため、reset_mapping_per_file時の連番が実行ごとに変わらないよう
    // ファイル名でソートしてから処理する。
    entries.sort_by_key(|e| e.file_name());

    // --inputと--outputに同一ディレクトリを指定すると、マスク後の内容で元データをその場で
    // 上書きしてしまい復元できなくなるため、書き込みを始める前に拒否する。
    if paths_refer_to_the_same_location(input_dir, output_dir) {
        return Err(CliError::InvalidMaskArgs(
            "--inputと--outputに同じディレクトリを指定することはできません(元データが上書きされます)"
                .to_string(),
        ));
    }

    fs::create_dir_all(output_dir).map_err(|source| CliError::IoAt { path: output_dir.to_path_buf(), source })?;

    let mut skipped = 0usize;
    for entry in entries {
        let file_type = entry.file_type().map_err(|source| CliError::IoAt { path: entry.path(), source })?;
        if !file_type.is_file() {
            // サブディレクトリ・(ファイルを指すものを含む)symlink等は対象外として黙って
            // 読み飛ばすのではなく、件数だけは必ず知らせる(結果がOkだけだと全件処理されたと
            // 誤解されるため)。
            skipped += 1;
            continue;
        }
        if reset_mapping_per_file {
            *store = MappingStore::new();
        }
        let (text, had_errors) = read_decoded(&entry.path(), encoding)?;
        if had_errors {
            warn_decode_errors(warn, &entry.path().display().to_string(), encoding)?;
        }
        let masked = mask_text(&text, compiled, store);
        write_at(&output_dir.join(entry.file_name()), &masked)?;
    }
    if skipped > 0 {
        writeln!(warn, "{skipped}件のサブディレクトリ/非ファイルをスキップしました")?;
    }
    Ok(())
}

pub(crate) fn run(args: &MaskArgs, store: &ProfileStore) -> Result<(), CliError> {
    let profile = match &args.profile {
        Some(name) => store.get_profile(name)?,
        None => store.active_profile()?.ok_or(CliError::NoActiveProfile)?,
    };
    let compiled = CompiledProfile::compile(&profile);
    let mut mapping_store = MappingStore::new();
    let mut warn = std::io::stderr();
    let encoding = args.encoding.as_deref().map(resolve_encoding).transpose()?;

    match resolve_mask_mode(args)? {
        MaskMode::OneShotStdio => run_one_shot(
            &mut std::io::stdin().lock(),
            &mut std::io::stdout().lock(),
            &mut warn,
            encoding,
            &compiled,
            &mut mapping_store,
        ),
        MaskMode::File { input, output } => {
            run_file(&input, &output, &mut warn, encoding, &compiled, &mut mapping_store)
        }
        MaskMode::Batch { input_dir, output_dir, reset_mapping_per_file } => run_batch(
            &input_dir,
            &output_dir,
            reset_mapping_per_file,
            &mut warn,
            encoding,
            &compiled,
            &mut mapping_store,
        ),
        MaskMode::Stream => run_stream(
            &mut std::io::stdin().lock(),
            &mut std::io::stdout().lock(),
            &mut warn,
            encoding,
            &compiled,
            &mut mapping_store,
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use masking_core::{Mode, PatternType, Rule, RuleProfile};

    use super::*;

    fn base_args() -> MaskArgs {
        MaskArgs {
            profile: None,
            input: None,
            output: None,
            batch: false,
            reset_mapping_per_file: false,
            stream: false,
            encoding: None,
        }
    }

    #[test]
    fn no_flags_resolves_to_one_shot_stdio() {
        assert_eq!(resolve_mask_mode(&base_args()).unwrap(), MaskMode::OneShotStdio);
    }

    #[test]
    fn input_and_output_resolve_to_file_mode() {
        let mut args = base_args();
        args.input = Some("in.txt".into());
        args.output = Some("out.txt".into());
        assert_eq!(
            resolve_mask_mode(&args).unwrap(),
            MaskMode::File { input: "in.txt".into(), output: "out.txt".into() }
        );
    }

    #[test]
    fn input_without_output_is_rejected() {
        let mut args = base_args();
        args.input = Some("in.txt".into());
        assert!(resolve_mask_mode(&args).is_err());
    }

    #[test]
    fn output_without_input_is_rejected() {
        let mut args = base_args();
        args.output = Some("out.txt".into());
        assert!(resolve_mask_mode(&args).is_err());
    }

    #[test]
    fn batch_with_both_dirs_resolves_to_batch_mode() {
        let mut args = base_args();
        args.batch = true;
        args.input = Some("in_dir".into());
        args.output = Some("out_dir".into());
        assert_eq!(
            resolve_mask_mode(&args).unwrap(),
            MaskMode::Batch {
                input_dir: "in_dir".into(),
                output_dir: "out_dir".into(),
                reset_mapping_per_file: false
            }
        );
    }

    #[test]
    fn batch_without_dirs_is_rejected() {
        let mut args = base_args();
        args.batch = true;
        assert!(resolve_mask_mode(&args).is_err());
    }

    #[test]
    fn reset_mapping_per_file_without_batch_is_rejected() {
        let mut args = base_args();
        args.reset_mapping_per_file = true;
        assert!(resolve_mask_mode(&args).is_err());
    }

    #[test]
    fn stream_combined_with_batch_is_rejected() {
        let mut args = base_args();
        args.stream = true;
        args.batch = true;
        assert!(resolve_mask_mode(&args).is_err());
    }

    #[test]
    fn stream_combined_with_input_is_rejected() {
        let mut args = base_args();
        args.stream = true;
        args.input = Some("in.txt".into());
        assert!(resolve_mask_mode(&args).is_err());
    }

    #[test]
    fn stream_combined_with_reset_mapping_per_file_is_rejected() {
        let mut args = base_args();
        args.stream = true;
        args.reset_mapping_per_file = true;
        assert!(resolve_mask_mode(&args).is_err());
    }

    #[test]
    fn stream_alone_resolves_to_stream_mode() {
        let mut args = base_args();
        args.stream = true;
        assert_eq!(resolve_mask_mode(&args).unwrap(), MaskMode::Stream);
    }

    #[test]
    fn resolve_encoding_accepts_a_known_whatwg_label() {
        let encoding = resolve_encoding("shift-jis").unwrap();
        assert_eq!(encoding.name(), "Shift_JIS");
    }

    #[test]
    fn resolve_encoding_rejects_an_unknown_label() {
        assert!(resolve_encoding("not-a-real-encoding").is_err());
    }

    fn sample_compiled_profile_and_store() -> (RuleProfile, MappingStore) {
        let rule = Rule::new(
            "ip",
            PatternType::Regex,
            r"\d+\.\d+\.\d+\.\d+",
            Mode::Sequential,
            None,
            Some("__MASK_IP_".to_string()),
            true,
            None,
        )
        .unwrap();
        (RuleProfile::new("p", None, vec![rule]).unwrap(), MappingStore::new())
    }

    #[test]
    fn run_one_shot_masks_the_entire_buffered_input() {
        let (profile, mut store) = sample_compiled_profile_and_store();
        let compiled = CompiledProfile::compile(&profile);
        let mut reader = Cursor::new(b"connect to 10.0.0.1 now".to_vec());
        let mut writer = Vec::new();
        let mut warn = Vec::new();

        run_one_shot(&mut reader, &mut writer, &mut warn, None, &compiled, &mut store).unwrap();

        assert_eq!(String::from_utf8(writer).unwrap(), "connect to __MASK_IP_1__ now");
        assert!(warn.is_empty(), "有効なUTF-8のみの入力では警告を出さないはず");
    }

    #[test]
    fn run_one_shot_replaces_invalid_utf8_bytes_and_warns_once() {
        let (profile, mut store) = sample_compiled_profile_and_store();
        let compiled = CompiledProfile::compile(&profile);
        // 0x81 0x40は有効なUTF-8として解釈できないShift-JIS由来のバイト列の例。
        let mut input = b"before 10.0.0.1 ".to_vec();
        input.extend_from_slice(&[0x81, 0x40]);
        input.extend_from_slice(b" after");
        let mut reader = Cursor::new(input);
        let mut writer = Vec::new();
        let mut warn = Vec::new();

        let result = run_one_shot(&mut reader, &mut writer, &mut warn, None, &compiled, &mut store);

        assert!(result.is_ok(), "非UTF-8バイト列があってもエラーにならないはず: {result:?}");
        let output = String::from_utf8(writer).unwrap();
        assert!(output.contains("__MASK_IP_1__"), "有効な部分は通常通りマスクされるはず: {output}");
        assert!(!warn.is_empty(), "非UTF-8バイト列があった場合は警告を出すはず");
    }

    #[test]
    fn run_one_shot_does_not_mask_a_value_whose_bytes_are_corrupted_by_invalid_utf8() {
        // 既知の制約: --encoding未指定時、機微情報の値の内部に非UTF-8バイトが落ちると、
        // 置き換え後の文字列は元のパターンにマッチしなくなり、マスクされない。警告は出るため
        // 無警告ではないが、マスク漏れそのものは解消されない(docs/cli/README.mdに明記)。
        let (profile, mut store) = sample_compiled_profile_and_store();
        let compiled = CompiledProfile::compile(&profile);
        let mut input = b"call 090-12".to_vec();
        input.extend_from_slice(&[0x81, 0x40]);
        input.extend_from_slice(b"34-5678 now");
        let mut reader = Cursor::new(input);
        let mut writer = Vec::new();
        let mut warn = Vec::new();

        run_one_shot(&mut reader, &mut writer, &mut warn, None, &compiled, &mut store).unwrap();

        assert!(!warn.is_empty(), "この既知の制約が起きたケースでは必ず警告が出るはず");
    }

    /// 全角数字"０９０-１２３４-５６７８"のShift-JIS(CP932)バイト列。
    /// 実機の`[System.Text.Encoding]::GetEncoding(932)`で実際にエンコードして確認済み。
    fn shift_jis_zenkaku_phone_bytes() -> Vec<u8> {
        vec![
            0x82, 0x4F, 0x82, 0x58, 0x82, 0x4F, 0x2D, 0x82, 0x50, 0x82, 0x51, 0x82, 0x52, 0x82, 0x53, 0x2D, 0x82,
            0x54, 0x82, 0x55, 0x82, 0x56, 0x82, 0x57,
        ]
    }

    fn zenkaku_phone_profile_and_store() -> (RuleProfile, MappingStore) {
        let rule = Rule::new(
            "zenkaku_phone",
            PatternType::Literal,
            "０９０-１２３４-５６７８",
            Mode::Fixed,
            Some("MASKED".to_string()),
            None,
            true,
            None,
        )
        .unwrap();
        (RuleProfile::new("p", None, vec![rule]).unwrap(), MappingStore::new())
    }

    #[test]
    fn run_one_shot_correctly_decodes_and_masks_shift_jis_when_encoding_is_specified() {
        let (profile, mut store) = zenkaku_phone_profile_and_store();
        let compiled = CompiledProfile::compile(&profile);
        let encoding = resolve_encoding("shift-jis").unwrap();
        let mut reader = Cursor::new(shift_jis_zenkaku_phone_bytes());
        let mut writer = Vec::new();
        let mut warn = Vec::new();

        run_one_shot(&mut reader, &mut writer, &mut warn, Some(encoding), &compiled, &mut store).unwrap();

        assert_eq!(String::from_utf8(writer).unwrap(), "MASKED");
        assert!(warn.is_empty(), "正しくデコードできた場合は警告を出さないはず");
    }

    #[test]
    fn without_encoding_flag_shift_jis_bytes_are_not_correctly_masked() {
        // --encoding未指定の既定動作(UTF-8前提のlossy変換)では、同じShift-JISバイト列は
        // 正しく解釈できずマスクされないことを確認する(--encoding指定の効果を裏付ける対照)。
        let (profile, mut store) = zenkaku_phone_profile_and_store();
        let compiled = CompiledProfile::compile(&profile);
        let mut reader = Cursor::new(shift_jis_zenkaku_phone_bytes());
        let mut writer = Vec::new();
        let mut warn = Vec::new();

        run_one_shot(&mut reader, &mut writer, &mut warn, None, &compiled, &mut store).unwrap();

        let output = String::from_utf8(writer).unwrap();
        assert_ne!(output, "MASKED", "非対応のエンコーディングでは元のルールと一致せずマスクされないはず");
        assert!(!warn.is_empty(), "非UTF-8バイト列があった場合は警告は出るはず");
    }

    #[test]
    fn run_file_correctly_decodes_shift_jis_from_a_real_file_when_encoding_is_specified() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("in.log");
        let output = dir.path().join("out.log");
        fs::write(&input, shift_jis_zenkaku_phone_bytes()).unwrap();
        let (profile, mut store) = zenkaku_phone_profile_and_store();
        let compiled = CompiledProfile::compile(&profile);
        let encoding = resolve_encoding("shift-jis").unwrap();
        let mut warn = Vec::new();

        run_file(&input, &output, &mut warn, Some(encoding), &compiled, &mut store).unwrap();

        assert_eq!(fs::read_to_string(&output).unwrap(), "MASKED");
        assert!(warn.is_empty());
    }

    #[test]
    fn run_stream_masks_each_line_and_shares_mapping_across_lines() {
        let (profile, mut store) = sample_compiled_profile_and_store();
        let compiled = CompiledProfile::compile(&profile);
        let mut reader = Cursor::new(b"first 10.0.0.1\nsecond 10.0.0.1 and 192.168.0.1\n".to_vec());
        let mut writer = Vec::new();
        let mut warn = Vec::new();

        run_stream(&mut reader, &mut writer, &mut warn, None, &compiled, &mut store).unwrap();

        let output = String::from_utf8(writer).unwrap();
        assert_eq!(output, "first __MASK_IP_1__\nsecond __MASK_IP_1__ and __MASK_IP_2__\n");
        assert!(warn.is_empty());
    }

    #[test]
    fn run_stream_handles_input_with_no_trailing_newline() {
        let (profile, mut store) = sample_compiled_profile_and_store();
        let compiled = CompiledProfile::compile(&profile);
        let mut reader = Cursor::new(b"10.0.0.1".to_vec());
        let mut writer = Vec::new();
        let mut warn = Vec::new();

        run_stream(&mut reader, &mut writer, &mut warn, None, &compiled, &mut store).unwrap();

        assert_eq!(String::from_utf8(writer).unwrap(), "__MASK_IP_1__\n");
    }

    #[test]
    fn run_stream_does_not_abort_on_a_single_invalid_utf8_line() {
        let (profile, mut store) = sample_compiled_profile_and_store();
        let compiled = CompiledProfile::compile(&profile);
        let mut input = b"good line 10.0.0.1\n".to_vec();
        input.extend_from_slice(&[0x81, 0x40]);
        input.extend_from_slice(b"\nafter 192.168.0.1\n");
        let mut reader = Cursor::new(input);
        let mut writer = Vec::new();
        let mut warn = Vec::new();

        let result = run_stream(&mut reader, &mut writer, &mut warn, None, &compiled, &mut store);

        assert!(result.is_ok(), "1行だけ非UTF-8でも継続するはず: {result:?}");
        let output = String::from_utf8(writer).unwrap();
        assert!(output.contains("good line __MASK_IP_1__"));
        assert!(output.contains("after __MASK_IP_2__"), "壊れた行の後も継続して処理されるはず: {output}");
        assert!(!warn.is_empty(), "非UTF-8の行があった場合は警告を出すはず");
    }

    #[test]
    fn run_stream_warns_only_once_even_with_multiple_invalid_utf8_lines() {
        let (profile, mut store) = sample_compiled_profile_and_store();
        let compiled = CompiledProfile::compile(&profile);
        let mut input = Vec::new();
        input.extend_from_slice(&[0x81, 0x40]);
        input.push(b'\n');
        input.extend_from_slice(&[0x81, 0x41]);
        input.push(b'\n');
        let mut reader = Cursor::new(input);
        let mut writer = Vec::new();
        let mut warn = Vec::new();

        run_stream(&mut reader, &mut writer, &mut warn, None, &compiled, &mut store).unwrap();

        let warn_text = String::from_utf8(warn).unwrap();
        assert_eq!(warn_text.lines().count(), 1, "複数行が非UTF-8でも警告は1回だけのはず: {warn_text}");
    }

    #[test]
    fn run_file_reads_and_writes_real_files() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("in.log");
        let output = dir.path().join("out.log");
        fs::write(&input, "10.0.0.1").unwrap();
        let (profile, mut store) = sample_compiled_profile_and_store();
        let compiled = CompiledProfile::compile(&profile);
        let mut warn = Vec::new();

        run_file(&input, &output, &mut warn, None, &compiled, &mut store).unwrap();

        assert_eq!(fs::read_to_string(&output).unwrap(), "__MASK_IP_1__");
        assert!(warn.is_empty());
    }

    #[test]
    fn run_file_creates_missing_parent_directory_for_output() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("in.log");
        let output = dir.path().join("nested").join("deep").join("out.log");
        fs::write(&input, "10.0.0.1").unwrap();
        let (profile, mut store) = sample_compiled_profile_and_store();
        let compiled = CompiledProfile::compile(&profile);
        let mut warn = Vec::new();

        run_file(&input, &output, &mut warn, None, &compiled, &mut store).unwrap();

        assert_eq!(fs::read_to_string(&output).unwrap(), "__MASK_IP_1__");
    }

    #[test]
    fn run_file_replaces_invalid_utf8_bytes_and_warns() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("in.log");
        let output = dir.path().join("out.log");
        let mut bytes = b"before 10.0.0.1 ".to_vec();
        bytes.extend_from_slice(&[0x81, 0x40]);
        fs::write(&input, &bytes).unwrap();
        let (profile, mut store) = sample_compiled_profile_and_store();
        let compiled = CompiledProfile::compile(&profile);
        let mut warn = Vec::new();

        run_file(&input, &output, &mut warn, None, &compiled, &mut store).unwrap();

        assert!(fs::read_to_string(&output).unwrap().contains("__MASK_IP_1__"));
        assert!(!warn.is_empty(), "非UTF-8バイト列を含むファイルは警告を出すはず");
        let warn_text = String::from_utf8(warn).unwrap();
        assert!(warn_text.contains(&input.display().to_string()), "警告にファイルパスが含まれるはず: {warn_text}");
    }

    #[test]
    fn run_batch_shares_mapping_across_files_by_default() {
        let in_dir = tempfile::tempdir().unwrap();
        let out_dir = tempfile::tempdir().unwrap();
        fs::write(in_dir.path().join("a.log"), "10.0.0.1").unwrap();
        fs::write(in_dir.path().join("b.log"), "10.0.0.1 and 192.168.0.1").unwrap();
        let (profile, mut store) = sample_compiled_profile_and_store();
        let compiled = CompiledProfile::compile(&profile);
        let mut warn = Vec::new();

        run_batch(in_dir.path(), out_dir.path(), false, &mut warn, None, &compiled, &mut store).unwrap();

        assert_eq!(fs::read_to_string(out_dir.path().join("a.log")).unwrap(), "__MASK_IP_1__");
        assert_eq!(
            fs::read_to_string(out_dir.path().join("b.log")).unwrap(),
            "__MASK_IP_1__ and __MASK_IP_2__"
        );
        assert!(warn.is_empty());
    }

    #[test]
    fn run_batch_resets_mapping_per_file_when_requested() {
        let in_dir = tempfile::tempdir().unwrap();
        let out_dir = tempfile::tempdir().unwrap();
        fs::write(in_dir.path().join("a.log"), "10.0.0.1").unwrap();
        fs::write(in_dir.path().join("b.log"), "10.0.0.1").unwrap();
        let (profile, mut store) = sample_compiled_profile_and_store();
        let compiled = CompiledProfile::compile(&profile);
        let mut warn = Vec::new();

        run_batch(in_dir.path(), out_dir.path(), true, &mut warn, None, &compiled, &mut store).unwrap();

        assert_eq!(fs::read_to_string(out_dir.path().join("a.log")).unwrap(), "__MASK_IP_1__");
        assert_eq!(fs::read_to_string(out_dir.path().join("b.log")).unwrap(), "__MASK_IP_1__");
    }

    #[test]
    fn run_batch_rejects_when_input_and_output_are_the_same_directory() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.log"), "original 10.0.0.1 data").unwrap();
        let (profile, mut store) = sample_compiled_profile_and_store();
        let compiled = CompiledProfile::compile(&profile);
        let mut warn = Vec::new();

        let err = run_batch(dir.path(), dir.path(), false, &mut warn, None, &compiled, &mut store)
            .expect_err("inputとoutputが同じディレクトリの場合は拒否されるはず");

        assert!(matches!(err, CliError::InvalidMaskArgs(_)));
        // 拒否された場合、元データが書き換えられていないことを確認する。
        assert_eq!(fs::read_to_string(dir.path().join("a.log")).unwrap(), "original 10.0.0.1 data");
    }

    #[test]
    fn run_batch_does_not_create_output_dir_when_input_dir_is_missing() {
        let base = tempfile::tempdir().unwrap();
        let input_dir = base.path().join("no_such_input");
        let output_dir = base.path().join("would_be_output");
        let (profile, mut store) = sample_compiled_profile_and_store();
        let compiled = CompiledProfile::compile(&profile);
        let mut warn = Vec::new();

        let err = run_batch(&input_dir, &output_dir, false, &mut warn, None, &compiled, &mut store)
            .expect_err("存在しない入力ディレクトリは失敗するはず");

        assert!(matches!(err, CliError::IoAt { .. }));
        assert!(!output_dir.exists(), "入力側の検証に失敗した時点で出力ディレクトリが作られてはいけない");
    }

    #[test]
    fn run_batch_skips_subdirectories_and_reports_the_count() {
        let in_dir = tempfile::tempdir().unwrap();
        let out_dir = tempfile::tempdir().unwrap();
        fs::write(in_dir.path().join("a.log"), "10.0.0.1").unwrap();
        fs::create_dir(in_dir.path().join("subdir")).unwrap();
        fs::write(in_dir.path().join("subdir").join("nested.log"), "192.168.0.1").unwrap();
        let (profile, mut store) = sample_compiled_profile_and_store();
        let compiled = CompiledProfile::compile(&profile);
        let mut warn = Vec::new();

        run_batch(in_dir.path(), out_dir.path(), false, &mut warn, None, &compiled, &mut store).unwrap();

        assert_eq!(fs::read_to_string(out_dir.path().join("a.log")).unwrap(), "__MASK_IP_1__");
        assert!(!out_dir.path().join("subdir").exists(), "サブディレクトリは対象外のはず");
        let warn_text = String::from_utf8(warn).unwrap();
        assert!(warn_text.contains('1'), "スキップ件数(1件)が警告に含まれるはず: {warn_text}");
    }

    #[test]
    fn run_batch_replaces_invalid_utf8_bytes_per_file_and_warns_with_file_path() {
        let in_dir = tempfile::tempdir().unwrap();
        let out_dir = tempfile::tempdir().unwrap();
        fs::write(in_dir.path().join("a.log"), "10.0.0.1").unwrap();
        let mut bad_bytes = b"192.168.0.1 ".to_vec();
        bad_bytes.extend_from_slice(&[0x81, 0x40]);
        fs::write(in_dir.path().join("b.log"), &bad_bytes).unwrap();
        let (profile, mut store) = sample_compiled_profile_and_store();
        let compiled = CompiledProfile::compile(&profile);
        let mut warn = Vec::new();

        run_batch(in_dir.path(), out_dir.path(), false, &mut warn, None, &compiled, &mut store).unwrap();

        assert_eq!(fs::read_to_string(out_dir.path().join("a.log")).unwrap(), "__MASK_IP_1__");
        let warn_text = String::from_utf8(warn).unwrap();
        assert!(warn_text.contains("b.log"), "警告にどのファイルが原因か含まれるはず: {warn_text}");
        assert!(!warn_text.contains("a.log"), "問題のなかったファイルは警告に含まれないはず: {warn_text}");
    }
}
