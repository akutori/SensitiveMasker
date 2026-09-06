//! プロファイル本体(JSONにシリアライズしたRuleProfile)の対称暗号化/復号。
//!
//! AAD(associated data)にプロファイル名を束ねる。これにより、DBファイルを直接書き換えて
//! 別の行の`rules_encrypted`/`nonce`を入れ替えても復号自体が失敗するようになる(AADが無いと、
//! 同じ鍵で暗号化された別プロファイルの暗号文をそのまま入れ替えても復号が成功してしまい、
//! 「name列は'personal'のままだが内容は'work'のルール」という取り違えが無警告で成立する)。

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand::RngExt;
use secrecy::{ExposeSecret, SecretBox};

use crate::key::KEY_LEN;

const NONCE_LEN: usize = 12;

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("復号に失敗しました(鍵が誤っているか、データが破損しています)")]
    DecryptionFailed,
}

pub struct Encrypted {
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
}

pub fn encrypt(key: &SecretBox<[u8; KEY_LEN]>, plaintext: &[u8], aad: &[u8]) -> Encrypted {
    let cipher = ChaCha20Poly1305::new(&Key::from(*key.expose_secret()));
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::rng().fill(&mut nonce_bytes);
    let nonce = Nonce::from(nonce_bytes);
    let ciphertext = cipher
        .encrypt(&nonce, Payload { msg: plaintext, aad })
        .expect("32バイト鍵での暗号化は失敗しない");
    Encrypted { ciphertext, nonce: nonce_bytes.to_vec() }
}

pub fn decrypt(
    key: &SecretBox<[u8; KEY_LEN]>,
    ciphertext: &[u8],
    nonce: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let cipher = ChaCha20Poly1305::new(&Key::from(*key.expose_secret()));
    // nonceは保存データ(DBの列)由来なので、破損等でサイズが不正な可能性がある。
    // ここでpanicせず、他の復号失敗と同じCryptoError扱いにする。
    let nonce = Nonce::try_from(nonce).map_err(|_| CryptoError::DecryptionFailed)?;
    cipher
        .decrypt(&nonce, Payload { msg: ciphertext, aad })
        .map_err(|_| CryptoError::DecryptionFailed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_of(byte: u8) -> SecretBox<[u8; KEY_LEN]> {
        SecretBox::new(Box::new([byte; KEY_LEN]))
    }

    #[test]
    fn encrypt_then_decrypt_round_trips() {
        let key = key_of(7);
        let plaintext = b"{\"profile_name\":\"p\",\"rules\":[]}";

        let encrypted = encrypt(&key, plaintext, b"p");
        let decrypted = decrypt(&key, &encrypted.ciphertext, &encrypted.nonce, b"p").unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn decrypting_with_the_wrong_key_fails() {
        let key_a = key_of(1);
        let key_b = key_of(2);
        let encrypted = encrypt(&key_a, b"secret", b"aad");

        let result = decrypt(&key_b, &encrypted.ciphertext, &encrypted.nonce, b"aad");
        assert!(matches!(result, Err(CryptoError::DecryptionFailed)));
    }

    #[test]
    fn tampered_ciphertext_fails_to_decrypt() {
        let key = key_of(3);
        let mut encrypted = encrypt(&key, b"secret data", b"aad");
        let last = encrypted.ciphertext.len() - 1;
        encrypted.ciphertext[last] ^= 0xFF;

        let result = decrypt(&key, &encrypted.ciphertext, &encrypted.nonce, b"aad");
        assert!(matches!(result, Err(CryptoError::DecryptionFailed)));
    }

    #[test]
    fn wrong_length_nonce_fails_cleanly_instead_of_panicking() {
        let key = key_of(4);
        let encrypted = encrypt(&key, b"secret", b"aad");

        let result = decrypt(&key, &encrypted.ciphertext, &[0u8; 3], b"aad");
        assert!(matches!(result, Err(CryptoError::DecryptionFailed)));
    }

    #[test]
    fn two_encryptions_of_the_same_plaintext_use_different_nonces() {
        let key = key_of(9);
        let a = encrypt(&key, b"same plaintext", b"aad");
        let b = encrypt(&key, b"same plaintext", b"aad");
        assert_ne!(a.nonce, b.nonce, "nonceは毎回ランダムに生成されるはず");
    }

    #[test]
    fn ciphertext_from_a_different_aad_context_fails_to_decrypt() {
        // 別の行(例: 別プロファイル名)向けに暗号化された暗号文を、そのままこの行のAADで
        // 復号しようとしても失敗するはず。
        let key = key_of(5);
        let encrypted = encrypt(&key, b"work profile rules", b"work");

        let result = decrypt(&key, &encrypted.ciphertext, &encrypted.nonce, b"personal");
        assert!(matches!(result, Err(CryptoError::DecryptionFailed)));
    }
}
