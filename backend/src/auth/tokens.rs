//! Refresh-token machinery (spec §2.2 / §4.1 / ADR 0001).
//!
//! The refresh token is a 256-bit opaque random string, not a JWT. The DB
//! stores only its SHA-256 hex; one login opens one token family. This module
//! provides the primitives shared by login (open a family) and the refresh
//! endpoint (rotate within a family, clear the cookie on logout).

use axum_extra::extract::cookie::{Cookie, SameSite};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::{Executor, FromRow, Postgres, Transaction};
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

/// Generate a new refresh token and its family id (login).
pub fn generate_refresh() -> NewRefreshToken {
    NewRefreshToken {
        plaintext: generate_plaintext(),
        family_id: Uuid::new_v4(),
    }
}

/// Generate the successor of a rotation: fresh entropy, same family.
pub fn generate_refresh_in_family(family_id: Uuid) -> NewRefreshToken {
    NewRefreshToken {
        plaintext: generate_plaintext(),
        family_id,
    }
}

/// 256 bits of OS entropy, hex-encoded.
fn generate_plaintext() -> String {
    let mut bytes = [0u8; REFRESH_TOKEN_BYTES];
    getrandom::fill(&mut bytes).expect("OS entropy source unavailable");
    hex(&bytes)
}

/// The stored form: SHA-256 hex of the plaintext (64 chars, spec §2.2).
pub fn hash_token(plaintext: &str) -> String {
    hex(&Sha256::digest(plaintext.as_bytes()))
}

/// Insert a token row: `expires_at = now + 7d`, `consumed_at` NULL
/// (spec §2.2). Works on a pool (family root, login) and on an open
/// transaction (rotation, refresh).
pub async fn insert_token<'e, E>(
    executor: E,
    user_id: Uuid,
    family_id: Uuid,
    token_hash: &str,
) -> Result<(), sqlx::Error>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query(
        "INSERT INTO refresh_tokens (family_id, user_id, token_hash, expires_at) \
         VALUES ($1, $2, $3, now() + make_interval(days => $4))",
    )
    .bind(family_id)
    .bind(user_id)
    .bind(token_hash)
    // make_interval's `days` argument is int (i32), not bigint.
    .bind(REFRESH_TTL_DAYS as i32)
    .execute(executor)
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

/// Removal cookie for logout: the same name/path/attributes as the session
/// cookie, so the browser overwrites the one it holds (`Max-Age=0` plus a
/// past expiry).
pub fn clear_refresh_cookie() -> Cookie<'static> {
    let mut cookie = refresh_cookie("");
    cookie.make_removal();
    cookie
}

/// A `refresh_tokens` row as the refresh flow reads it.
#[derive(Debug, FromRow)]
pub(crate) struct RefreshRow {
    pub(crate) id: Uuid,
    pub(crate) family_id: Uuid,
    pub(crate) user_id: Uuid,
    pub(crate) consumed_at: Option<DateTime<Utc>>,
    /// Evaluated by Postgres so the DB clock — not the app server's — decides
    /// expiry.
    pub(crate) is_expired: bool,
}

/// Advisory-lock key for one token family: the first 8 UUID bytes as a
/// bigint. Derived in Rust so the handler and its tests agree by construction.
pub fn family_lock_key(family_id: Uuid) -> i64 {
    i64::from_be_bytes(
        family_id.as_bytes()[..8]
            .try_into()
            .expect("a UUID has at least 8 bytes"),
    )
}

/// Serialize every mutation of one family — rotation, reuse revocation,
/// logout — on a transaction-scoped advisory lock.
///
/// Why this lock is load-bearing: under READ COMMITTED a family `DELETE`
/// takes its snapshot when the statement starts. A concurrent rotation that
/// has already inserted its successor but not yet committed is invisible to
/// that statement, which then blocks on the consumed row and finishes
/// without deleting the successor — the family survives a revocation that
/// reported success. Holding the family lock makes a rotation either finish
/// first (so the delete sees and removes its successor) or start after the
/// revocation (and find no token). Any future multi-family revocation
/// (password change, disable) must take these locks too, ordered by
/// `family_id` to avoid deadlocks.
pub(crate) async fn lock_family(
    tx: &mut Transaction<'_, Postgres>,
    family_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(family_lock_key(family_id))
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// The family that owns `token_hash`, if the row still exists. Callers must
/// take [`lock_family`] next and re-read the row under that lock: between the
/// two statements the token may be rotated or revoked.
pub(crate) async fn family_of_token(
    tx: &mut Transaction<'_, Postgres>,
    token_hash: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT family_id FROM refresh_tokens WHERE token_hash = $1")
        .bind(token_hash)
        .fetch_optional(&mut **tx)
        .await
}

/// The row matching `token_hash`, locked for the rotation transaction. Call
/// only while holding the family lock.
pub(crate) async fn find_for_update(
    tx: &mut Transaction<'_, Postgres>,
    token_hash: &str,
) -> Result<Option<RefreshRow>, sqlx::Error> {
    sqlx::query_as::<_, RefreshRow>(
        "SELECT id, family_id, user_id, consumed_at, (expires_at <= now()) AS is_expired \
         FROM refresh_tokens WHERE token_hash = $1 FOR UPDATE",
    )
    .bind(token_hash)
    .fetch_optional(&mut **tx)
    .await
}

/// Mark a row consumed — the old half of a rotation. Call only while holding
/// the family lock.
pub(crate) async fn consume(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE refresh_tokens SET consumed_at = now() WHERE id = $1")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Family revocation = delete every row of the family (spec §2.2). Call only
/// while holding the family lock.
pub(crate) async fn revoke_family(
    tx: &mut Transaction<'_, Postgres>,
    family_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM refresh_tokens WHERE family_id = $1")
        .bind(family_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Revoke every family of one user (spec §4.2: password reset and disable
/// revoke explicitly; deletion cascades via the FK instead). All sessions of
/// the user end — other users' families are untouched.
///
/// Locks each family before deleting, ordered by `family_id` so two
/// concurrent multi-family revocations cannot deadlock, and so a rotation
/// already in flight is either deleted with its family or starts after us
/// and finds no row (see [`lock_family`]).
pub(crate) async fn revoke_user_families(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<(), sqlx::Error> {
    let family_ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT DISTINCT family_id FROM refresh_tokens \
         WHERE user_id = $1 ORDER BY family_id",
    )
    .bind(user_id)
    .fetch_all(&mut **tx)
    .await?;
    for family_id in family_ids {
        lock_family(tx, family_id).await?;
    }
    sqlx::query("DELETE FROM refresh_tokens WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
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
    fn rotation_keeps_the_family_and_changes_the_plaintext() {
        let first = generate_refresh();
        let next = generate_refresh_in_family(first.family_id);
        assert_eq!(next.family_id, first.family_id, "rotation stays in-family");
        assert_ne!(next.plaintext, first.plaintext);
        assert_eq!(next.plaintext.len(), 64);
    }

    #[test]
    fn cleared_cookie_expires_immediately_with_the_same_scope() {
        let wire = clear_refresh_cookie().to_string();
        assert!(wire.starts_with("rs_admin_refresh=;"), "{wire}");
        for attribute in [
            "Max-Age=0",
            "HttpOnly",
            "Secure",
            "SameSite=Strict",
            "Path=/api/auth",
        ] {
            assert!(wire.contains(attribute), "missing {attribute} in {wire}");
        }
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
