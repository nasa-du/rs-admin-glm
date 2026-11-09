//! Guardrail contract tests (issue #18, spec §3): self-management and
//! last-superadmin refusals, each asserted to land whole — a refused
//! operation must leave no partial effect — plus the disable branch of the
//! auth timing contract (§4.2): a disabled user's refresh is refused and
//! re-enabling restores sign-in.

mod common;

use axum::http::{Method, StatusCode};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use common::{
    assert_error_contract, body_json, login_request, manage_request, refresh_request, seed_user,
    sign_in, sign_in_session, test_app,
};

/// The target's (display_name, role, is_enabled) as stored.
async fn stored_user(pool: &PgPool, id: Uuid) -> (String, String, bool) {
    sqlx::query_as("SELECT display_name, role::text, is_enabled FROM users WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("user row")
}

#[sqlx::test]
async fn self_management_is_refused_whole(pool: PgPool) {
    let app = test_app(pool.clone());
    let root_id = seed_user(&pool, "root@example.com", "superadmin", true).await;
    let manager_id = seed_user(&pool, "manager@example.com", "admin", true).await;
    let (root_token, _) = sign_in_session(&app, "root@example.com").await;
    let (manager_token, _) = sign_in_session(&app, "manager@example.com").await;

    // Guardrail 1 (spec §3): deleting, disabling or role-changing one's own
    // account is refused — for every field combination, and for the other
    // two lifecycle endpoints as well.
    let bodies = [
        serde_json::json!({ "displayName": "New Name" }),
        serde_json::json!({ "role": "admin" }),
        serde_json::json!({ "isEnabled": false }),
        serde_json::json!({
            "displayName": "New Name",
            "role": "admin",
            "isEnabled": false,
        }),
    ];
    for body in bodies {
        let response = manage_request(
            &app,
            Method::PATCH,
            &format!("/api/users/{root_id}"),
            &root_token,
            Some(body),
        )
        .await;
        assert_error_contract(response, StatusCode::FORBIDDEN, "SELF_MANAGEMENT_FORBIDDEN").await;
    }

    let response = manage_request(
        &app,
        Method::DELETE,
        &format!("/api/users/{root_id}"),
        &root_token,
        None,
    )
    .await;
    assert_error_contract(response, StatusCode::FORBIDDEN, "SELF_MANAGEMENT_FORBIDDEN").await;

    let response = manage_request(
        &app,
        Method::POST,
        &format!("/api/users/{manager_id}/password-reset"),
        &manager_token,
        None,
    )
    .await;
    assert_error_contract(response, StatusCode::FORBIDDEN, "SELF_MANAGEMENT_FORBIDDEN").await;

    // Whole-operation refusal: none of the combined body's fields landed.
    assert_eq!(
        stored_user(&pool, root_id).await,
        ("Seeded User".to_owned(), "superadmin".to_owned(), true),
        "the refused PATCH must not change any field"
    );

    // The self check outranks the last-superadmin guardrail: root is the
    // only enabled superadmin here, yet the refusal names self-management
    // (403, not 409).
}

#[sqlx::test]
async fn last_enabled_superadmin_survives_demote_disable_and_delete(pool: PgPool) {
    let app = test_app(pool.clone());
    seed_user(&pool, "manager@example.com", "admin", true).await;
    let last_id = seed_user(&pool, "last@example.com", "superadmin", true).await;
    let (manager_token, _) = sign_in_session(&app, "manager@example.com").await;
    let (_, last_refresh) = sign_in_session(&app, "last@example.com").await;

    // Guardrail 2 (spec §3): demoting or disabling the last enabled
    // superadmin is a 409, and it is refused before manage-down — the
    // admin's rank complaint (also true) never shadows the invariant.
    for body in [
        serde_json::json!({ "role": "admin" }),
        serde_json::json!({ "isEnabled": false }),
    ] {
        let response = manage_request(
            &app,
            Method::PATCH,
            &format!("/api/users/{last_id}"),
            &manager_token,
            Some(body),
        )
        .await;
        assert_error_contract(response, StatusCode::CONFLICT, "LAST_SUPERADMIN").await;
    }

    // Deleting them needs a caller who passes the superadmin route gate; the
    // only enabled superadmin is the target, so the operator is a superadmin
    // whose row was disabled after their token was issued — exactly the
    // ≤15-minute stale-claim window the guardrail backstops (spec §3).
    let stale_id = seed_user(&pool, "stale@example.com", "superadmin", true).await;
    let (stale_token, _) = sign_in_session(&app, "stale@example.com").await;
    sqlx::query("UPDATE users SET is_enabled = FALSE WHERE id = $1")
        .bind(stale_id)
        .execute(&pool)
        .await
        .unwrap();

    let response = manage_request(
        &app,
        Method::DELETE,
        &format!("/api/users/{last_id}"),
        &stale_token,
        None,
    )
    .await;
    assert_error_contract(response, StatusCode::CONFLICT, "LAST_SUPERADMIN").await;

    // Refused whole: still an enabled superadmin, with a live session — the
    // disable never half-landed by revoking families.
    assert_eq!(
        stored_user(&pool, last_id).await,
        ("Seeded User".to_owned(), "superadmin".to_owned(), true)
    );
    let response = app
        .clone()
        .oneshot(refresh_request(Some(&last_refresh)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "no session was revoked");

    // With a second enabled superadmin, the identical operations fall
    // through to manage-down's 403 instead — the 409 belongs to the last
    // superadmin alone.
    seed_user(&pool, "second@example.com", "superadmin", true).await;
    for (method, body, token) in [
        (
            Method::PATCH,
            Some(serde_json::json!({ "role": "admin" })),
            &manager_token,
        ),
        (Method::DELETE, None, &stale_token),
    ] {
        let response =
            manage_request(&app, method, &format!("/api/users/{last_id}"), token, body).await;
        assert_error_contract(response, StatusCode::FORBIDDEN, "FORBIDDEN").await;
    }
}

#[sqlx::test]
async fn disabling_a_user_ends_their_sessions_until_reenabled(pool: PgPool) {
    let app = test_app(pool.clone());
    seed_user(&pool, "manager@example.com", "admin", true).await;
    let victim_id = seed_user(&pool, "victim@example.com", "user", true).await;
    let (manager_token, _) = sign_in_session(&app, "manager@example.com").await;

    // Two browsers = two token families, both live before the disable.
    let first = sign_in(&app, "victim@example.com").await;
    let second = sign_in(&app, "victim@example.com").await;

    let response = manage_request(
        &app,
        Method::PATCH,
        &format!("/api/users/{victim_id}"),
        &manager_token,
        Some(serde_json::json!({ "isEnabled": false })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["user"]["isEnabled"], false);

    // Every session is cut at the next refresh: the disable revoked all of
    // the victim's families (spec §4.2), so the answer is 401 REFRESH_INVALID.
    for token in [&first, &second] {
        let response = app
            .clone()
            .oneshot(refresh_request(Some(token)))
            .await
            .unwrap();
        assert_error_contract(response, StatusCode::UNAUTHORIZED, "REFRESH_INVALID").await;
    }

    // Correct password, disabled account → USER_DISABLED (password is
    // verified before the enabled flag, §4.2).
    let response = app
        .clone()
        .oneshot(login_request("victim@example.com", common::TEST_PASSWORD))
        .await
        .unwrap();
    assert_error_contract(response, StatusCode::UNAUTHORIZED, "USER_DISABLED").await;

    // Re-enabling restores sign-in — with a fresh family; the revoked tokens
    // stay dead (enabling is not resurrection).
    let response = manage_request(
        &app,
        Method::PATCH,
        &format!("/api/users/{victim_id}"),
        &manager_token,
        Some(serde_json::json!({ "isEnabled": true })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .clone()
        .oneshot(refresh_request(Some(&first)))
        .await
        .unwrap();
    assert_error_contract(response, StatusCode::UNAUTHORIZED, "REFRESH_INVALID").await;

    let response = app
        .clone()
        .oneshot(login_request("victim@example.com", common::TEST_PASSWORD))
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "can sign in again once enabled"
    );
}
