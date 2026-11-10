//! Self-service profile (issue #19, spec §5.3): `PATCH /users/me`.
//!
//! The self endpoint exists so profile edits never entangle with manage-down:
//! no target id, no guardrails, no role arithmetic — every role may call it,
//! and the only writable field is the display name. A body carrying anything
//! else is not an error; the extra keys simply have nowhere to land (serde
//! ignores unknown fields), which is what "仅可改显示名" means on the wire.

use axum::Json;
use axum::extract::State;
use axum_extra::extract::WithRejection;
use serde::Deserialize;
use utoipa::ToSchema;

use crate::auth::UserDto;
use crate::auth::extractor::AuthUser;
use crate::error::AppError;
use crate::state::AppState;

use super::manage::{UpdateUserResponse, validated_display_name};

/// `PATCH /users/me` request body (spec §5.3). `displayName` is the one field
/// the endpoint accepts.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMeRequest {
    pub display_name: String,
}

/// `PATCH /users/me` (spec §5.3): the caller's own display name, nothing else.
/// A profile edit is not a password change — sessions are left alone.
#[utoipa::path(
    patch,
    path = "/users/me",
    tag = "users",
    request_body = UpdateMeRequest,
    responses(
        (status = 200, description = "Updated; the full user row", body = UpdateUserResponse),
        (status = 401, description = "Missing or invalid access token", body = crate::error::ErrorBody),
        (status = 422, description = "VALIDATION_ERROR — displayName (DISPLAY_NAME_REQUIRED | DISPLAY_NAME_TOO_LONG)",
         body = crate::error::ErrorBody),
    )
)]
pub async fn update_me(
    State(state): State<AppState>,
    auth: AuthUser,
    WithRejection(Json(body), _): WithRejection<Json<UpdateMeRequest>, AppError>,
) -> Result<Json<UpdateUserResponse>, AppError> {
    let display_name = validated_display_name(&body.display_name)?;

    let user = sqlx::query_as::<_, UserDto>(
        "UPDATE users \
         SET display_name = $2, updated_at = now() \
         WHERE id = $1 \
         RETURNING id, email, display_name, role, is_enabled, must_change_password, \
                   created_at, updated_at",
    )
    .bind(auth.id)
    .bind(display_name)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(UpdateUserResponse { user }))
}
