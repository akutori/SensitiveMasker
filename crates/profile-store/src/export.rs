//! プロファイルのエクスポート/インポート用の再暗号化(ageクレート、パスフレーズベース)。
//!
//! DBの鍵ファイル(key.rs)は端末ローカルの鍵であり、別マシンへ持ち出す用途には使えない。
//! そのためエクスポート/インポートはローカル鍵を経由せず、ユーザーが指定したパスフレーズのみで
//! 復号できる形(age -p相当)に再暗号化する。

use age::scrypt::{Identity, Recipient};
use secrecy::{ExposeSecret, SecretString};

// ageのscrypt::Identity::set_max_work_factorの上限。ageクレート自身のドキュメントが
// 「22を超えると悪意あるファイルの復号試行に数時間・数十GiBのRAMを要する場合がある」と
// 明記している境界値であり、これより上げない。既定(このメソッドを呼ばない場合)は
// このマシンの約1秒キャリブレーション値+4段階のみが許容されるため、エクスポート元マシンが
// 大幅に高速な場合に正しいパスフレーズでも復号を拒否されてしまう。22まで許容することで
// その誤検知を減らしつつ、DoS的な悪用コストは低いまま保つ。
const MAX_WORK_FACTOR_LOG_N: u8 = 22;

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("パスフレーズが空です")]
    EmptyPassphrase,
    #[error("エクスポート用の暗号化に失敗しました")]
    EncryptionFailed,
    #[error("復号に失敗しました(パスフレーズが誤っているか、データが破損しています)")]
    DecryptionFailed,
    #[error(
        "このファイルはより高性能な環境でエクスポートされたため、このマシンでは復号できません\
         (必要なワークファクタ: 2^{required}、このマシンでの許容上限: 2^{allowed_max})"
    )]
    ExcessiveWork { required: u8, allowed_max: u8 },
}

pub fn encrypt_for_export(plaintext: &[u8], passphrase: SecretString) -> Result<Vec<u8>, ExportError> {
    if passphrase.expose_secret().is_empty() {
        return Err(ExportError::EmptyPassphrase);
    }
    let recipient = Recipient::new(passphrase);
    age::encrypt(&recipient, plaintext).map_err(|_| ExportError::EncryptionFailed)
}

/// 復号の結果。`passphrase_trimmed`は、入力のままでは復号できず、前後の空白・不可視文字を除いたパスフレーズで
/// 復号できたことを表す(貼り付けで混ざった文字を、利用者へ知らせるため)。
pub struct DecryptedImport {
    pub plaintext: Vec<u8>,
    pub passphrase_trimmed: bool,
}

pub fn decrypt_import(ciphertext: &[u8], passphrase: SecretString) -> Result<Vec<u8>, ExportError> {
    decrypt_with_whitespace_fallback(ciphertext, passphrase, MAX_WORK_FACTOR_LOG_N)
}

/// `decrypt_import`と同じ復号で、空白を除いて再試行して成功したかも返す。
pub fn decrypt_import_reporting_trim(
    ciphertext: &[u8],
    passphrase: SecretString,
) -> Result<DecryptedImport, ExportError> {
    decrypt_with_whitespace_fallback_reporting(ciphertext, passphrase, MAX_WORK_FACTOR_LOG_N)
        .map(|(plaintext, passphrase_trimmed)| DecryptedImport { plaintext, passphrase_trimmed })
}

// 貼り付けで混入しうる、パスフレーズの前後の空白と不可視文字(ゼロ幅スペース・BOM等)。
// str::trimが除くのはUnicodeのWhite_Spaceだけで、これらの不可視文字は含まれない。
fn is_paste_noise(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{2060}' | '\u{FEFF}')
}

// 入力のままの復号に失敗した後の、再試行用の候補。前後の空白・不可視文字を除いた結果が
// 入力と異なり、かつ空でない場合だけ返す。除くものが無いなら、同じ値で再試行しても無駄に
// scryptを走らせるだけであり、空になるなら空のパスフレーズを試すことになるため。
// 入力の部分文字列としてだけ返すので、パスフレーズ内部の空白は変わらない。
fn retry_candidate(typed: &str) -> Option<&str> {
    let candidate = typed.trim_matches(is_paste_noise);
    (!candidate.is_empty() && candidate.len() != typed.len()).then_some(candidate)
}

// 貼り付けやTTY入力で前後に空白・不可視文字が混ざっていても復号できるよう、入力のまま
// 復号に失敗したときだけ、それらを除いたパスフレーズで1回だけ再試行する。入力のまま成功する
// 場合はそのまま復号する(前後に空白を含むパスフレーズで作ったファイルも復号できる。
// 空白を「足す」方向は試さない)。ワークファクタ超過はパスフレーズの正誤と無関係なので、
// 再試行しない。
fn decrypt_with_whitespace_fallback(
    ciphertext: &[u8],
    passphrase: SecretString,
    max_log_n: u8,
) -> Result<Vec<u8>, ExportError> {
    decrypt_with_whitespace_fallback_reporting(ciphertext, passphrase, max_log_n).map(|(plaintext, _)| plaintext)
}

// `decrypt_with_whitespace_fallback`の本体。復号した内容と、空白を除いて再試行して成功したか(true)を返す。
fn decrypt_with_whitespace_fallback_reporting(
    ciphertext: &[u8],
    passphrase: SecretString,
    max_log_n: u8,
) -> Result<(Vec<u8>, bool), ExportError> {
    // 最初の試行がパスフレーズを消費するため、再試行用の値は先に作る。再試行の候補が無い
    // (通常の)場合は作らず、機微な値のコピーを増やさない。
    let fallback = retry_candidate(passphrase.expose_secret())
        .map(|candidate| SecretString::from(candidate.to_owned()));

    match (decrypt_import_with_max_work_factor(ciphertext, passphrase, max_log_n), fallback) {
        (Err(ExportError::DecryptionFailed), Some(fallback_passphrase)) => {
            decrypt_import_with_max_work_factor(ciphertext, fallback_passphrase, max_log_n)
                .map(|plaintext| (plaintext, true))
        }
        (result, _) => result.map(|plaintext| (plaintext, false)),
    }
}

// 上限値を注入可能にして、実際に高コストなscrypt計算を発生させずにテストできるようにする
// (2^22相当の暗号文を本当に生成するテストは、それ自体が数GiB規模のメモリ・CPUを要してしまう)。
fn decrypt_import_with_max_work_factor(
    ciphertext: &[u8],
    passphrase: SecretString,
    max_log_n: u8,
) -> Result<Vec<u8>, ExportError> {
    let mut identity = Identity::new(passphrase);
    identity.set_max_work_factor(max_log_n);
    match age::decrypt(&identity, ciphertext) {
        Ok(plaintext) => Ok(plaintext),
        // ExcessiveWorkはパスフレーズを試す前に、暗号文自身が埋め込むワークファクタと
        // このマシンのキャリブレーション値だけで決まる(パスフレーズの正誤とは無関係)。
        // 「パスフレーズが誤っている」と同じ扱いにすると、正しいパスフレーズでも
        // 再入力を促すだけの誤った案内になるため、専用のエラーとして区別する。
        //
        // age::DecryptError::ExcessiveWorkのtargetフィールドは「このマシンの約1秒
        // キャリブレーション値」であり、set_max_work_factorで設定した実際の許容上限
        // (max_log_n)とは別物(ageのDisplay実装が推定時間を出すためだけに使う値)。
        // ユーザーに見せる上限値は、ここで実際に強制しているmax_log_nの方を使う。
        Err(age::DecryptError::ExcessiveWork { required, .. }) => {
            Err(ExportError::ExcessiveWork { required, allowed_max: max_log_n })
        }
        Err(_) => Err(ExportError::DecryptionFailed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passphrase(s: &str) -> SecretString {
        SecretString::from(s.to_owned())
    }

    #[test]
    fn encrypt_then_decrypt_round_trips() {
        let plaintext = b"{\"profile_name\":\"p\",\"rules\":[]}";

        let encrypted = encrypt_for_export(plaintext, passphrase("correct-horse")).unwrap();
        let decrypted = decrypt_import(&encrypted, passphrase("correct-horse")).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn decrypting_with_the_wrong_passphrase_fails() {
        let encrypted = encrypt_for_export(b"secret", passphrase("correct-horse")).unwrap();

        let result = decrypt_import(&encrypted, passphrase("wrong-horse"));

        assert!(matches!(result, Err(ExportError::DecryptionFailed)));
    }

    #[test]
    fn tampered_ciphertext_fails_to_decrypt() {
        let mut encrypted = encrypt_for_export(b"secret data", passphrase("pw")).unwrap();
        let last = encrypted.len() - 1;
        encrypted[last] ^= 0xFF;

        let result = decrypt_import(&encrypted, passphrase("pw"));

        assert!(matches!(result, Err(ExportError::DecryptionFailed)));
    }

    #[test]
    fn garbage_bytes_are_rejected_instead_of_panicking() {
        let result = decrypt_import(b"not an age file at all", passphrase("pw"));

        assert!(matches!(result, Err(ExportError::DecryptionFailed)));
    }

    #[test]
    fn encrypting_with_an_empty_passphrase_is_rejected() {
        let result = encrypt_for_export(b"secret", passphrase(""));

        assert!(matches!(result, Err(ExportError::EmptyPassphrase)));
    }

    #[test]
    fn a_work_factor_at_the_local_ceiling_still_decrypts() {
        // 実際に2^22相当のscryptを走らせるとGiB級のメモリ・CPUを要するため、
        // 安価なワークファクタ(2^4)と、テスト用に注入した同じ値の上限で検証する。
        // set_max_work_factorが「ちょうど上限と同じ値は許容する」ことの確認。
        let mut recipient = Recipient::new(passphrase("pw"));
        recipient.set_work_factor(4);
        let encrypted = age::encrypt(&recipient, b"secret").unwrap();

        let result = decrypt_import_with_max_work_factor(&encrypted, passphrase("pw"), 4);

        assert_eq!(result.unwrap(), b"secret");
    }

    #[test]
    fn a_work_factor_beyond_the_local_ceiling_is_reported_distinctly_from_a_wrong_passphrase() {
        // エクスポート元マシンがこのマシンより高速だったことを、安価なワークファクタの
        // 組み合わせ(4 > 上限2)で模擬する。実際の値(MAX_WORK_FACTOR_LOG_N=22)そのものでは
        // GiB級のメモリを要してしまうため、ここでは注入した小さい上限でロジックのみ検証する。
        let mut recipient = Recipient::new(passphrase("pw"));
        recipient.set_work_factor(4);
        let encrypted = age::encrypt(&recipient, b"secret").unwrap();

        let result = decrypt_import_with_max_work_factor(&encrypted, passphrase("pw"), 2);

        match result {
            Err(ExportError::ExcessiveWork { required, allowed_max }) => {
                assert_eq!(required, 4);
                // 表示する上限値は、age側のtarget(このマシンの1秒キャリブレーション値。
                // ここでは2ではない値になり得る)ではなく、実際に強制した注入値(2)であるはず。
                assert_eq!(allowed_max, 2);
            }
            other => panic!("ExcessiveWorkを期待したが、実際は: {other:?}"),
        }
    }

    #[test]
    fn decrypt_import_enforces_the_public_ceiling_constant_not_just_the_test_helpers_value() {
        // これは「decrypt_importがMAX_WORK_FACTOR_LOG_Nを使っている」ことの完全な証明では
        // ない(2^22相当の暗号文を実際に生成するテストは数GiB規模のコストを要するため、
        // ここでは低いワークファクタが定数より小さい上限でも問題なく通ることのみを確認する)。
        // 定数がヘルパーと食い違っていないかは目視でも確認すること。
        let mut recipient = Recipient::new(passphrase("pw"));
        recipient.set_work_factor(4);
        let encrypted = age::encrypt(&recipient, b"secret").unwrap();

        assert!(MAX_WORK_FACTOR_LOG_N > 4, "この定数より小さいワークファクタでのテストが前提");
        let result = decrypt_import(&encrypted, passphrase("pw"));

        assert_eq!(result.unwrap(), b"secret");
    }

    #[test]
    fn two_encryptions_of_the_same_plaintext_and_passphrase_produce_different_ciphertext() {
        // ageは内部でランダムなsalt/ephemeral値を使うため、同じ平文・パスフレーズでも
        // 暗号文は毎回変わるはず。
        let a = encrypt_for_export(b"same plaintext", passphrase("pw")).unwrap();
        let b = encrypt_for_export(b"same plaintext", passphrase("pw")).unwrap();

        assert_ne!(a, b);
    }

    // 貼り付けやTTY入力で前後に混ざった空白を、復号側で吸収する挙動のテスト用。
    // ワークファクタを下げた暗号文を使うのは、既定値のscryptを何度も走らせて遅くならないようにするため。
    fn encrypt_with_cheap_work_factor(plaintext: &[u8], passphrase_text: &str) -> Vec<u8> {
        let mut recipient = Recipient::new(passphrase(passphrase_text));
        recipient.set_work_factor(4);
        age::encrypt(&recipient, plaintext).unwrap()
    }

    #[test]
    fn surrounding_whitespace_is_ignored_when_the_passphrase_as_typed_fails() {
        let encrypted = encrypt_with_cheap_work_factor(b"secret", "correct-horse");

        // 半角スペース・改行・タブ・全角スペース・NBSPが前後に混ざった貼り付けを想定する。
        for pasted in [
            "correct-horse ",
            " correct-horse",
            "correct-horse\n",
            "correct-horse\r\n",
            "\tcorrect-horse\t",
            "\u{3000}correct-horse\u{3000}",
            "\u{a0}correct-horse\u{a0}",
        ] {
            let result = decrypt_import(&encrypted, passphrase(pasted));
            assert_eq!(result.unwrap(), b"secret", "入力: {pasted:?}");
        }
    }

    #[test]
    fn the_report_says_the_passphrase_was_trimmed_only_when_the_retry_was_needed() {
        let encrypted = encrypt_for_export(b"secret", passphrase("correct-horse")).unwrap();

        let exact = decrypt_import_reporting_trim(&encrypted, passphrase("correct-horse")).unwrap();
        assert!(!exact.passphrase_trimmed, "入力どおりで復号できたときは、除いていない");

        let padded = decrypt_import_reporting_trim(&encrypted, passphrase(" correct-horse\n")).unwrap();
        assert!(padded.passphrase_trimmed, "空白を除いて再試行して復号できたときは、除いたと報告する");
        assert_eq!(padded.plaintext, b"secret");
    }

    #[test]
    fn the_report_says_nothing_was_trimmed_when_the_passphrase_itself_has_whitespace() {
        let encrypted = encrypt_for_export(b"secret", passphrase(" spaced ")).unwrap();

        let as_typed = decrypt_import_reporting_trim(&encrypted, passphrase(" spaced ")).unwrap();

        assert!(!as_typed.passphrase_trimmed, "前後に空白を含むパスフレーズは、入力どおりで復号でき、除いていない");
    }

    #[test]
    fn a_passphrase_that_itself_has_surrounding_whitespace_decrypts_as_typed() {
        let encrypted = encrypt_with_cheap_work_factor(b"secret", " spaced passphrase ");

        let result = decrypt_import(&encrypted, passphrase(" spaced passphrase "));

        assert_eq!(result.unwrap(), b"secret");
    }

    #[test]
    fn whitespace_is_never_added_to_the_typed_passphrase() {
        // 空白を含むパスフレーズで作ったファイルを、空白を落として入力した場合は復号できない
        // (再試行は「除く」方向だけで、「足す」方向は試さない)。
        let encrypted = encrypt_with_cheap_work_factor(b"secret", "correct-horse ");

        let result = decrypt_import(&encrypted, passphrase("correct-horse"));

        assert!(matches!(result, Err(ExportError::DecryptionFailed)));
    }

    #[test]
    fn whitespace_inside_the_passphrase_is_kept() {
        let encrypted = encrypt_with_cheap_work_factor(b"secret", "correct horse");

        let with_trailing = decrypt_import(&encrypted, passphrase("correct horse "));
        let collapsed = decrypt_import(&encrypted, passphrase("correct  horse"));

        assert_eq!(with_trailing.unwrap(), b"secret");
        assert!(matches!(collapsed, Err(ExportError::DecryptionFailed)));
    }

    #[test]
    fn a_wrong_passphrase_is_still_rejected_when_it_has_surrounding_whitespace() {
        let encrypted = encrypt_with_cheap_work_factor(b"secret", "correct-horse");

        let result = decrypt_import(&encrypted, passphrase(" wrong-horse\n"));

        assert!(matches!(result, Err(ExportError::DecryptionFailed)));
    }

    #[test]
    fn a_whitespace_only_passphrase_is_rejected_without_retrying_as_empty() {
        let encrypted = encrypt_with_cheap_work_factor(b"secret", "correct-horse");

        let result = decrypt_import(&encrypted, passphrase("  \n"));

        assert!(matches!(result, Err(ExportError::DecryptionFailed)));
    }

    #[test]
    fn an_excessive_work_factor_is_reported_as_such_even_with_surrounding_whitespace() {
        // 空白付きの入力でも、ワークファクタ超過は「パスフレーズが誤っている」に化けない。
        let encrypted = encrypt_with_cheap_work_factor(b"secret", "correct-horse");

        let result = decrypt_with_whitespace_fallback(&encrypted, passphrase("correct-horse "), 2);

        assert!(matches!(result, Err(ExportError::ExcessiveWork { required: 4, allowed_max: 2 })));
    }

    #[test]
    fn retry_candidate_is_none_when_there_is_nothing_to_strip() {
        // 除くものが無いのに再試行すると、同じ値で無駄にscryptを走らせることになる。
        assert_eq!(retry_candidate("abc"), None);
        assert_eq!(retry_candidate(""), None);
    }

    #[test]
    fn retry_candidate_strips_surrounding_whitespace_and_invisible_characters() {
        for (typed, expected) in [
            ("abc ", "abc"),
            (" abc", "abc"),
            ("\u{3000}abc\n", "abc"),
            ("\u{a0}abc\t", "abc"),
            ("\u{200B}abc", "abc"),
            ("abc\u{FEFF}", "abc"),
            ("\u{2060}abc\u{200D}", "abc"),
            ("\u{200C}abc", "abc"),
        ] {
            assert_eq!(retry_candidate(typed), Some(expected), "入力: {typed:?}");
        }
    }

    #[test]
    fn retry_candidate_keeps_whitespace_inside_the_passphrase() {
        assert_eq!(retry_candidate("a  b "), Some("a  b"));
        assert_eq!(retry_candidate(" a b"), Some("a b"));
    }

    #[test]
    fn retry_candidate_is_none_when_only_noise_remains() {
        // 空になる場合に再試行すると、空のパスフレーズを試すことになる。
        assert_eq!(retry_candidate("  \n"), None);
        assert_eq!(retry_candidate("\u{200B}\u{FEFF}"), None);
    }

    #[test]
    fn invisible_characters_around_the_passphrase_are_ignored() {
        let encrypted = encrypt_with_cheap_work_factor(b"secret", "correct-horse");

        for pasted in ["\u{FEFF}correct-horse", "correct-horse\u{200B}", "\u{2060}correct-horse\u{200D}"] {
            let result = decrypt_import(&encrypted, passphrase(pasted));
            assert_eq!(result.unwrap(), b"secret", "入力: {pasted:?}");
        }
    }

    #[test]
    fn inner_whitespace_survives_the_retry() {
        // 前後に空白が付いて再試行になっても、内部の空白(2つ連続)は変わらない。
        let encrypted = encrypt_with_cheap_work_factor(b"secret", "correct  horse");

        let result = decrypt_import(&encrypted, passphrase("correct  horse "));

        assert_eq!(result.unwrap(), b"secret");
    }

    #[test]
    fn a_file_made_with_an_empty_passphrase_is_not_opened_by_a_whitespace_only_input() {
        // 空白のみの入力を空文字として再試行すると、空のパスフレーズで作ったファイルが開いてしまう。
        // (encrypt_for_exportは空を拒否するため、ageを直接使って空のパスフレーズの暗号文を作る。)
        let encrypted = encrypt_with_cheap_work_factor(b"secret", "");

        let result = decrypt_import(&encrypted, passphrase("  \n"));

        assert!(matches!(result, Err(ExportError::DecryptionFailed)));
    }
}
