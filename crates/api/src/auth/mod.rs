//! Authentication: argon2id passwords, opaque DB-backed sessions, the
//! `AuthUser` request extractor, and the CSRF guard.
//!
//! Route protection is fail-closed by construction: any handler that wants a
//! user takes [`AuthUser`], and there's no way to obtain one without a valid
//! session cookie. Public routes simply omit the extractor.

pub mod csrf;
pub mod extractor;
pub mod password;
pub mod session;

pub use extractor::AuthUser;

/// The custom header the frontend sends on every mutating request. Cross-site JS
/// cannot set a custom header without a CORS preflight we never grant, so its
/// presence proves the request came from our own origin.
pub const CSRF_HEADER: &str = "X-Squirrel-CSRF";

/// Session cookie name. The `__Host-` prefix is only valid with `Secure`, so we
/// use the bare `sid` over plain-HTTP dev.
pub fn cookie_name(cookie_secure: bool) -> &'static str {
    if cookie_secure {
        "__Host-sid"
    } else {
        "sid"
    }
}

/// Build the `Set-Cookie` value for a freshly issued session. Attributes:
/// `HttpOnly`, `SameSite=Strict`, `Path=/`, no `Domain`, `Secure` in prod, and
/// `Max-Age` = the session absolute cap.
pub fn session_cookie(value: &str, cookie_secure: bool) -> String {
    let name = cookie_name(cookie_secure);
    let mut cookie = format!(
        "{name}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}",
        session::COOKIE_MAX_AGE_SECS
    );
    if cookie_secure {
        cookie.push_str("; Secure");
    }
    cookie
}

/// Build the `Set-Cookie` value that clears the session cookie. Must use the
/// same attributes (minus value/age) as [`session_cookie`] so the browser
/// actually overwrites it.
pub fn clear_cookie(cookie_secure: bool) -> String {
    let name = cookie_name(cookie_secure);
    let mut cookie = format!("{name}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0");
    if cookie_secure {
        cookie.push_str("; Secure");
    }
    cookie
}
