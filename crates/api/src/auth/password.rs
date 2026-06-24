//! Password hashing with argon2id.
//!
//! Parameters are pinned explicitly (not `Argon2::default()`) so they can't
//! drift across crate versions: 19 MiB memory, 2 iterations, 1 lane — the
//! OWASP-recommended argon2id floor. Each hash carries a fresh random salt in
//! its PHC string. Passwords are a *separate* primitive from the AES-GCM used
//! for Plaid tokens (`crypto.rs`); the two are never crossed.

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};

/// A fixed password used only to build the dummy verification hash. Its value is
/// irrelevant — it never authenticates anyone.
const DUMMY_PASSWORD: &str = "squirrel-timing-equalization-dummy";

/// Build the pinned argon2id hasher. Errors only if the params are invalid,
/// which is impossible for these constants — but we surface it rather than panic.
fn hasher() -> anyhow::Result<Argon2<'static>> {
    let params = Params::new(19_456, 2, 1, None)
        .map_err(|e| anyhow::anyhow!("invalid argon2 params: {e}"))?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

/// Hash a password into a PHC string (`$argon2id$...`) with a fresh random salt.
pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = hasher()?
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("password hashing failed: {e}"))?;
    Ok(hash.to_string())
}

/// Verify a password against a stored PHC string. Returns `false` for any
/// mismatch or malformed/empty hash — never an error, never a panic.
pub fn verify_password(password: &str, phc: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(phc) else {
        return false;
    };
    // The hasher instance only carries fallback params; the real params come
    // from the PHC string, so this verifies correctly regardless of our pins.
    let Ok(argon2) = hasher() else {
        return false;
    };
    argon2.verify_password(password.as_bytes(), &parsed).is_ok()
}

/// Build a real PHC hash of a throwaway password, held in app state so the login
/// path can run a verify even when the email is unknown — equalizing timing so
/// "no such user" and "wrong password" cost the same.
pub fn dummy_hash() -> String {
    hash_password(DUMMY_PASSWORD)
        .expect("hashing a constant password with valid params cannot fail")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_argon2id_and_verifies() {
        let phc = hash_password("correct horse battery staple").unwrap();
        assert!(phc.starts_with("$argon2id$"), "got {phc}");
        assert!(verify_password("correct horse battery staple", &phc));
        assert!(!verify_password("wrong password", &phc));
    }

    #[test]
    fn empty_or_malformed_hash_never_verifies() {
        assert!(!verify_password("", ""));
        assert!(!verify_password("anything", "not-a-phc-string"));
    }
}
