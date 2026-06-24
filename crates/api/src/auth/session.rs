//! Opaque session tokens.
//!
//! A token is 32 bytes of OS randomness, base64url-encoded for the cookie. We
//! never store the token — only its SHA-256 (a fast hash is correct here: the
//! input is already 256 bits of uniform randomness, so there's nothing to
//! brute-force; argon2 would be pointless). The hash is the DB lookup key.

use chrono::{DateTime, Duration, Utc};
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::{Digest, Sha256};

/// Sliding inactivity window applied at creation.
const SLIDING_WINDOW_DAYS: i64 = 7;
/// Hard ceiling on a session's lifetime regardless of activity.
const ABSOLUTE_CAP_DAYS: i64 = 30;

/// Cookie `Max-Age`, in seconds — the absolute cap (30 days).
pub const COOKIE_MAX_AGE_SECS: i64 = ABSOLUTE_CAP_DAYS * 24 * 60 * 60;

/// Mint a new opaque token. Returns `(raw_token_for_cookie, sha256_for_storage)`.
pub fn new_token() -> (String, Vec<u8>) {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let raw =
        base64::engine::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, bytes);
    let hash = hash_token(&raw);
    (raw, hash)
}

/// SHA-256 of a raw token, as bytes — the form stored and queried in `sessions`.
pub fn hash_token(raw: &str) -> Vec<u8> {
    Sha256::digest(raw.as_bytes()).to_vec()
}

/// Absolute expiry for a freshly created session:
/// `LEAST(now + sliding_window, created_at + absolute_cap)`. For a new session
/// `created_at == now`, so this is `now + 7d`; the cap bounds any later sliding.
pub fn new_expiry(now: DateTime<Utc>) -> DateTime<Utc> {
    let sliding = now + Duration::days(SLIDING_WINDOW_DAYS);
    let cap = now + Duration::days(ABSOLUTE_CAP_DAYS);
    sliding.min(cap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_round_trips_to_its_stored_hash() {
        let (raw, stored) = new_token();
        // The cookie value re-hashes to exactly what we'd look up in the DB.
        assert_eq!(hash_token(&raw), stored);
        assert_eq!(stored.len(), 32);
        // base64url-no-pad of 32 bytes is 43 chars.
        assert_eq!(raw.len(), 43);
    }

    #[test]
    fn tokens_are_unique() {
        let (a, _) = new_token();
        let (b, _) = new_token();
        assert_ne!(a, b);
    }

    #[test]
    fn new_expiry_is_seven_days_out() {
        let now = Utc::now();
        assert_eq!(new_expiry(now), now + Duration::days(SLIDING_WINDOW_DAYS));
    }
}
