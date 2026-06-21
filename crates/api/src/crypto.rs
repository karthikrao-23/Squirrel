//! Authenticated encryption for secrets at rest (Plaid access tokens).
//!
//! Uses AES-256-GCM. Each ciphertext is prefixed with its random 12-byte nonce,
//! so the stored blob is `nonce (12 bytes) || ciphertext+tag`. The 32-byte key
//! comes from the base64 `TOKEN_ENCRYPTION_KEY` env var (see `config.rs`).

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, Key, Nonce};

const NONCE_LEN: usize = 12;

/// Encrypt `plaintext`, returning `nonce || ciphertext`.
pub fn encrypt(key: &[u8; 32], plaintext: &[u8]) -> anyhow::Result<Vec<u8>> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|e| anyhow::anyhow!("encryption failed: {e}"))?;

    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(nonce.as_slice());
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypt a blob produced by [`encrypt`].
pub fn decrypt(key: &[u8; 32], blob: &[u8]) -> anyhow::Result<Vec<u8>> {
    if blob.len() < NONCE_LEN {
        anyhow::bail!("ciphertext too short");
    }
    let (nonce_bytes, ciphertext) = blob.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("decryption failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let key = [7u8; 32];
        let secret = b"access-sandbox-abc123";
        let blob = encrypt(&key, secret).unwrap();
        // Nonce is prepended, so the blob is longer than the plaintext.
        assert!(blob.len() > secret.len());
        assert_ne!(&blob[12..], secret); // actually encrypted, not stored in clear
        assert_eq!(decrypt(&key, &blob).unwrap(), secret);
    }

    #[test]
    fn wrong_key_fails() {
        let blob = encrypt(&[1u8; 32], b"hunter2").unwrap();
        assert!(decrypt(&[2u8; 32], &blob).is_err());
    }
}
