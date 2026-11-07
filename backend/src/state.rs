//! AppState — the shared, clonable handle every handler extracts (spec §1).
//!
//! JWT keys are constructed once at startup and reused for every sign;
//! HS256 symmetric keys are cheap to hold (ADR 0001: single secret, no
//! kid/JWKS rotation).

use anyhow::{Context, Result, ensure};
use jsonwebtoken::EncodingKey;
use sqlx::PgPool;

/// Application state injected into the router.
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub jwt: JwtKeys,
}

/// HS256 key pair for the access JWT. Only the encoding half exists while
/// auth is the only implemented module; the users-management milestone adds
/// the decoding half for the `AuthUser` extractor.
#[derive(Clone)]
pub struct JwtKeys {
    pub encoding: EncodingKey,
}

impl JwtKeys {
    /// Build from raw secret bytes, enforcing the ≥32-byte floor of spec §4.1.
    pub fn from_secret(secret: &[u8]) -> Result<Self> {
        ensure!(
            secret.len() >= 32,
            "JWT_SECRET must be at least 32 bytes of randomness (e.g. `openssl rand -base64 48`)"
        );
        Ok(Self {
            encoding: EncodingKey::from_secret(secret),
        })
    }

    /// Build from the `JWT_SECRET` environment variable (spec §9.2).
    pub fn from_env() -> Result<Self> {
        let secret = std::env::var("JWT_SECRET")
            .context("JWT_SECRET is not set — generate one with `openssl rand -base64 48`")?;
        Self::from_secret(secret.as_bytes())
            .with_context(|| "invalid JWT_SECRET (from backend/.env)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_short_secrets() {
        assert!(JwtKeys::from_secret(b"too-short").is_err());
        assert!(JwtKeys::from_secret(&[]).is_err());
    }

    #[test]
    fn accepts_min_length_secrets() {
        assert!(JwtKeys::from_secret(&[b'x'; 32]).is_ok());
    }
}
