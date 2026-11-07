//! AppError — the semantic error-code boundary between backend and frontend
//! (docs/spec.md §5.2, ADR 0002/0003).
//!
//! Every API failure crosses this enum into one of ten stable codes; the HTTP
//! status and the `{code, title}` body derive purely from the variant.
//! `title` is an English fallback only — display text lives in the frontend
//! locales keyed by `code`; the server never knows the UI language.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// Field-level codes carried by [`AppError::ValidationError`] — e.g.
/// `errors: { "email": ["EMAIL_TAKEN"] }`. Stringly-typed on purpose: the
/// set grows with implementation and stays in lockstep with the frontend
/// `errors` locale namespace (spec §5.2).
pub type FieldErrors = BTreeMap<String, Vec<String>>;

/// The ten stable error codes of spec §5.2. Exhaustive `match`es below keep
/// adding a variant from compiling without a mapping.
///
/// The mapping is erected ahead of its consumers (issue #14): most variants
/// are first constructed by the auth module (M2) and users module (M3).
#[allow(
    dead_code,
    reason = "skeleton — first consumers land with M2/M3 (spec §8)"
)]
#[derive(Debug)]
pub enum AppError {
    /// 401 — login email unknown or password wrong (same code, anti-enumeration).
    InvalidCredentials,
    /// 401 — correct password but `is_enabled = false`; also refresh-time disability.
    UserDisabled,
    /// 409 — create-user email collision (carries `errors.email`).
    EmailTaken,
    /// 422 — field validation failed (carries the per-field code map).
    ValidationError(FieldErrors),
    /// 403 — role rank insufficient / manage-down violation / non-superadmin role change.
    Forbidden,
    /// 403 — self delete / self disable / changing own role (guardrail 1, spec §3).
    SelfManagementForbidden,
    /// 409 — the last enabled superadmin would be downgraded / disabled / deleted
    /// (guardrail 2, spec §3).
    LastSuperadmin,
    /// 401 — refresh missing / unknown / expired / reused (family revoked with it).
    RefreshInvalid,
    /// 404 — path target user does not exist.
    NotFound,
    /// 500 — catch-all; details are logged server-side, never sent to clients.
    Internal(anyhow::Error),
}

impl AppError {
    /// Pure mapping variant → (HTTP status, stable code, English fallback title).
    /// This is the function the §5.2 table pins; unit-tested for all ten codes.
    pub fn parts(&self) -> (StatusCode, &'static str, &'static str) {
        match self {
            Self::InvalidCredentials => (
                StatusCode::UNAUTHORIZED,
                "INVALID_CREDENTIALS",
                "Email or password is incorrect",
            ),
            Self::UserDisabled => (
                StatusCode::UNAUTHORIZED,
                "USER_DISABLED",
                "This account has been disabled",
            ),
            Self::EmailTaken => (
                StatusCode::CONFLICT,
                "EMAIL_TAKEN",
                "This email is already registered",
            ),
            Self::ValidationError(_) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "VALIDATION_ERROR",
                "Validation failed",
            ),
            Self::Forbidden => (
                StatusCode::FORBIDDEN,
                "FORBIDDEN",
                "You do not have permission to perform this action",
            ),
            Self::SelfManagementForbidden => (
                StatusCode::FORBIDDEN,
                "SELF_MANAGEMENT_FORBIDDEN",
                "You cannot apply this action to your own account",
            ),
            Self::LastSuperadmin => (
                StatusCode::CONFLICT,
                "LAST_SUPERADMIN",
                "The system must keep at least one enabled superadmin",
            ),
            Self::RefreshInvalid => (
                StatusCode::UNAUTHORIZED,
                "REFRESH_INVALID",
                "Your session has expired, please sign in again",
            ),
            Self::NotFound => (StatusCode::NOT_FOUND, "NOT_FOUND", "Not found"),
            Self::Internal(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                "Internal server error",
            ),
        }
    }

    /// The wire body for this error (test seam; production path is IntoResponse).
    fn body(&self) -> ErrorBody<'_> {
        let (_, code, title) = self.parts();
        ErrorBody {
            code,
            title,
            errors: match self {
                Self::ValidationError(errors) => Some(errors),
                // §5.2: 409 EMAIL_TAKEN carries `errors.email` so the create
                // form can flag the offending field.
                Self::EmailTaken => Some(&EMAIL_TAKEN_ERRORS),
                _ => None,
            },
        }
    }
}

/// The `errors.email` map attached to 409 EMAIL_TAKEN (§5.2).
static EMAIL_TAKEN_ERRORS: LazyLock<FieldErrors> =
    LazyLock::new(|| BTreeMap::from([("email".to_string(), vec!["EMAIL_TAKEN".to_string()])]));

/// `{code, title}` + optional `errors` — camelCase per ADR 0003.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorBody<'a> {
    code: &'a str,
    title: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    errors: Option<&'a FieldErrors>,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        if let Self::Internal(err) = &self {
            tracing::error!(error = ?err, "internal error");
        }
        let (status, _, _) = self.parts();
        (status, Json(self.body())).into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        Self::Internal(err.into())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use axum::response::IntoResponse;

    use super::AppError;

    fn case(name: &str) -> AppError {
        match name {
            "invalid_credentials" => AppError::InvalidCredentials,
            "user_disabled" => AppError::UserDisabled,
            "email_taken" => AppError::EmailTaken,
            "validation_error" => {
                let mut errors = BTreeMap::new();
                errors.insert("email".to_string(), vec!["EMAIL_TAKEN".to_string()]);
                AppError::ValidationError(errors)
            }
            "forbidden" => AppError::Forbidden,
            "self_management_forbidden" => AppError::SelfManagementForbidden,
            "last_superadmin" => AppError::LastSuperadmin,
            "refresh_invalid" => AppError::RefreshInvalid,
            "not_found" => AppError::NotFound,
            "internal" => AppError::Internal(anyhow::anyhow!("boom")),
            other => panic!("unknown case {other}"),
        }
    }

    /// The full §5.2 table: variant → expected (HTTP status, code).
    const SPEC_TABLE: &[(&str, u16, &str)] = &[
        ("invalid_credentials", 401, "INVALID_CREDENTIALS"),
        ("user_disabled", 401, "USER_DISABLED"),
        ("email_taken", 409, "EMAIL_TAKEN"),
        ("validation_error", 422, "VALIDATION_ERROR"),
        ("forbidden", 403, "FORBIDDEN"),
        (
            "self_management_forbidden",
            403,
            "SELF_MANAGEMENT_FORBIDDEN",
        ),
        ("last_superadmin", 409, "LAST_SUPERADMIN"),
        ("refresh_invalid", 401, "REFRESH_INVALID"),
        ("not_found", 404, "NOT_FOUND"),
        ("internal", 500, "INTERNAL_ERROR"),
    ];

    #[test]
    fn every_variant_maps_to_its_spec_status_and_code() {
        assert_eq!(SPEC_TABLE.len(), 10, "spec §5.2 pins exactly ten codes");
        for (name, status, code) in SPEC_TABLE {
            let (got_status, got_code, got_title) = case(name).parts();
            assert_eq!(got_status.as_u16(), *status, "status for {name}");
            assert_eq!(got_code, *code, "code for {name}");
            assert!(!got_title.is_empty(), "title must be a non-empty fallback");
        }
    }

    #[test]
    fn codes_are_unique() {
        let codes: Vec<_> = SPEC_TABLE.iter().map(|(_, _, code)| *code).collect();
        let mut sorted = codes.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(codes.len(), sorted.len(), "codes must not collide");
    }

    async fn body_json(err: AppError) -> serde_json::Value {
        // IntoResponse composes the same body the wire sees; exercise it
        // directly so tests assert the actual response payload shape.
        let response = err.into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        serde_json::from_slice(&body).expect("body is JSON")
    }

    #[tokio::test]
    async fn internal_error_body_never_leaks_details() {
        let body = body_json(case("internal")).await;
        assert_eq!(body["code"], "INTERNAL_ERROR");
        assert_eq!(body["title"], "Internal server error");
        assert!(body.get("errors").is_none(), "no field map on non-422");
        assert!(
            !body.to_string().contains("boom"),
            "internal details must not reach the wire"
        );
    }

    #[tokio::test]
    async fn validation_error_carries_field_map() {
        let body = body_json(case("validation_error")).await;
        assert_eq!(body["code"], "VALIDATION_ERROR");
        assert_eq!(body["title"], "Validation failed");
        assert_eq!(body["errors"]["email"][0], "EMAIL_TAKEN");
    }

    #[tokio::test]
    async fn email_taken_carries_the_email_field() {
        // §5.2: "EMAIL_TAKEN | 409 | 创建用户邮箱重复（附 errors.email）"
        let body = body_json(case("email_taken")).await;
        assert_eq!(body["code"], "EMAIL_TAKEN");
        assert_eq!(body["errors"]["email"][0], "EMAIL_TAKEN");
    }

    #[tokio::test]
    async fn into_response_uses_mapped_status() {
        for (name, status, _) in SPEC_TABLE {
            let response = case(name).into_response();
            assert_eq!(response.status().as_u16(), *status, "response for {name}");
        }
    }
}
