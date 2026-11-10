//! users module — list, create, and the lifecycle closure of update / delete
//! / password-reset (issues #17–#18) plus the self-service `me` endpoint
//! (issue #19); spec §3 / §5.3 / §5.4.
//!
//! `GET /users` is the admin+ server-side list: pagination, sort whitelist,
//! role / status filters, and fuzzy search over login identifier and display
//! name. `POST /users` is manage-down creation: the new role must be
//! strictly below the caller's, so no API call can mint a superadmin. The
//! `manage` submodule closes the lifecycle with the two spec §3 guardrails;
//! `me` is the no-entanglement self profile (displayName only).
//!
//! Authorization reads the role from the JWT claim via [`AuthUser`]; there
//! is no per-request users lookup (spec §3). The created initial password is
//! generated here, returned once in the 201 body, and stored only as its
//! argon2 PHC hash.

pub mod manage;
pub mod me;
pub mod query;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum_extra::extract::Query;
use axum_extra::extract::WithRejection;
use serde::{Deserialize, Serialize};
use sqlx::AssertSqlSafe;
use utoipa::ToSchema;

use crate::auth::extractor::AdminUser;
use crate::auth::{Role, UserDto};
use crate::error::{AppError, FieldErrors};
use crate::password;
use crate::state::AppState;
use query::{ListQuery, ValidatedList};

/// `POST /users` request body (spec §5.3). `role` is a string rather than a
/// [`Role`] so an unknown value becomes a field-level `ROLE_INVALID` 422.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserRequest {
    pub email: String,
    pub display_name: String,
    pub role: String,
}

/// `201` body: the created user plus the one-time initial password (§2.4).
/// The password is generated server-side and never readable again.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserResponse {
    pub user: UserDto,
    pub initial_password: String,
}

/// `GET /users` success body (spec §5.4).
#[derive(Debug, Serialize, ToSchema)]
pub struct UserListResponse {
    pub data: Vec<UserDto>,
    pub meta: ListMeta,
}

/// Pagination metadata (spec §5.4).
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListMeta {
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

/// `GET /users` (spec §5.3/§5.4): admin+ only.
#[utoipa::path(
    get,
    path = "/users",
    tag = "users",
    params(ListQuery),
    responses(
        (status = 200, description = "Paginated user list: `{data, meta}`", body = UserListResponse),
        (status = 401, description = "Missing or invalid access token", body = crate::error::ErrorBody),
        (status = 403, description = "FORBIDDEN — caller is below admin", body = crate::error::ErrorBody),
        (status = 422, description = "VALIDATION_ERROR — page / pageSize / sort / query value", body = crate::error::ErrorBody),
    )
)]
pub async fn list(
    State(state): State<AppState>,
    _admin: AdminUser,
    WithRejection(Query(raw), _): WithRejection<Query<ListQuery>, AppError>,
) -> Result<Json<UserListResponse>, AppError> {
    let filter = raw.validate()?;
    let (data, total) = fetch_page(&state, &filter).await?;

    Ok(Json(UserListResponse {
        data,
        meta: ListMeta {
            total,
            page: filter.page,
            page_size: filter.page_size,
        },
    }))
}

fn filter_clause(filter: &ValidatedList) -> (String, usize) {
    let mut clause = String::from(" WHERE TRUE");
    let mut next = 1;

    if !filter.roles.is_empty() {
        clause.push_str(&format!(" AND role = ANY(${next})"));
        next += 1;
    }
    if filter.search_pattern.is_some() {
        // ESCAPE '\' makes the escaped `%` / `_` literal (spec §5.4).
        clause.push_str(&format!(
            " AND (email ILIKE ${next} ESCAPE '\\' OR display_name ILIKE ${next} ESCAPE '\\')"
        ));
        next += 1;
    }
    if filter.status.is_some() {
        clause.push_str(&format!(" AND is_enabled = ${next}"));
        next += 1;
    }

    (clause, next)
}

/// Run the count + page queries over the same filters.
async fn fetch_page(
    state: &AppState,
    filter: &ValidatedList,
) -> Result<(Vec<UserDto>, i64), AppError> {
    let (where_sql, next) = filter_clause(filter);
    // `Role` derives sqlx::Type for the `user_role` enum and PgHasArrayType,
    // so a Vec<Role> binds directly as `user_role[]`.
    let roles = &filter.roles;

    // Sort identifiers come from the SortColumn enum, never the query
    // string; AssertSqlSafe covers only the numeric placeholder stitching.
    let list_sql = format!(
        "SELECT id, email, display_name, role, is_enabled, must_change_password, \
                created_at, updated_at \
         FROM users{where_sql} \
         ORDER BY {} {}, id ASC \
         LIMIT ${next} OFFSET ${}",
        filter.sort.column.sql(),
        filter.sort.direction_sql(),
        next + 1
    );
    let mut list = sqlx::query_as::<_, UserDto>(AssertSqlSafe(list_sql));
    if !roles.is_empty() {
        list = list.bind(roles.clone());
    }
    if let Some(pattern) = &filter.search_pattern {
        list = list.bind(pattern);
    }
    if let Some(enabled) = filter.status {
        list = list.bind(enabled);
    }
    let data = list
        .bind(filter.page_size)
        .bind(filter.offset())
        .fetch_all(&state.pool)
        .await?;

    let count_sql = format!("SELECT count(*) FROM users{where_sql}");
    let mut count = sqlx::query_scalar::<_, i64>(AssertSqlSafe(count_sql));
    if !roles.is_empty() {
        count = count.bind(roles);
    }
    if let Some(pattern) = &filter.search_pattern {
        count = count.bind(pattern);
    }
    if let Some(enabled) = filter.status {
        count = count.bind(enabled);
    }
    let total = count.fetch_one(&state.pool).await?;

    Ok((data, total))
}

/// `POST /users` (spec §5.3): admin+, manage-down. Returns the generated
/// initial password exactly once.
#[utoipa::path(
    post,
    path = "/users",
    tag = "users",
    request_body = CreateUserRequest,
    responses(
        (status = 201, description = "Created; `initialPassword` is shown only once", body = CreateUserResponse),
        (status = 401, description = "Missing or invalid access token", body = crate::error::ErrorBody),
        (status = 403, description = "FORBIDDEN — caller below admin, or new role not strictly below the caller", body = crate::error::ErrorBody),
        (status = 409, description = "EMAIL_TAKEN", body = crate::error::ErrorBody),
        (status = 422, description = "VALIDATION_ERROR — email / displayName / role", body = crate::error::ErrorBody),
    )
)]
pub async fn create(
    State(state): State<AppState>,
    admin: AdminUser,
    WithRejection(Json(body), _): WithRejection<Json<CreateUserRequest>, AppError>,
) -> Result<(StatusCode, Json<CreateUserResponse>), AppError> {
    let auth = admin.0;
    let role = Role::from_wire(&body.role).ok_or_else(|| field_error("role", "ROLE_INVALID"))?;
    // Manage-down: the new role must be strictly below the caller. This also
    // means even a superadmin cannot create another superadmin through the
    // API — the seed CLI is the only source of a second superadmin (§3).
    if !auth.role.strictly_outranks(role) {
        return Err(AppError::Forbidden);
    }

    let (email, display_name) = validate_profile(&body)?;

    let initial_password = password::generate_initial_password();
    let password_hash = password::hash_password(&initial_password).map_err(AppError::Internal)?;

    let user = sqlx::query_as::<_, UserDto>(
        "INSERT INTO users (email, display_name, password_hash, role, is_enabled, \
                must_change_password) \
         VALUES ($1, $2, $3, $4, TRUE, TRUE) \
         RETURNING id, email, display_name, role, is_enabled, must_change_password, \
                   created_at, updated_at",
    )
    .bind(&email)
    .bind(&display_name)
    .bind(&password_hash)
    .bind(role)
    .fetch_one(&state.pool)
    .await;

    match user {
        Ok(user) => Ok((
            StatusCode::CREATED,
            Json(CreateUserResponse {
                user,
                initial_password,
            }),
        )),
        Err(err) if is_users_email_unique_violation(&err) => Err(AppError::EmailTaken),
        Err(err) => Err(err.into()),
    }
}

pub(crate) fn field_error(field: &str, code: &str) -> AppError {
    AppError::ValidationError(FieldErrors::from([(
        field.to_owned(),
        vec![code.to_owned()],
    )]))
}

/// Trim and validate the profile fields (spec §2.1 / §5.2).
fn validate_profile(body: &CreateUserRequest) -> Result<(String, String), AppError> {
    let mut errors = FieldErrors::new();

    let email = body.email.trim().to_owned();
    if !is_valid_email(&email) {
        errors
            .entry("email".to_owned())
            .or_default()
            .push("EMAIL_INVALID".to_owned());
    }

    let display_name = body.display_name.trim().to_owned();
    let display_name_len = display_name.chars().count();
    if display_name_len == 0 {
        errors
            .entry("displayName".to_owned())
            .or_default()
            .push("DISPLAY_NAME_REQUIRED".to_owned());
    } else if display_name_len > 32 {
        errors
            .entry("displayName".to_owned())
            .or_default()
            .push("DISPLAY_NAME_TOO_LONG".to_owned());
    }

    if errors.is_empty() {
        Ok((email, display_name))
    } else {
        Err(AppError::ValidationError(errors))
    }
}

/// Application-level email sanity check (spec §2.1: format in the app
/// layer, uniqueness in the DB). Deliberately small: one `@`, a non-empty
/// local part, and a dotted domain; the login identifier is not a mail
/// delivery address.
fn is_valid_email(email: &str) -> bool {
    if email.chars().any(char::is_whitespace) {
        return false;
    }
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && !domain.contains('@')
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
}

fn is_users_email_unique_violation(err: &sqlx::Error) -> bool {
    err.as_database_error()
        .is_some_and(|db| db.is_unique_violation() && db.constraint() == Some("users_email_key"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(email: &str, display_name: &str, role: &str) -> CreateUserRequest {
        CreateUserRequest {
            email: email.to_owned(),
            display_name: display_name.to_owned(),
            role: role.to_owned(),
        }
    }

    fn field_codes(err: AppError) -> FieldErrors {
        match err {
            AppError::ValidationError(errors) => errors,
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[test]
    fn valid_email_sanity() {
        for email in ["admin@example.com", "a.b+tag@sub.example.co", "x@y.z"] {
            assert!(is_valid_email(email), "should accept {email}");
        }
        for email in [
            "",
            "no-at-sign",
            "@example.com",
            "admin@",
            "admin@example",
            "a@b@c.com",
            "a b@example.com",
            "admin@.com",
            "admin@example.",
        ] {
            assert!(!is_valid_email(email), "should reject {email}");
        }
    }

    #[test]
    fn profile_validation_reports_each_field() {
        let errors =
            field_codes(validate_profile(&request("not-an-email", "   ", "user")).unwrap_err());
        assert_eq!(errors["email"], vec!["EMAIL_INVALID"]);
        assert_eq!(errors["displayName"], vec!["DISPLAY_NAME_REQUIRED"]);

        let errors = field_codes(
            validate_profile(&request("valid@example.com", &"x".repeat(33), "user")).unwrap_err(),
        );
        assert_eq!(errors["displayName"], vec!["DISPLAY_NAME_TOO_LONG"]);

        assert_eq!(
            validate_profile(&request(" valid@example.com ", "  Name  ", "user")).unwrap(),
            ("valid@example.com".to_owned(), "Name".to_owned()),
            "both fields are trimmed"
        );
        assert_eq!(
            validate_profile(&request("valid@example.com", &"é".repeat(32), "user"))
                .unwrap()
                .1
                .chars()
                .count(),
            32,
            "the 32-character cap counts characters, not bytes"
        );
    }

    #[test]
    fn role_wire_parsing_is_exact() {
        for (wire, role) in [
            ("superadmin", Role::Superadmin),
            ("admin", Role::Admin),
            ("user", Role::User),
        ] {
            assert_eq!(Role::from_wire(wire), Some(role));
        }
        for bad in ["", "root", "Admin", "super-admin"] {
            assert_eq!(Role::from_wire(bad), None, "role={bad}");
        }
    }
}
