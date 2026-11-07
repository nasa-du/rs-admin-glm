//! Refresh rotation and logout (issue #16, spec §4.2 / ADR 0001).
//!
//! Both endpoints eat the refresh cookie and therefore also require the
//! `X-Requested-With` header as the CSRF second line of defense (the first is
//! `SameSite=Strict` on the cookie). Refresh implements RFC 9700 rotation:
//! consume the presented row, insert its successor in the same family, and
//! revoke the whole family when a consumed token reappears. Every refresh
//! re-reads the users row, so a deleted or disabled account is cut off at the
//! next rotation and a role change lands in the newly issued access token.
//!
//! All family mutations run under [`tokens::lock_family`] — see that function
//! for the concurrent-rotation race it closes.

use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::http::header::SET_COOKIE;
use axum::response::{IntoResponse, Response};
use axum_extra::extract::cookie::CookieJar;
use sqlx::{Postgres, Transaction};

use super::{SessionResponse, UserRow, jwt, tokens};
use crate::error::AppError;
use crate::state::AppState;

/// CSRF second line of defense (spec §4.1): refresh and logout require this
/// exact header, injected by the frontend axios instance.
const CSRF_HEADER: &str = "x-requested-with";
const CSRF_VALUE: &str = "XMLHttpRequest";

/// Reject a cookie-bearing call that lacks the exact CSRF header. Checked
/// before the cookie is even looked at, so a rejected call never rotates or
/// revokes anything.
fn require_csrf_header(headers: &HeaderMap) -> Result<(), AppError> {
    let valid = headers
        .get(CSRF_HEADER)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == CSRF_VALUE);
    if valid {
        Ok(())
    } else {
        Err(AppError::RefreshInvalid)
    }
}

/// Revoke the family, commit the revocation, and hand the refusal back to the
/// caller. An SQL failure supersedes the refusal (it is a 500, not a 401).
async fn revoke_and_fail(
    mut tx: Transaction<'_, Postgres>,
    family_id: uuid::Uuid,
    refusal: AppError,
) -> AppError {
    let result = match tokens::revoke_family(&mut tx, family_id).await {
        Ok(()) => tx.commit().await,
        Err(err) => Err(err),
    };
    match result {
        Ok(()) => refusal,
        Err(err) => AppError::Internal(err.into()),
    }
}

/// `POST /auth/refresh` (spec §4.2): cookie + CSRF header in,
/// `200 {accessToken, user}` + rotated Set-Cookie out.
#[utoipa::path(
    post,
    path = "/auth/refresh",
    tag = "auth",
    responses(
        (status = 200, description = "Access token reissued; rotates rs_admin_refresh",
         body = SessionResponse),
        (status = 401, description = "REFRESH_INVALID (missing cookie / unknown / expired / reused) or USER_DISABLED",
         body = crate::error::ErrorBody),
    )
)]
pub async fn refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Response, AppError> {
    require_csrf_header(&headers)?;

    let presented = jar
        .get(tokens::REFRESH_COOKIE_NAME)
        .map(|cookie| cookie.value().to_owned())
        .ok_or(AppError::RefreshInvalid)?;
    let token_hash = tokens::hash_token(&presented);

    let mut tx = state.pool.begin().await?;

    // Discover the family without a lock, then serialize on it before the
    // row lock: every rotation and revocation of this family queues here.
    let Some(family_id) = tokens::family_of_token(&mut tx, &token_hash).await? else {
        return Err(AppError::RefreshInvalid);
    };
    tokens::lock_family(&mut tx, family_id).await?;

    // Re-read under the family lock; a concurrent logout or revocation may
    // have removed the row while we waited.
    let Some(row) = tokens::find_for_update(&mut tx, &token_hash).await? else {
        return Err(AppError::RefreshInvalid);
    };

    // A consumed token reappearing is a leak signal: revoke the whole family
    // (RFC 9700 reuse detection).
    if row.consumed_at.is_some() {
        return Err(revoke_and_fail(tx, row.family_id, AppError::RefreshInvalid).await);
    }
    if row.is_expired {
        return Err(revoke_and_fail(tx, row.family_id, AppError::RefreshInvalid).await);
    }

    // Defensive depth (ADR 0001): the user row is re-read on every rotation.
    // Deleting a user already cascades their tokens away, but the explicit
    // checks keep the branch correct on their own.
    let user = sqlx::query_as::<_, UserRow>(
        "SELECT id, email, display_name, password_hash, role, is_enabled, \
                must_change_password, created_at, updated_at \
         FROM users WHERE id = $1",
    )
    .bind(row.user_id)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(user) = user else {
        return Err(revoke_and_fail(tx, row.family_id, AppError::RefreshInvalid).await);
    };
    if !user.is_enabled {
        return Err(revoke_and_fail(tx, row.family_id, AppError::UserDisabled).await);
    }

    let access = jwt::issue_access_now(user.id, &user.email, user.role, &state.jwt.encoding)
        .map_err(AppError::Internal)?;

    // Rotate: consume the presented row, insert its successor in the same
    // family with a fresh 7-day sliding expiry.
    let next = tokens::generate_refresh_in_family(row.family_id);
    tokens::consume(&mut tx, row.id).await?;
    tokens::insert_token(
        &mut *tx,
        user.id,
        row.family_id,
        &tokens::hash_token(&next.plaintext),
    )
    .await?;
    tx.commit().await?;

    let body = SessionResponse {
        access_token: access,
        user: user.into(),
    };
    Ok((
        [(
            SET_COOKIE,
            tokens::refresh_cookie(&next.plaintext).to_string(),
        )],
        Json(body),
    )
        .into_response())
}

/// `POST /auth/logout` (spec §4.2): revoke the presented cookie's family,
/// clear the cookie, `204`. No cookie (or an unknown one) is still `204` —
/// logout is idempotent. Access tokens are not blacklisted; they die
/// naturally.
#[utoipa::path(
    post,
    path = "/auth/logout",
    tag = "auth",
    responses(
        (status = 204, description = "Current token family revoked (when a cookie was presented); cookie cleared"),
        (status = 401, description = "REFRESH_INVALID — missing X-Requested-With header",
         body = crate::error::ErrorBody),
    )
)]
pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
    jar: CookieJar,
) -> Result<Response, AppError> {
    require_csrf_header(&headers)?;

    if let Some(cookie) = jar.get(tokens::REFRESH_COOKIE_NAME) {
        let token_hash = tokens::hash_token(cookie.value());
        let mut tx = state.pool.begin().await?;

        // Same family lock as refresh: a rotation already in flight finishes
        // first and its successor is deleted with the rest of the family (or
        // it starts after us and finds no token).
        if let Some(family_id) = tokens::family_of_token(&mut tx, &token_hash).await? {
            tokens::lock_family(&mut tx, family_id).await?;
            tokens::revoke_family(&mut tx, family_id).await?;
        }
        tx.commit().await?;
    }

    Ok((
        StatusCode::NO_CONTENT,
        [(SET_COOKIE, tokens::clear_refresh_cookie().to_string())],
    )
        .into_response())
}
