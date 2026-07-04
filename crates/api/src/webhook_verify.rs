//! Plaid webhook signature verification.
//!
//! `/api/plaid/webhook` is the only **public, mutating** route, so it can't trust
//! its body. Plaid signs every webhook with an ES256 JWT in the
//! `Plaid-Verification` header; this module verifies it against Plaid's published
//! key before a single byte of the payload is acted on. The checks, in order:
//!
//! 1. Pin `alg = ES256` and reject anything else (`none`/`HS256` ⇒ alg-confusion).
//! 2. Resolve the signing key by `kid` (cached per-`kid` with a TTL for rotation).
//! 3. Verify the JWT signature.
//! 4. Assert the JWT's `request_body_sha256` equals SHA-256 of the *raw* bytes,
//!    compared in constant time (binds the signature to *this* body).
//! 5. Enforce `iat` freshness (replay protection).
//!
//! Only then is the JSON parsed. Any failure is an error; nothing is acted on.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use plaid::webhooks::PlaidWebhook;

use crate::plaid_clients::PlaidClients;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// How long a fetched verification key is trusted before re-fetching. Bounds the
/// window during which a rotated-out key would still be accepted.
const KEY_TTL: Duration = Duration::from_secs(3600);
/// Accepted clock skew / replay window for the JWT `iat`.
const IAT_LEEWAY_SECS: i64 = 300;

#[derive(Debug, thiserror::Error)]
pub enum WebhookError {
    #[error("missing or malformed Plaid-Verification header")]
    MissingHeader,
    #[error("unsupported JWT algorithm (expected ES256)")]
    BadAlgorithm,
    #[error("missing key id (kid)")]
    MissingKid,
    #[error("could not fetch verification key: {0}")]
    KeyFetch(String),
    #[error("invalid signature")]
    InvalidSignature,
    #[error("request body hash mismatch")]
    BodyHashMismatch,
    #[error("stale webhook (iat outside the freshness window)")]
    Stale,
    #[error("malformed webhook body")]
    BadBody,
}

#[derive(Deserialize)]
struct Claims {
    iat: i64,
    request_body_sha256: String,
}

#[derive(Clone)]
struct CachedKey {
    x: String,
    y: String,
    fetched_at: Instant,
}

/// Verifies `Plaid-Verification` JWTs, caching signing keys by `kid`. Cheap to
/// clone (the cache is shared); lives in `AppState`.
#[derive(Clone, Default)]
pub struct WebhookVerifier {
    cache: Arc<RwLock<HashMap<String, CachedKey>>>,
}

impl WebhookVerifier {
    pub fn new() -> Self {
        Self::default()
    }

    /// Verify the header+body and, on success, return the parsed webhook.
    pub async fn verify_and_parse(
        &self,
        clients: &PlaidClients,
        verification_header: Option<&str>,
        body: &[u8],
    ) -> Result<PlaidWebhook, WebhookError> {
        let jwt = verification_header.ok_or(WebhookError::MissingHeader)?;

        // 1. Pin ES256 from the (unverified) header — rejects none/HS256 before we
        //    ever hand the token to a verifier keyed with an EC public key.
        let header = decode_header(jwt).map_err(|_| WebhookError::MissingHeader)?;
        if header.alg != Algorithm::ES256 {
            return Err(WebhookError::BadAlgorithm);
        }
        let kid = header.kid.ok_or(WebhookError::MissingKid)?;

        // 2 + 3. Resolve the key and verify the signature.
        let (x, y) = self.resolve_key(clients, &kid).await?;
        let decoding_key =
            DecodingKey::from_ec_components(&x, &y).map_err(|_| WebhookError::InvalidSignature)?;
        let mut validation = Validation::new(Algorithm::ES256);
        validation.required_spec_claims.clear(); // Plaid's JWT has no `exp`
        validation.validate_exp = false;
        validation.validate_aud = false;
        let data = decode::<Claims>(jwt, &decoding_key, &validation)
            .map_err(|_| WebhookError::InvalidSignature)?;

        // 4. Bind the signature to *this* body (constant-time hex compare).
        let computed = hex_lower(&Sha256::digest(body));
        let claimed = data.claims.request_body_sha256.as_bytes();
        if computed.len() != claimed.len() || !bool::from(computed.as_bytes().ct_eq(claimed)) {
            return Err(WebhookError::BodyHashMismatch);
        }

        // 5. Replay protection.
        let now = chrono::Utc::now().timestamp();
        if (now - data.claims.iat).abs() > IAT_LEEWAY_SECS {
            return Err(WebhookError::Stale);
        }

        // Now — and only now — the bytes are trustworthy enough to parse.
        serde_json::from_slice(body).map_err(|_| WebhookError::BadBody)
    }

    /// Return the `(x, y)` EC coordinates for `kid`, from cache if fresh, else by
    /// fetching from Plaid and caching. Honors rotation via the TTL.
    async fn resolve_key(
        &self,
        clients: &PlaidClients,
        kid: &str,
    ) -> Result<(String, String), WebhookError> {
        if let Some(key) = self.cache.read().expect("cache lock").get(kid) {
            if key.fetched_at.elapsed() < KEY_TTL {
                return Ok((key.x.clone(), key.y.clone()));
            }
        }

        // A `kid` belongs to exactly one Plaid app, and its key is only fetchable
        // with that app's credentials — so try each configured app until one
        // returns it. (The webhook's item_id isn't trustworthy pre-verification,
        // so we can't use it to pick the app up front.)
        let mut last_err = None;
        for plaid in clients.configured() {
            match plaid.get_webhook_verification_key(kid).await {
                Ok(key) => {
                    if key.alg != "ES256" || key.kty != "EC" {
                        return Err(WebhookError::BadAlgorithm);
                    }
                    self.cache.write().expect("cache lock").insert(
                        kid.to_string(),
                        CachedKey {
                            x: key.x.clone(),
                            y: key.y.clone(),
                            fetched_at: Instant::now(),
                        },
                    );
                    return Ok((key.x, key.y));
                }
                Err(e) => last_err = Some(e),
            }
        }
        Err(WebhookError::KeyFetch(
            last_err
                .map(|e| e.to_string())
                .unwrap_or_else(|| "no configured Plaid app".to_string()),
        ))
    }

    #[cfg(test)]
    fn insert_key_for_test(&self, kid: &str, x: &str, y: &str) {
        self.cache.write().unwrap().insert(
            kid.to_string(),
            CachedKey {
                x: x.to_string(),
                y: y.to_string(),
                fetched_at: Instant::now(),
            },
        );
    }
}

/// Lowercase hex encoding (matches Plaid's `request_body_sha256`).
fn hex_lower(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};
    use serde_json::json;

    const TEST_KID: &str = "test-kid";
    const TEST_X: &str = "6IPd2ffQbpjGx1ARddyoWkvt3PdkhqxEWb0kpBc-aAw";
    const TEST_Y: &str = "C2Q6VWhnugWyNFrvrPOIdT3K6g7VB5LsiQv7QRyW8bo";
    // P-256 private key matching TEST_X/TEST_Y, for signing test tokens only.
    const TEST_SK_PEM: &str = "-----BEGIN PRIVATE KEY-----\n\
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgF+s45FDS4s6P4+tP\n\
+n8D7gunWZbKRojvOLHU2Fs9/HShRANCAATog93Z99BumMbHUBF13KhaS+3c92SG\n\
rERZvSSkFz5oDAtkOlVoZ7oFsjRa76zziHU9yuoO1QeS7IkL+0EclvG6\n\
-----END PRIVATE KEY-----\n";

    const BODY: &[u8] =
        br#"{"webhook_type":"HOLDINGS","webhook_code":"DEFAULT_UPDATE","item_id":"item_1"}"#;

    fn dummy_clients() -> PlaidClients {
        // Never actually fetched: the tests seed the key cache, so no HTTP occurs.
        PlaidClients::new(plaid::PlaidEnv::Sandbox, &[])
    }

    fn verifier() -> WebhookVerifier {
        let v = WebhookVerifier::new();
        v.insert_key_for_test(TEST_KID, TEST_X, TEST_Y);
        v
    }

    fn sign(body: &[u8], iat: i64, alg: Algorithm, kid: Option<&str>) -> String {
        let mut header = Header::new(alg);
        header.kid = kid.map(str::to_string);
        let claims = json!({
            "iat": iat,
            "request_body_sha256": hex_lower(&Sha256::digest(body)),
        });
        let key = match alg {
            Algorithm::ES256 => EncodingKey::from_ec_pem(TEST_SK_PEM.as_bytes()).unwrap(),
            _ => EncodingKey::from_secret(b"hmac-secret"),
        };
        encode(&header, &claims, &key).unwrap()
    }

    fn now() -> i64 {
        chrono::Utc::now().timestamp()
    }

    #[tokio::test]
    async fn valid_webhook_verifies() {
        let jwt = sign(BODY, now(), Algorithm::ES256, Some(TEST_KID));
        let hook = verifier()
            .verify_and_parse(&dummy_clients(), Some(&jwt), BODY)
            .await
            .unwrap();
        assert_eq!(hook.item_id, "item_1");
        assert!(hook.is_investments_update());
    }

    #[tokio::test]
    async fn missing_header_is_rejected() {
        let err = verifier()
            .verify_and_parse(&dummy_clients(), None, BODY)
            .await
            .unwrap_err();
        assert!(matches!(err, WebhookError::MissingHeader));
    }

    #[tokio::test]
    async fn hs256_token_is_rejected() {
        // alg-confusion: an attacker signs HS256 with the (public) key material.
        let jwt = sign(BODY, now(), Algorithm::HS256, Some(TEST_KID));
        let err = verifier()
            .verify_and_parse(&dummy_clients(), Some(&jwt), BODY)
            .await
            .unwrap_err();
        assert!(matches!(err, WebhookError::BadAlgorithm));
    }

    #[tokio::test]
    async fn tampered_body_is_rejected() {
        // Signature is valid, but the body differs from the signed hash.
        let jwt = sign(BODY, now(), Algorithm::ES256, Some(TEST_KID));
        let tampered =
            br#"{"webhook_type":"HOLDINGS","webhook_code":"DEFAULT_UPDATE","item_id":"EVIL"}"#;
        let err = verifier()
            .verify_and_parse(&dummy_clients(), Some(&jwt), tampered)
            .await
            .unwrap_err();
        assert!(matches!(err, WebhookError::BodyHashMismatch));
    }

    #[tokio::test]
    async fn stale_iat_is_rejected() {
        let jwt = sign(BODY, now() - 3600, Algorithm::ES256, Some(TEST_KID));
        let err = verifier()
            .verify_and_parse(&dummy_clients(), Some(&jwt), BODY)
            .await
            .unwrap_err();
        assert!(matches!(err, WebhookError::Stale));
    }

    #[tokio::test]
    async fn wrong_key_fails_signature() {
        // A token signed by our test key but verified against a *different* key.
        let jwt = sign(BODY, now(), Algorithm::ES256, Some("other-kid"));
        let v = WebhookVerifier::new();
        // Seed "other-kid" with a mismatched public key (swap x/y).
        v.insert_key_for_test("other-kid", TEST_Y, TEST_X);
        let err = v
            .verify_and_parse(&dummy_clients(), Some(&jwt), BODY)
            .await
            .unwrap_err();
        assert!(matches!(err, WebhookError::InvalidSignature));
    }
}
