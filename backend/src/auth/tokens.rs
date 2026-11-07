//! Refresh-token machinery (spec §2.2 / §4.1 / ADR 0001).
//!
//! The refresh token is a 256-bit opaque random string, not a JWT. The DB
//! stores only its SHA-256 hex; one login opens one token family. Rotation,
//! reuse detection and family revocation arrive with the refresh endpoint
//! (their own ticket) — this module provides the primitives they share.

use axum_extra::extract::cookie::{Cookie, SameSite};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[cfg(test)]
use crate::auth::jwt;

/// Cookie name of the refresh token (spec §4.1).
pub const REFRESH_COOKIE_NAME: &str = "rs_admin_refresh";

/// Cookie attributes: HttpOnly (invisible to JS), Secure, SameSite=Strict
/// (the CSRF first line of defense), Path narrowed to /api/auth (spec §4.1).
pub const REFRESH_COOKIE_PATH: &str = "/api/auth";

/// Sliding refresh window (spec §4.1): 7 days, reset on every rotation.
pub const REFRESH_TTL_DAYS: i64 = 7;

/// Entropy of a refresh token: 256 bits (spec §4.1).
const REFRESH_TOKEN_BYTES: usize = 32;

/// A freshly generated refresh token and the family it opens.
#[derive(Debug)]
pub struct NewRefreshToken {
    /// Hex-encoded plaintext — cookie value; never stored server-side.
    pub plaintext: String,
    /// One login = one token family (CONTEXT.md 令牌族).
    pub family_id: Uuid,
}

/// Generate a new refresh token and its family id.
pub fn generate_refresh() -> NewRefreshToken {
    let mut bytes = [0u8; REFRESH_TOKEN_BYTES];
    getrandom::fill(&mut bytes).expect("OS entropy source unavailable");
    NewRefreshToken {
        plaintext: hex(&bytes),
        family_id: Uuid::new_v4(),
    }
}

/// The stored form: SHA-256 hex of the plaintext (64 chars, spec §2.2).
pub fn hash_token(plaintext: &str) -> String {
    hex(&Sha256::digest(plaintext.as_bytes()))
}

/// Insert the first row of a token family: `expires_at = now + 7d`,
/// `consumed_at` NULL (spec §2.2).
pub async fn insert_family_root(
    pool: &sqlx::PgPool,
    user_id: Uuid,
    family_id: Uuid,
    token_hash: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO refresh_tokens (family_id, user_id, token_hash, expires_at) \
         VALUES ($1, $2, $3, now() + make_interval(days => $4))",
    )
    .bind(family_id)
    .bind(user_id)
    .bind(token_hash)
    // make_interval's `days` argument is int (i32), not bigint.
    .bind(REFRESH_TTL_DAYS as i32)
    .execute(pool)
    .await?;
    Ok(())
}

/// Build the `Set-Cookie` value for a refresh plaintext. `Max-Age` matches
/// the 7-day sliding window so the cookie survives browser restarts; every
/// rotation re-issues it.
pub fn refresh_cookie(plaintext: &str) -> Cookie<'static> {
    Cookie::build((REFRESH_COOKIE_NAME, plaintext.to_owned()))
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Strict)
        .path(REFRESH_COOKIE_PATH)
        .max_age(time::Duration::days(REFRESH_TTL_DAYS))
        .build()
}

/// Lowercase hex encoding.
fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[usize::from(byte >> 4)] as char);
        out.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_tokens_are_64_hex_chars_with_unique_families() {
        let first = generate_refresh();
        let second = generate_refresh();
        assert_eq!(first.plaintext.len(), 64);
        assert!(
            first
                .plaintext
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        );
        assert_ne!(first.plaintext, second.plaintext);
        assert_ne!(first.family_id, second.family_id);
    }

    #[test]
    fn hash_matches_reference_sha256_vectors() {
        // Independent source of truth: `echo -n abc | sha256sum`
        assert_eq!(
            hash_token("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hash_token(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn refresh_cookie_carries_the_spec_attributes() {
        let cookie = refresh_cookie("plaintext-value");
        let wire = cookie.to_string();
        assert!(
            wire.starts_with("rs_admin_refresh=plaintext-value"),
            "{wire}"
        );
        for attribute in [
            "HttpOnly",
            "Secure",
            "SameSite=Strict",
            "Path=/api/auth",
            "Max-Age=604800",
        ] {
            assert!(wire.contains(attribute), "missing {attribute} in {wire}");
        }
    }

    #[test]
    fn access_ttl_and_refresh_window_match_the_spec() {
        assert_eq!(jwt::ACCESS_TOKEN_TTL_SECS, 15 * 60);
        assert_eq!(REFRESH_TTL_DAYS, 7);
    }
}
