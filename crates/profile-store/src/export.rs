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

pub fn decrypt_import(ciphertext: &[u8], passphrase: SecretString) -> Result<Vec<u8>, ExportError> {
    decrypt_with_whitespace_fallback(ciphertext, passphrase, MAX_WORK_FACTOR_LOG_N)
}

// 貼り付けやTTY入力で前後に空白(改行・タブ・全角スペース等)が混ざっていても復号できるよう、
// 入力のまま復号に失敗したときだけ、前後の空白を除いたパスフレーズで1回だけ再試行する。
// 入力のまま成功する場合は従来どおりで、前後に空白を含むパスフレーズで作ったファイルも
// 復号できる(空白を「足す」方向は試さない)。ワークファクタ超過はパスフレーズの正誤と
// 無関係なので、再試行しない。
fn decrypt_with_whitespace_fallback(
    ciphertext: &[u8],
    passphrase: SecretString,
    max_log_n: u8,
) -> Result<Vec<u8>, ExportError> {
    // 最初の試行がパスフレーズを消費するため、再試行用の値は先に作る。前後に空白が無い
    // (通常の)場合は作らず、機微な値のコピーを増やさない。
    let typed = passphrase.expose_secret();
    let trimmed = typed.trim();
    let fallback = (!trimmed.is_empty() && trimmed.len() != typed.len())
        .then(|| SecretString::from(trimmed.to_owned()));

    match (decrypt_import_with_max_work_factor(ciphertext, passphrase, max_log_n), fallback) {
        (Err(ExportError::DecryptionFailed), Some(trimmed)) => {
            decrypt_import_with_max_work_factor(ciphertext, trimmed, max_log_n)
        }
        (result, _) => result,
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
}
