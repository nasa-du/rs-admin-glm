//! User lifecycle — update, delete, password reset (issue #18, spec §3 /
//! §5.3).
//!
//! Every mutation is one transaction with all guardrails evaluated before the
//! first write, so a refused operation never lands partially (护栏：触碰即
//! 整体拒绝). Evaluation order, most specific refusal first:
//!
//! 1. `404 NOT_FOUND` — the path target does not exist;
//! 2. `403 SELF_MANAGEMENT_FORBIDDEN` — the target is the caller
//!    (self delete / disable / role change; profile edits belong to
//!    `PATCH /users/me`, password changes to `PATCH /auth/password`);
//! 3. `409 LAST_SUPERADMIN` — the operation would leave zero enabled
//!    superadmins. Checked inside the transaction under an advisory lock,
//!    and before manage-down: the invariant matters even when manage-down
//!    would also refuse, which is what makes the §5.3 `409` rows observable;
//! 4. `403 FORBIDDEN` — manage-down: the target's current role (and, on
//!    update, the requested new role) must rank strictly below the caller's;
//!    role changes are superadmin-only;
//! 5. `422 VALIDATION_ERROR` — field-level payload validation.
//!
//! Side effects on success (spec §4.2): disabling a user revokes all their
//! token families; a password reset does the same and re-arms
//! `must_change_password` while leaving `is_enabled` alone (重置 ≠ 解禁);
//! deletion cascades the families away via the foreign key.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum_extra::extract::WithRejection;
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Transaction};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::auth::extractor::{AdminUser, AuthUser, SuperadminUser};
use crate::auth::tokens;
use crate::auth::{Role, UserDto, UserRow};
use crate::error::AppError;
use crate::password;
use crate::state::AppState;

use super::field_error;

/// `PATCH /users/:id` request body (spec §5.3). Every field is optional;
/// absent fields keep their current value. `role` is a string rather than a
/// [`Role`] so an unknown value becomes a field-level `ROLE_INVALID` 422.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUserRequest {
    pub display_name: Option<String>,
    pub role: Option<String>,
    pub is_enabled: Option<bool>,
}

/// `200` body of `PATCH /users/:id` (spec §5.3).
#[derive(Debug, Serialize, ToSchema)]
pub struct UpdateUserResponse {
    pub user: UserDto,
}

/// `200` body of `POST /users/:id/password-reset` (spec §5.3): the new
/// one-time initial password, shown to the operator exactly once.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResetPasswordResponse {
    pub initial_password: String,
}

/// Advisory-lock key serializing last-superadmin evaluations: two concurrent
/// operations that each remove one of the last two enabled superadmins must
/// not both pass the survivor count. A `hashtext` collision can only
/// over-serialize, never under-lock.
const LAST_SUPERADMIN_LOCK: &str = "users:last-superadmin";

/// The target row, locked for the coming mutation (404 when absent).
async fn fetch_target_for_update(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<UserRow, AppError> {
    sqlx::query_as::<_, UserRow>(
        "SELECT id, email, display_name, password_hash, role, is_enabled, \
                must_change_password, created_at, updated_at \
         FROM users WHERE id = $1 FOR UPDATE",
    )
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(AppError::NotFound)
}

/// Guardrail 1 (spec §3): no deleting, disabling, role-changing — in short,
/// no managing — one's own account.
fn ensure_not_self(auth: &AuthUser, target: &UserRow) -> Result<(), AppError> {
    if auth.id == target.id {
        Err(AppError::SelfManagementForbidden)
    } else {
        Ok(())
    }
}

/// Guardrail 2 (spec §3): the system must always keep at least one enabled
/// superadmin. `target_leaves_set` says the operation removes the target from
/// the enabled-superadmin set — a demotion or disable for update, always for
/// delete.
async fn ensure_superadmin_survives(
    tx: &mut Transaction<'_, Postgres>,
    target: &UserRow,
    target_leaves_set: bool,
) -> Result<(), AppError> {
    if !(target_leaves_set && target.role == Role::Superadmin && target.is_enabled) {
        return Ok(());
    }
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(LAST_SUPERADMIN_LOCK)
        .execute(&mut **tx)
        .await?;
    let survivors: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM users \
         WHERE role = 'superadmin' AND is_enabled AND id <> $1",
    )
    .bind(target.id)
    .fetch_one(&mut **tx)
    .await?;
    if survivors == 0 {
        Err(AppError::LastSuperadmin)
    } else {
        Ok(())
    }
}

/// Manage-down (spec §3): the caller may act only on roles strictly below
/// their own — peers and superiors are read-only.
fn ensure_manage_down(auth: &AuthUser, target_role: Role) -> Result<(), AppError> {
    if auth.role.strictly_outranks(target_role) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

/// Validate an optional display name: trim, then 1–32 characters by count
/// (spec §2.1). `None` passes through — absent means "keep".
fn validated_display_name(raw: &Option<String>) -> Result<Option<String>, AppError> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    let len = trimmed.chars().count();
    if len == 0 {
        Err(field_error("displayName", "DISPLAY_NAME_REQUIRED"))
    } else if len > 32 {
        Err(field_error("displayName", "DISPLAY_NAME_TOO_LONG"))
    } else {
        Ok(Some(trimmed.to_owned()))
    }
}

/// `PATCH /users/:id` (spec §5.3): display name, role (superadmin only) and
/// enabled flag, manage-down, all-or-nothing.
#[utoipa::path(
    patch,
    path = "/users/{id}",
    tag = "users",
    params(
        ("id" = Uuid, Path, description = "Target user id"),
    ),
    request_body = UpdateUserRequest,
    responses(
        (status = 200, description = "Updated; the full user row", body = UpdateUserResponse),
        (status = 401, description = "Missing or invalid access token", body = crate::error::ErrorBody),
        (status = 403, description = "FORBIDDEN (below admin / manage-down / role change not by superadmin) \
         or SELF_MANAGEMENT_FORBIDDEN (target is the caller)", body = crate::error::ErrorBody),
        (status = 404, description = "NOT_FOUND — no user with this id", body = crate::error::ErrorBody),
        (status = 409, description = "LAST_SUPERADMIN — would leave zero enabled superadmins", body = crate::error::ErrorBody),
        (status = 422, description = "VALIDATION_ERROR — displayName / role", body = crate::error::ErrorBody),
    )
)]
pub async fn update(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(user_id): Path<Uuid>,
    WithRejection(Json(body), _): WithRejection<Json<UpdateUserRequest>, AppError>,
) -> Result<Json<UpdateUserResponse>, AppError> {
    let auth = admin.0;
    // Parse the role first, like create: an unparseable role is a malformed
    // request (422 ROLE_INVALID), not an authorization decision.
    let new_role = match body.role.as_deref() {
        None => None,
        Some(wire) => {
            Some(Role::from_wire(wire).ok_or_else(|| field_error("role", "ROLE_INVALID"))?)
        }
    };

    let mut tx = state.pool.begin().await?;
    let target = fetch_target_for_update(&mut tx, user_id).await?;
    ensure_not_self(&auth, &target)?;

    let effective_role = new_role.unwrap_or(target.role);
    let effective_enabled = body.is_enabled.unwrap_or(target.is_enabled);
    let leaves_set = effective_role != Role::Superadmin || !effective_enabled;
    ensure_superadmin_survives(&mut tx, &target, leaves_set).await?;

    ensure_manage_down(&auth, target.role)?;
    if new_role.is_some() {
        // Role assignment is superadmin-exclusive (spec §3), and the new role
        // must also sit strictly below the caller — no promoting to peers.
        if auth.role != Role::Superadmin || !auth.role.strictly_outranks(effective_role) {
            return Err(AppError::Forbidden);
        }
    }
    let display_name = validated_display_name(&body.display_name)?;

    // Disabling ends every session of the target now (spec §4.2); enabling
    // and role changes do not — the next refresh re-reads the row instead.
    if target.is_enabled && !effective_enabled {
        tokens::revoke_user_families(&mut tx, target.id).await?;
    }

    let user = sqlx::query_as::<_, UserDto>(
        "UPDATE users \
         SET display_name = COALESCE($2, display_name), \
             role = COALESCE($3, role), \
             is_enabled = COALESCE($4, is_enabled), \
             updated_at = now() \
         WHERE id = $1 \
         RETURNING id, email, display_name, role, is_enabled, must_change_password, \
                   created_at, updated_at",
    )
    .bind(target.id)
    .bind(display_name)
    .bind(new_role)
    .bind(body.is_enabled)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Json(UpdateUserResponse { user }))
}

/// `DELETE /users/:id` (spec §5.3): superadmin-only hard delete. The users
/// row goes away and the foreign key cascades every token family with it —
/// no soft delete, no tombstone (spec §2.1).
#[utoipa::path(
    delete,
    path = "/users/{id}",
    tag = "users",
    params(
        ("id" = Uuid, Path, description = "Target user id"),
    ),
    responses(
        (status = 204, description = "Hard-deleted; sessions cascaded away"),
        (status = 401, description = "Missing or invalid access token", body = crate::error::ErrorBody),
        (status = 403, description = "FORBIDDEN — caller is not a superadmin or manages up; \
         SELF_MANAGEMENT_FORBIDDEN — the target is the caller", body = crate::error::ErrorBody),
        (status = 404, description = "NOT_FOUND — no user with this id", body = crate::error::ErrorBody),
        (status = 409, description = "LAST_SUPERADMIN — would leave zero enabled superadmins", body = crate::error::ErrorBody),
    )
)]
pub async fn delete(
    State(state): State<AppState>,
    superadmin: SuperadminUser,
    Path(user_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let auth = superadmin.0;

    let mut tx = state.pool.begin().await?;
    let target = fetch_target_for_update(&mut tx, user_id).await?;
    ensure_not_self(&auth, &target)?;
    ensure_superadmin_survives(&mut tx, &target, true).await?;
    ensure_manage_down(&auth, target.role)?;

    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(target.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}

/// `POST /users/:id/password-reset` (spec §5.3): issue a fresh one-time
/// initial password, re-arm `must_change_password`, revoke every token family
/// of the target, and deliberately leave `is_enabled` alone — resetting a
/// password is not un-disabling an account (重置 ≠ 解禁).
#[utoipa::path(
    post,
    path = "/users/{id}/password-reset",
    tag = "users",
    params(
        ("id" = Uuid, Path, description = "Target user id"),
    ),
    responses(
        (status = 200, description = "`initialPassword` is shown only once", body = ResetPasswordResponse),
        (status = 401, description = "Missing or invalid access token", body = crate::error::ErrorBody),
        (status = 403, description = "FORBIDDEN — below admin / manage-down; \
         SELF_MANAGEMENT_FORBIDDEN — the target is the caller", body = crate::error::ErrorBody),
        (status = 404, description = "NOT_FOUND — no user with this id", body = crate::error::ErrorBody),
    )
)]
pub async fn reset_password(
    State(state): State<AppState>,
    admin: AdminUser,
    Path(user_id): Path<Uuid>,
) -> Result<Json<ResetPasswordResponse>, AppError> {
    let auth = admin.0;

    let mut tx = state.pool.begin().await?;
    let target = fetch_target_for_update(&mut tx, user_id).await?;
    ensure_not_self(&auth, &target)?;
    ensure_manage_down(&auth, target.role)?;

    let initial_password = password::generate_initial_password();
    let password_hash = password::hash_password(&initial_password).map_err(AppError::Internal)?;
    sqlx::query(
        "UPDATE users \
         SET password_hash = $2, must_change_password = TRUE, updated_at = now() \
         WHERE id = $1",
    )
    .bind(target.id)
    .bind(&password_hash)
    .execute(&mut *tx)
    .await?;
    tokens::revoke_user_families(&mut tx, target.id).await?;
    tx.commit().await?;

    Ok(Json(ResetPasswordResponse { initial_password }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name_validation_mirrors_create() {
        assert_eq!(
            validated_display_name(&None).unwrap(),
            None,
            "absent keeps the current value"
        );
        assert_eq!(
            validated_display_name(&Some("  Name  ".to_owned())).unwrap(),
            Some("Name".to_owned()),
            "present values are trimmed"
        );
        assert_eq!(
            validated_display_name(&Some("é".repeat(32)))
                .unwrap()
                .expect("valid name")
                .chars()
                .count(),
            32,
            "the cap counts characters, not bytes"
        );

        for (raw, code) in [
            ("   ", "DISPLAY_NAME_REQUIRED"),
            (&"x".repeat(33), "DISPLAY_NAME_TOO_LONG"),
        ] {
            let err = validated_display_name(&Some(raw.to_owned())).unwrap_err();
            let AppError::ValidationError(errors) = err else {
                panic!("expected a validation error, got {err:?}");
            };
            assert_eq!(errors["displayName"], vec![code]);
        }
    }
}
