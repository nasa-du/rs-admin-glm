//! Password change (issue #19, spec §4.2 / §5.3).
//!
//! `PATCH /auth/password` closes the first-login loop: the one-time initial
//! password (CONTEXT.md 初始密码) fulfills its purpose here and dies with it.
//! A wrong current password is a field-level 422, deliberately not a 401 —
//! the frontend's 401 interceptor would answer a perfectly valid session
//! with a pointless refresh + replay, and the user would see the same
//! complaint twice.
//!
//! Success is all-or-nothing in one transaction: the new hash lands,
//! `must_change_password` clears, and **every** token family of the user —
//! including the browser asking — is revoked (会话：改密终止全部会话). The
//! response clears the refresh cookie so this browser does not carry a dead
//! one into its next sign-in, and the frontend routes back to sign-in.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::header::SET_COOKIE;
use axum::response::{IntoResponse, Response};
use axum_extra::extract::WithRejection;
use serde::Deserialize;
use utoipa::ToSchema;

use super::UserRow;
use super::extractor::AuthUser;
use super::tokens;
use crate::error::{AppError, FieldErrors};
use crate::password::{PASSWORD_MAX_CHARS, PASSWORD_MIN_CHARS};
use crate::password::{hash_password, verify_password};
use crate::state::AppState;

/// `PATCH /auth/password` request body (spec §5.3).
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

/// `PATCH /auth/password` (spec §4.2): verify the current password → rewrite
/// the hash, clear `must_change_password`, revoke every family → `204`.
#[utoipa::path(
    patch,
    path = "/auth/password",
    tag = "auth",
    request_body = ChangePasswordRequest,
    responses(
        (status = 204, description = "Password rewritten, every token family revoked, refresh cookie cleared"),
        (status = 401, description = "Missing or invalid access token", body = crate::error::ErrorBody),
        (status = 404, description = "NOT_FOUND — the caller's account no longer exists", body = crate::error::ErrorBody),
        (status = 422, description = "VALIDATION_ERROR — currentPassword (CURRENT_PASSWORD_INCORRECT) / newPassword (PASSWORD_TOO_SHORT | PASSWORD_TOO_LONG)",
         body = crate::error::ErrorBody),
    )
)]
pub async fn change_password(
    State(state): State<AppState>,
    auth: AuthUser,
    WithRejection(Json(body), _): WithRejection<Json<ChangePasswordRequest>, AppError>,
) -> Result<Response, AppError> {
    // Both fields are checked before anything is written, so a request with a
    // wrong current password *and* a policy-violating new password reports
    // both codes in one 422.
    let mut errors = FieldErrors::new();
    let new_len = body.new_password.chars().count();
    if new_len < PASSWORD_MIN_CHARS {
        errors
            .entry("newPassword".to_owned())
            .or_default()
            .push("PASSWORD_TOO_SHORT".to_owned());
    } else if new_len > PASSWORD_MAX_CHARS {
        errors
            .entry("newPassword".to_owned())
            .or_default()
            .push("PASSWORD_TOO_LONG".to_owned());
    }

    let mut tx = state.pool.begin().await?;
    // The row lock serializes a concurrent change of the same password; the
    // argon2 verify below reads the hash it protects.
    let user = sqlx::query_as::<_, UserRow>(
        "SELECT id, email, display_name, password_hash, role, is_enabled, \
                must_change_password, created_at, updated_at \
         FROM users WHERE id = $1 FOR UPDATE",
    )
    .bind(auth.id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;

    let verified =
        verify_password(&body.current_password, &user.password_hash).map_err(AppError::Internal)?;
    if !verified {
        errors
            .entry("currentPassword".to_owned())
            .or_default()
            .push("CURRENT_PASSWORD_INCORRECT".to_owned());
    }

    if !errors.is_empty() {
        // Dropping the transaction rolls back: a refusal never touches the
        // hash, the flag, or a single family (护栏：整体拒绝).
        return Err(AppError::ValidationError(errors));
    }

    let new_hash = hash_password(&body.new_password).map_err(AppError::Internal)?;
    sqlx::query(
        "UPDATE users \
         SET password_hash = $2, must_change_password = FALSE, updated_at = now() \
         WHERE id = $1",
    )
    .bind(auth.id)
    .bind(&new_hash)
    .execute(&mut *tx)
    .await?;
    tokens::revoke_user_families(&mut tx, auth.id).await?;
    tx.commit().await?;

    Ok((
        StatusCode::NO_CONTENT,
        [(SET_COOKIE, tokens::clear_refresh_cookie().to_string())],
    )
        .into_response())
}
