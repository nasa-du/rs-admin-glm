//! Access-JWT signing (spec §4.1 / ADR 0001).
//!
//! HS256, single secret, claims `sub` (user uuid) / `email` / `role` (single
//! value) / `iat` / `exp`, TTL 15 minutes. The token lives in frontend memory
//! only; the signing time is injected so claim construction stays pure and
//! unit-testable.

use anyhow::{Context, Result};
use chrono::Utc;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};

use super::Role;

/// Access-token lifetime (spec §4.1: 15 min — the RBAC change-propagation
/// upper bound, §3).
pub const ACCESS_TOKEN_TTL_SECS: i64 = 15 * 60;

/// The access-JWT claim set (spec §4.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessClaims {
    /// User uuid (= `users.id`).
    pub sub: String,
    /// Login identifier.
    pub email: String,
    /// Single-value hierarchical role. The domain type keeps the wire
    /// spelling defined in one place (serde `rename_all`) instead of a bare
    /// string re-encoded by hand.
    pub role: Role,
    /// Issued-at, unix seconds.
    pub iat: i64,
    /// Expiry, unix seconds (`iat + ACCESS_TOKEN_TTL_SECS`).
    pub exp: i64,
}

/// Sign the access JWT for a user at `now` (unix seconds). `iat`/`exp` are
/// taken from `now` so tests can pin them exactly.
pub fn issue_access(
    user_id: impl Into<String>,
    email: &str,
    role: Role,
    now: i64,
    key: &EncodingKey,
) -> Result<String> {
    let claims = AccessClaims {
        sub: user_id.into(),
        email: email.to_owned(),
        role,
        iat: now,
        exp: now + ACCESS_TOKEN_TTL_SECS,
    };
    encode(&Header::new(Algorithm::HS256), &claims, key).context("failed to sign the access token")
}

/// Convenience wrapper for `issue_access` at wall-clock time.
pub fn issue_access_now(
    user_id: impl Into<String>,
    email: &str,
    role: Role,
    key: &EncodingKey,
) -> Result<String> {
    issue_access(user_id, email, role, Utc::now().timestamp(), key)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"test-secret-that-is-at-least-32-bytes";

    #[test]
    fn claims_carry_spec_fields_and_15_minute_ttl() {
        // A fixed future signing time: decoding with jsonwebtoken's defaults
        // validates `exp`, so the token must not already be expired.
        const NOW: i64 = 2_000_000_000;
        let key = EncodingKey::from_secret(SECRET);
        let token = issue_access(
            "2f0d8e52-93e6-4c9e-9db5-7c1a3e6b41cc",
            "root@example.com",
            Role::Superadmin,
            NOW,
            &key,
        )
        .unwrap();

        // Independent check: decode with jsonwebtoken's defaults (HS256,
        // exp validated) and read the claims back out.
        let token_data = jsonwebtoken::decode::<AccessClaims>(
            &token,
            &jsonwebtoken::DecodingKey::from_secret(SECRET),
            &jsonwebtoken::Validation::new(Algorithm::HS256),
        )
        .unwrap();

        assert_eq!(
            token_data.claims.sub,
            "2f0d8e52-93e6-4c9e-9db5-7c1a3e6b41cc"
        );
        assert_eq!(token_data.claims.email, "root@example.com");
        assert_eq!(token_data.claims.role, Role::Superadmin);
        assert_eq!(token_data.claims.iat, NOW);
        assert_eq!(
            token_data.claims.exp - token_data.claims.iat,
            ACCESS_TOKEN_TTL_SECS
        );
        assert_eq!(ACCESS_TOKEN_TTL_SECS, 900, "spec §4.1 pins 15 minutes");
    }

    #[test]
    fn signature_is_verified_by_an_independent_decoding_key() {
        let other_key = EncodingKey::from_secret(b"a-different-secret-of-at-least-32-bytes!!");
        let token = issue_access("id", "a@b.c", Role::User, 0, &other_key).unwrap();

        let result = jsonwebtoken::decode::<AccessClaims>(
            &token,
            &jsonwebtoken::DecodingKey::from_secret(SECRET),
            &jsonwebtoken::Validation::new(Algorithm::HS256),
        );
        assert!(result.is_err(), "wrong key must not verify");
    }
}
