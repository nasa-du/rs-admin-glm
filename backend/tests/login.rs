//! `POST /api/auth/login` contract tests (issue #15, spec §4.2 / §5.3).
//!
//! Every test is one row of the API contract: request in → status + JSON
//! body out, through the assembled router (ADR 0004).

mod common;

use axum::http::StatusCode;
use chrono::DateTime;
use sqlx::PgPool;
use tower::ServiceExt;

use common::{assert_refresh_cookie, decode_access, login_request, seed_user, test_app};

#[sqlx::test]
async fn correct_credentials_return_200_user_and_refresh_cookie(pool: PgPool) {
    let user_id = seed_user(&pool, "root@example.com", "superadmin", true).await;

    let response = test_app(pool.clone())
        .oneshot(login_request("root@example.com", common::TEST_PASSWORD))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // §4.1 cookie: rs_admin_refresh; HttpOnly; Secure; SameSite=Strict;
    // Path=/api/auth. Max-Age carries the 7-day sliding window.
    assert_refresh_cookie(&response);

    let body = common::body_json(response).await;

    // accessToken: verifiable HS256 JWT with the §4.1 claim set.
    let access = body["accessToken"].as_str().expect("accessToken");
    let claims = decode_access(access);
    assert_eq!(claims["sub"], user_id.to_string());
    assert_eq!(claims["email"], "root@example.com");
    assert_eq!(claims["role"], "superadmin");
    assert_eq!(
        claims["exp"].as_i64().unwrap() - claims["iat"].as_i64().unwrap(),
        900,
        "spec §4.1 pins a 15-minute access token"
    );

    // user: the §5.1 wire shape, camelCase, no secrets.
    let user = &body["user"];
    assert_eq!(user["id"], user_id.to_string());
    assert_eq!(user["email"], "root@example.com");
    assert_eq!(user["displayName"], "Seeded User");
    assert_eq!(user["role"], "superadmin");
    assert_eq!(user["isEnabled"], true);
    assert_eq!(user["mustChangePassword"], false);
    for field in ["createdAt", "updatedAt"] {
        let raw = user[field].as_str().unwrap_or_else(|| panic!("{field}"));
        DateTime::parse_from_rfc3339(raw)
            .unwrap_or_else(|err| panic!("{field} must be RFC 3339 ({raw}): {err}"));
    }
    assert!(
        user.get("passwordHash").is_none(),
        "the wire user must never leak the hash"
    );
}

#[sqlx::test]
async fn wrong_password_returns_401_invalid_credentials(pool: PgPool) {
    seed_user(&pool, "root@example.com", "admin", true).await;

    let response = test_app(pool)
        .oneshot(login_request("root@example.com", "not-the-seeded-password"))
        .await
        .unwrap();

    let body =
        common::assert_error_contract(response, StatusCode::UNAUTHORIZED, "INVALID_CREDENTIALS")
            .await;
    assert!(
        body.get("errors").is_none(),
        "401 carries no field map: {body}"
    );
}

#[sqlx::test]
async fn unknown_email_returns_the_same_401_body_as_wrong_password(pool: PgPool) {
    // §4.2 anti-enumeration: same code, same body — an attacker learns
    // nothing about which emails exist.
    seed_user(&pool, "root@example.com", "user", true).await;

    let app = test_app(pool);
    let wrong_password = app
        .clone()
        .oneshot(login_request("root@example.com", "not-the-seeded-password"))
        .await
        .unwrap();
    let unknown_email = app
        .oneshot(login_request(
            "nobody@example.com",
            "not-the-seeded-password",
        ))
        .await
        .unwrap();

    assert_eq!(wrong_password.status(), unknown_email.status());
    let wrong_body = common::assert_error_contract(
        wrong_password,
        StatusCode::UNAUTHORIZED,
        "INVALID_CREDENTIALS",
    )
    .await;
    let unknown_body = common::assert_error_contract(
        unknown_email,
        StatusCode::UNAUTHORIZED,
        "INVALID_CREDENTIALS",
    )
    .await;
    assert_eq!(wrong_body, unknown_body, "bodies must be byte-identical");
}

#[sqlx::test]
async fn correct_password_on_disabled_account_returns_401_user_disabled(pool: PgPool) {
    seed_user(&pool, "suspended@example.com", "user", false).await;

    let response = test_app(pool)
        .oneshot(login_request(
            "suspended@example.com",
            common::TEST_PASSWORD,
        ))
        .await
        .unwrap();

    common::assert_error_contract(response, StatusCode::UNAUTHORIZED, "USER_DISABLED").await;
}

#[sqlx::test]
async fn wrong_password_on_disabled_account_stays_invalid_credentials(pool: PgPool) {
    // The disabled code only surfaces once the password is proven — a wrong
    // password must not leak that the account exists and is disabled.
    seed_user(&pool, "suspended@example.com", "user", false).await;

    let response = test_app(pool)
        .oneshot(login_request(
            "suspended@example.com",
            "not-the-seeded-password",
        ))
        .await
        .unwrap();

    common::assert_error_contract(response, StatusCode::UNAUTHORIZED, "INVALID_CREDENTIALS").await;
}

#[sqlx::test]
async fn unusable_credentials_still_return_401_not_422(pool: PgPool) {
    // Issue #15 AC2 draws the login contract at 200/401 only: a password
    // outside the creation-time policy or a malformed identifier is still
    // just a failed sign-in, not a field-validation error.
    seed_user(&pool, "root@example.com", "user", true).await;
    let app = test_app(pool);

    for (email, password) in [
        ("root@example.com", "short"),            // below the 8-char floor
        ("not-an-email", "long-enough-password"), // not an email at all
    ] {
        let response = app
            .clone()
            .oneshot(login_request(email, password))
            .await
            .unwrap();
        common::assert_error_contract(response, StatusCode::UNAUTHORIZED, "INVALID_CREDENTIALS")
            .await;
    }
}

#[sqlx::test]
async fn malformed_body_returns_the_contract_422_not_axum_text(pool: PgPool) {
    let response = test_app(pool)
        .oneshot(common::json_request(
            "/api/auth/login",
            serde_json::json!("not an object"),
        ))
        .await
        .unwrap();

    let body = common::assert_error_contract(
        response,
        StatusCode::UNPROCESSABLE_ENTITY,
        "VALIDATION_ERROR",
    )
    .await;
    assert!(
        body.get("errors").is_none(),
        "unparseable body has no field to blame: {body}"
    );
}

// Swagger: the OpenAPI document names the login endpoint, and only the
// development app mounts it (user story 32; issue #15 acceptance 5).
#[sqlx::test]
async fn dev_swagger_documents_the_login_endpoint(pool: PgPool) {
    let response = test_app(pool)
        .oneshot(
            axum::http::Request::get("/api-docs/openapi.json")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let doc = common::body_json(response).await;
    assert!(
        doc["paths"]["/api/auth/login"]["post"].is_object(),
        "openapi.json must document POST /api/auth/login: {}",
        doc["paths"]
    );
}

#[sqlx::test]
async fn production_app_mounts_no_swagger(pool: PgPool) {
    let app = rs_admin_backend::app_router(common::test_state(pool), false, None);

    for path in ["/api-docs/openapi.json", "/swagger-ui"] {
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::get(path)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
}
