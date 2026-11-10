//! `PATCH /api/auth/password` and `PATCH /api/users/me` contract tests
//! (issue #19, spec §4.2 / §5.3). One test = one contract row:
//!
//! - the current password is verified with a field-level 422 (never a 401,
//!   which the frontend interceptor would answer with a pointless refresh
//!   replay) and the new password follows the same 8–128 policy as creation;
//! - success rewrites the hash, clears `must_change_password` and revokes
//!   **every** token family of the user — the last branch of the auth timing
//!   contract (§4.2, category ①) closes here;
//! - the first-login chain (category ⑤, the admin-creates → hand-over →
//!   forced-change demo of issue #19 AC5) runs end-to-end through the real
//!   create endpoint and its one-time initial password;
//! - `/users/me` is the self-service profile endpoint: displayName only.

mod common;

use axum::http::{Method, StatusCode};
use sqlx::PgPool;
use tower::ServiceExt;

use common::{
    TEST_PASSWORD, assert_error_contract, body_json, login_request, manage_request,
    refresh_request, seed_user, sign_in_session, test_app,
};

/// The caller's new password in the happy-path tests (inside the 8–128 policy).
const NEW_PASSWORD: &str = "brand-new-pass-1";

/// `PATCH /api/auth/password` and the response, status asserted by the caller.
async fn change_password(
    app: &axum::Router,
    access: &str,
    current: &str,
    new: &str,
) -> axum::response::Response {
    manage_request(
        app,
        Method::PATCH,
        "/api/auth/password",
        access,
        Some(serde_json::json!({
            "currentPassword": current,
            "newPassword": new,
        })),
    )
    .await
}

#[sqlx::test]
async fn password_change_revokes_every_family_and_forces_relogin(pool: PgPool) {
    let app = test_app(pool.clone());
    let user_id = seed_user(&pool, "member@example.com", "user", true).await;

    // Two browsers hold live sessions (two logins = two families).
    let (access, own_refresh) = sign_in_session(&app, "member@example.com").await;
    let other_refresh = common::sign_in(&app, "member@example.com").await;

    let response = change_password(&app, &access, TEST_PASSWORD, NEW_PASSWORD).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(bytes.is_empty(), "204 carries no body");

    // Every family is revoked — the caller's own included.
    let remaining: i64 =
        sqlx::query_scalar("SELECT count(*) FROM refresh_tokens WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(remaining, 0, "password change revokes every family");

    for token in [&own_refresh, &other_refresh] {
        let response = app
            .clone()
            .oneshot(refresh_request(Some(token)))
            .await
            .unwrap();
        assert_error_contract(response, StatusCode::UNAUTHORIZED, "REFRESH_INVALID").await;
    }

    // The stored hash is the new password; the flag is cleared.
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(rs_admin_backend::password::verify_password(NEW_PASSWORD, &hash).unwrap());

    // The old password no longer signs in; the new one does, flag cleared.
    let response = app
        .clone()
        .oneshot(login_request("member@example.com", TEST_PASSWORD))
        .await
        .unwrap();
    assert_error_contract(response, StatusCode::UNAUTHORIZED, "INVALID_CREDENTIALS").await;

    let response = app
        .clone()
        .oneshot(login_request("member@example.com", NEW_PASSWORD))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await["user"]["mustChangePassword"],
        false
    );
}

#[sqlx::test]
async fn wrong_current_password_is_a_field_level_422_not_a_401(pool: PgPool) {
    let app = test_app(pool.clone());
    seed_user(&pool, "member@example.com", "user", true).await;
    let (access, refresh) = sign_in_session(&app, "member@example.com").await;

    let response = change_password(&app, &access, "not-my-password", NEW_PASSWORD).await;
    let body = assert_error_contract(
        response,
        StatusCode::UNPROCESSABLE_ENTITY,
        "VALIDATION_ERROR",
    )
    .await;
    assert_eq!(
        body["errors"]["currentPassword"],
        serde_json::json!(["CURRENT_PASSWORD_INCORRECT"]),
        "the refusal names the offending field: {body}"
    );

    // Refused whole (护栏 semantics): the old password still signs in and the
    // live session survives — a wrong current password revokes nothing.
    let response = app
        .clone()
        .oneshot(refresh_request(Some(&refresh)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "the family survives");
    let response = app
        .clone()
        .oneshot(login_request("member@example.com", TEST_PASSWORD))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "the password survives");
}

#[sqlx::test]
async fn the_new_password_follows_the_8_to_128_policy(pool: PgPool) {
    let app = test_app(pool.clone());
    seed_user(&pool, "member@example.com", "user", true).await;
    let (access, _) = sign_in_session(&app, "member@example.com").await;

    for (new, code) in [
        ("seven77", "PASSWORD_TOO_SHORT"),
        (&"x".repeat(129), "PASSWORD_TOO_LONG"),
    ] {
        let response = change_password(&app, &access, TEST_PASSWORD, new).await;
        let body = assert_error_contract(
            response,
            StatusCode::UNPROCESSABLE_ENTITY,
            "VALIDATION_ERROR",
        )
        .await;
        assert_eq!(
            body["errors"]["newPassword"],
            serde_json::json!([code]),
            "for {new}: {body}"
        );
    }

    // Both fields wrong at once: one 422 carries both field codes.
    let response = change_password(&app, &access, "not-my-password", "seven77").await;
    let body = assert_error_contract(
        response,
        StatusCode::UNPROCESSABLE_ENTITY,
        "VALIDATION_ERROR",
    )
    .await;
    assert_eq!(
        body["errors"]["currentPassword"],
        serde_json::json!(["CURRENT_PASSWORD_INCORRECT"])
    );
    assert_eq!(
        body["errors"]["newPassword"],
        serde_json::json!(["PASSWORD_TOO_SHORT"])
    );
}

#[sqlx::test]
async fn password_change_requires_a_valid_access_token(pool: PgPool) {
    let app = test_app(pool.clone());
    seed_user(&pool, "member@example.com", "user", true).await;

    let missing = axum::http::Request::patch("/api/auth/password")
        .header("content-type", "application/json")
        .body(axum::body::Body::from(
            serde_json::json!({
                "currentPassword": TEST_PASSWORD,
                "newPassword": NEW_PASSWORD,
            })
            .to_string(),
        ))
        .unwrap();
    let response = app.clone().oneshot(missing).await.unwrap();
    assert_error_contract(response, StatusCode::UNAUTHORIZED, "INVALID_CREDENTIALS").await;

    let response = change_password(&app, "not-a-jwt", TEST_PASSWORD, NEW_PASSWORD).await;
    assert_error_contract(response, StatusCode::UNAUTHORIZED, "INVALID_CREDENTIALS").await;
}

/// The issue #19 AC5 demo as a test (category ⑤ of spec §7): an admin creates
/// a user, the one-time initial password is handed over out of band, the new
/// user's first sign-in is flagged, the forced change lands on
/// `PATCH /auth/password`, and the initial password dies with it.
#[sqlx::test]
async fn the_first_login_password_chain_closes_end_to_end(pool: PgPool) {
    let app = test_app(pool.clone());
    seed_user(&pool, "root@example.com", "superadmin", true).await;
    let (root_access, _) = sign_in_session(&app, "root@example.com").await;

    // The admin creates the account; the initial password is shown exactly once.
    let response = manage_request(
        &app,
        Method::POST,
        "/api/users",
        &root_access,
        Some(serde_json::json!({
            "email": "newbie@example.com",
            "displayName": "New User",
            "role": "user",
        })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = body_json(response).await;
    assert_eq!(body["user"]["mustChangePassword"], true);
    let initial_password = body["initialPassword"].as_str().expect("initialPassword");

    // First sign-in with the initial password succeeds and carries the flag —
    // the frontend guard's cue to force the password page.
    let response = app
        .clone()
        .oneshot(login_request("newbie@example.com", initial_password))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let refresh = common::refresh_token(&response);
    let body = body_json(response).await;
    assert_eq!(body["user"]["mustChangePassword"], true);
    let access = body["accessToken"].as_str().unwrap().to_owned();

    // The forced change succeeds…
    let response = change_password(&app, &access, initial_password, NEW_PASSWORD).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    // …and with it the initial password is dead, the session is dead, and the
    // next sign-in with the chosen password is unflagged.
    let response = app
        .clone()
        .oneshot(login_request("newbie@example.com", initial_password))
        .await
        .unwrap();
    assert_error_contract(response, StatusCode::UNAUTHORIZED, "INVALID_CREDENTIALS").await;

    let response = app
        .clone()
        .oneshot(refresh_request(Some(&refresh)))
        .await
        .unwrap();
    assert_error_contract(response, StatusCode::UNAUTHORIZED, "REFRESH_INVALID").await;

    let response = app
        .clone()
        .oneshot(login_request("newbie@example.com", NEW_PASSWORD))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await["user"]["mustChangePassword"],
        false
    );
}

#[sqlx::test]
async fn users_me_updates_only_the_display_name(pool: PgPool) {
    let app = test_app(pool.clone());
    let user_id = seed_user(&pool, "member@example.com", "user", true).await;
    let (access, refresh) = sign_in_session(&app, "member@example.com").await;

    // Self-service profile (spec §5.3): displayName only. Any other field in
    // the body is ignored — the endpoint has no manage-down entanglement.
    let response = manage_request(
        &app,
        Method::PATCH,
        "/api/users/me",
        &access,
        Some(serde_json::json!({
            "displayName": "  Own Name  ",
            "email": "hacker@evil.com",
            "role": "superadmin",
            "isEnabled": false,
        })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["user"]["displayName"], "Own Name", "trimmed");
    assert_eq!(body["user"]["email"], "member@example.com");
    assert_eq!(body["user"]["role"], "user");
    assert_eq!(body["user"]["isEnabled"], true);

    let stored: (String, String, bool) =
        sqlx::query_as("SELECT email, role::text, is_enabled FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        stored,
        ("member@example.com".to_owned(), "user".to_owned(), true),
        "nothing but displayName landed"
    );

    // The same 1–32 display-name policy as create / update, field-level.
    for (name, code) in [
        ("   ", "DISPLAY_NAME_REQUIRED"),
        (&"x".repeat(33), "DISPLAY_NAME_TOO_LONG"),
    ] {
        let response = manage_request(
            &app,
            Method::PATCH,
            "/api/users/me",
            &access,
            Some(serde_json::json!({ "displayName": name })),
        )
        .await;
        let body = assert_error_contract(
            response,
            StatusCode::UNPROCESSABLE_ENTITY,
            "VALIDATION_ERROR",
        )
        .await;
        assert_eq!(
            body["errors"]["displayName"],
            serde_json::json!([code]),
            "for {name}: {body}"
        );
    }

    // A profile edit is not a password change: the session survives.
    let response = app
        .clone()
        .oneshot(refresh_request(Some(&refresh)))
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "profile edit keeps sessions"
    );
}

#[sqlx::test]
async fn users_me_requires_a_valid_access_token(pool: PgPool) {
    let app = test_app(pool.clone());
    seed_user(&pool, "member@example.com", "user", true).await;

    let missing = axum::http::Request::patch("/api/users/me")
        .header("content-type", "application/json")
        .body(axum::body::Body::from(
            serde_json::json!({ "displayName": "No Token" }).to_string(),
        ))
        .unwrap();
    let response = app.clone().oneshot(missing).await.unwrap();
    assert_error_contract(response, StatusCode::UNAUTHORIZED, "INVALID_CREDENTIALS").await;

    let response = manage_request(
        &app,
        Method::PATCH,
        "/api/users/me",
        "not-a-jwt",
        Some(serde_json::json!({ "displayName": "Garbage Token" })),
    )
    .await;
    assert_error_contract(response, StatusCode::UNAUTHORIZED, "INVALID_CREDENTIALS").await;
}

#[sqlx::test]
async fn dev_swagger_documents_password_change_and_users_me(pool: PgPool) {
    let response = test_app(pool)
        .oneshot(
            axum::http::Request::get("/api-docs/openapi.json")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let doc = body_json(response).await;
    assert!(
        doc["paths"]["/api/auth/password"]["patch"].is_object(),
        "openapi.json must document patch /api/auth/password: {}",
        doc["paths"]
    );
    assert!(
        doc["paths"]["/api/users/me"]["patch"].is_object(),
        "openapi.json must document patch /api/users/me: {}",
        doc["paths"]
    );
}
