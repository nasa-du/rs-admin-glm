//! `AuthUser` — the access-token extractor for business endpoints (spec §3).
//!
//! The JWT carries the identity and the single-value role; this extractor
//! verifies the signature and hands the claims to the handler. No database
//! lookup happens per request: role changes take effect at the next refresh,
//! which is the ≤15-minute propagation bound of ADR 0001.
//!
//! A missing, malformed, expired, or wrongly-signed token is one 401. The
//! §5.2 code list has no dedicated access-token code; `INVALID_CREDENTIALS`
//! is the generic authentication failure and is what the frontend's 401
//! interceptor treats as "refresh and replay". The body never says which of
//! those failed.

use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use jsonwebtoken::{Algorithm, Validation, decode};
use uuid::Uuid;

use super::Role;
use super::jwt::AccessClaims;
use crate::error::AppError;
use crate::state::AppState;

/// The authenticated caller, reconstructed from the access-token claims.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthUser {
    /// `users.id` (= JWT `sub`).
    pub id: Uuid,
    /// Login identifier carried by the token.
    pub email: String,
    /// Role as of token issuance.
    pub role: Role,
}

impl AuthUser {
    /// Route-level role gate: 403 when the caller's claim is below `minimum`.
    pub fn require_at_least(&self, minimum: Role) -> Result<(), AppError> {
        if self.role.at_least(minimum) {
            Ok(())
        } else {
            Err(AppError::Forbidden)
        }
    }
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .ok_or(AppError::InvalidCredentials)?;

        // jsonwebtoken's default Validation checks `exp` (60 s leeway) and
        // pins HS256; no audience/issuer is issued, so none is required.
        let token_data = decode::<AccessClaims>(
            token,
            &state.jwt.decoding,
            &Validation::new(Algorithm::HS256),
        )
        .map_err(|_| AppError::InvalidCredentials)?;

        let id =
            Uuid::parse_str(&token_data.claims.sub).map_err(|_| AppError::InvalidCredentials)?;

        Ok(Self {
            id,
            email: token_data.claims.email,
            role: token_data.claims.role,
        })
    }
}

/// Admin-or-above route gate (spec §3). Checking inside an extractor makes
/// the role gate run before body / query parsing: a caller below admin gets
/// 403 even when the payload is also malformed. Handlers that need the
/// caller use the wrapped [`AuthUser`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminUser(pub AuthUser);

impl FromRequestParts<AppState> for AdminUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = AuthUser::from_request_parts(parts, state).await?;
        user.require_at_least(Role::Admin)?;
        Ok(Self(user))
    }
}

/// Superadmin-only route gate (spec §3: deletion is superadmin-exclusive).
/// Role assignment is enforced field-conditionally inside the update handler
/// instead (a PATCH without `role` must not need the gate). Same ordering
/// property as [`AdminUser`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuperadminUser(pub AuthUser);

impl FromRequestParts<AppState> for SuperadminUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user = AuthUser::from_request_parts(parts, state).await?;
        user.require_at_least(Role::Superadmin)?;
        Ok(Self(user))
    }
}
