//! auth module — the login endpoint and its token machinery (spec §4).
//!
//! Issue #15 lands `POST /auth/login`; refresh / logout / password follow in
//! their own tickets. Login is the tracer bullet: argon2 verification, an
//! HS256 access JWT (15 min), a fresh token family, and the
//! `rs_admin_refresh` cookie (ADR 0001).

pub mod jwt;
pub mod tokens;

use std::sync::LazyLock;

use axum::Json;
use axum::extract::State;
use axum::http::header::SET_COOKIE;
use axum::response::{IntoResponse, Response};
use axum_extra::extract::WithRejection;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::PgPool;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::AppError;
use crate::password;
use crate::state::AppState;

/// The hierarchical single-value role (spec §3): superadmin ⊃ admin ⊃ user.
/// The wire spelling lives once, in `serde(rename_all)` — the same derive
/// serves DTOs, the JWT `role` claim, and (via `sqlx::Type`) the DB enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, ToSchema)]
#[sqlx(type_name = "user_role", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Superadmin,
    Admin,
    User,
}

/// `POST /auth/login` request body.
#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

/// The `User` wire shape (spec §5.1) — camelCase, never leaks the hash.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserDto {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub role: Role,
    pub is_enabled: bool,
    pub must_change_password: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// `POST /auth/login` success body: `200 {accessToken, user}` + Set-Cookie.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginResponse {
    pub access_token: String,
    pub user: UserDto,
}

/// A `users` row as login reads it.
#[derive(Debug, FromRow)]
struct UserRow {
    id: Uuid,
    email: String,
    display_name: String,
    password_hash: String,
    role: Role,
    is_enabled: bool,
    must_change_password: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<UserRow> for UserDto {
    fn from(row: UserRow) -> Self {
        Self {
            id: row.id,
            email: row.email,
            display_name: row.display_name,
            role: row.role,
            is_enabled: row.is_enabled,
            must_change_password: row.must_change_password,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// A pre-computed hash the unknown-email path verifies against, so response
/// timing matches the wrong-password path (§4.2 anti-enumeration).
static DUMMY_HASH: LazyLock<String> = LazyLock::new(|| {
    password::hash_password("timing-equalizer").expect("hashing a constant cannot fail")
});

/// `POST /auth/login` (spec §4.2): verify password → distinguish the two 401s
/// by error code → sign access JWT → open a token family → refresh cookie.
///
/// No field pre-validation on login: any unusable credential — unknown email,
/// wrong password, a password outside the creation-time 8–128 policy — is
/// simply a 401 `INVALID_CREDENTIALS` (issue #15 AC2). The only 422 here is
/// an unparseable request body, which cannot even be verified.
#[utoipa::path(
    post,
    path = "/auth/login",
    tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Signed in; sets the rs_admin_refresh HttpOnly cookie",
         body = LoginResponse),
        (status = 401, description = "INVALID_CREDENTIALS (unknown email or wrong password) or USER_DISABLED",
         body = crate::error::ErrorBody),
        (status = 422, description = "VALIDATION_ERROR — request body is not parseable",
         body = crate::error::ErrorBody),
    )
)]
pub async fn login(
    State(state): State<AppState>,
    WithRejection(Json(request), _): WithRejection<Json<LoginRequest>, AppError>,
) -> Result<Response, AppError> {
    // Unknown login identifier: burn one argon2 verification against a dummy
    // hash so timing matches the wrong-password path, then answer with the
    // same code — enumeration stays blind (§4.2).
    let Some(user) = fetch_user_by_email(&state.pool, &request.email).await? else {
        let _ = password::verify_password(&request.password, &DUMMY_HASH);
        return Err(AppError::InvalidCredentials);
    };

    // Password first, enabled second: a wrong password on a disabled account
    // must still read as INVALID_CREDENTIALS (§4.2).
    let verified = password::verify_password(&request.password, &user.password_hash)
        .map_err(AppError::Internal)?;
    if !verified {
        return Err(AppError::InvalidCredentials);
    }
    if !user.is_enabled {
        return Err(AppError::UserDisabled);
    }

    let access = jwt::issue_access_now(user.id, &user.email, user.role, &state.jwt.encoding)
        .map_err(AppError::Internal)?;

    let refresh = tokens::generate_refresh();
    tokens::insert_family_root(
        &state.pool,
        user.id,
        refresh.family_id,
        &tokens::hash_token(&refresh.plaintext),
    )
    .await?;

    let body = LoginResponse {
        access_token: access,
        user: user.into(),
    };
    Ok((
        [(
            SET_COOKIE,
            tokens::refresh_cookie(&refresh.plaintext).to_string(),
        )],
        Json(body),
    )
        .into_response())
}

/// Fetch a `users` row by login identifier (exact match).
async fn fetch_user_by_email(pool: &PgPool, email: &str) -> Result<Option<UserRow>, sqlx::Error> {
    sqlx::query_as::<_, UserRow>(
        "SELECT id, email, display_name, password_hash, role, is_enabled, \
                must_change_password, created_at, updated_at \
         FROM users WHERE email = $1",
    )
    .bind(email)
    .fetch_optional(pool)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The role wire spelling is a contract (spec §5.1 / §4.1): DTO JSON and
    /// the JWT `role` claim must be exactly these strings.
    #[test]
    fn roles_serialize_to_their_spec_spelling() {
        for (role, wire) in [
            (Role::Superadmin, "superadmin"),
            (Role::Admin, "admin"),
            (Role::User, "user"),
        ] {
            assert_eq!(serde_json::to_value(role).unwrap(), wire);
            assert_eq!(serde_json::from_value::<Role>(wire.into()).unwrap(), role);
        }
    }
}
