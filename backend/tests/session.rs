//! `POST /api/auth/refresh` and `POST /api/auth/logout` contract tests
//! (issue #16, spec §4.2). One test = one row of the auth timing contract:
//! rotation, reuse detection, independent families, idempotent logout, and
//! the CSRF header guard — all through the assembled router (ADR 0004).

mod common;

use std::time::Duration;

use axum::Router;
use axum::http::StatusCode;
use rs_admin_backend::auth::tokens::{family_lock_key, hash_token};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use common::{
    CSRF_VALUE, assert_error_contract, assert_refresh_cookie, auth_request, body_json,
    decode_access, logout_request, refresh_request, refresh_token, seed_user, sign_in, test_app,
};

/// The family that owns a presented refresh token.
async fn family_of(pool: &PgPool, token: &str) -> Uuid {
    sqlx::query_scalar("SELECT family_id FROM refresh_tokens WHERE token_hash = $1")
        .bind(hash_token(token))
        .fetch_one(pool)
        .await
        .expect("the presented token has a row")
}

/// A successful rotation: asserts 200 + a fresh §4.1 cookie, returns the new
/// refresh plaintext.
async fn refresh_ok(app: &Router, cookie: &str) -> String {
    let response = app
        .clone()
        .oneshot(refresh_request(Some(cookie)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "refresh must succeed");
    assert_refresh_cookie(&response);
    refresh_token(&response)
}

#[sqlx::test]
async fn refresh_rotates_and_replaying_the_old_token_revokes_the_family(pool: PgPool) {
    seed_user(&pool, "root@example.com", "superadmin", true).await;
    let app = test_app(pool);
    let first = sign_in(&app, "root@example.com").await;

    let response = app
        .clone()
        .oneshot(refresh_request(Some(&first)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_refresh_cookie(&response);
    let second = refresh_token(&response);
    assert_ne!(first, second, "a successful refresh rotates the token");

    let body = body_json(response).await;
    assert!(
        body["accessToken"].is_string(),
        "refresh returns an access token"
    );
    assert_eq!(body["user"]["email"], "root@example.com");
    let claims = decode_access(body["accessToken"].as_str().unwrap());
    assert_eq!(claims["email"], "root@example.com");

    // The old token reappears: reuse detection revokes the whole family.
    let response = app
        .clone()
        .oneshot(refresh_request(Some(&first)))
        .await
        .unwrap();
    assert_error_contract(response, StatusCode::UNAUTHORIZED, "REFRESH_INVALID").await;

    // Even the legitimate successor is dead now.
    let response = app.oneshot(refresh_request(Some(&second))).await.unwrap();
    assert_error_contract(response, StatusCode::UNAUTHORIZED, "REFRESH_INVALID").await;
}

#[sqlx::test]
async fn two_browsers_hold_and_rotate_independent_families(pool: PgPool) {
    seed_user(&pool, "root@example.com", "superadmin", true).await;
    let app = test_app(pool);

    // Two logins = two token families (one per browser).
    let desktop = sign_in(&app, "root@example.com").await;
    let laptop = sign_in(&app, "root@example.com").await;
    assert_ne!(desktop, laptop);

    // Both chains rotate without interfering.
    let desktop = refresh_ok(&app, &desktop).await;
    let laptop = refresh_ok(&app, &laptop).await;
    let desktop = refresh_ok(&app, &desktop).await;
    let laptop = refresh_ok(&app, &laptop).await;

    // Logging out one browser revokes only its own family.
    let response = app
        .clone()
        .oneshot(logout_request(Some(&desktop)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let response = app
        .clone()
        .oneshot(refresh_request(Some(&desktop)))
        .await
        .unwrap();
    assert_error_contract(response, StatusCode::UNAUTHORIZED, "REFRESH_INVALID").await;

    refresh_ok(&app, &laptop).await;
}

#[sqlx::test]
async fn logout_is_idempotent_and_clears_the_cookie(pool: PgPool) {
    seed_user(&pool, "root@example.com", "user", true).await;
    let app = test_app(pool);
    let token = sign_in(&app, "root@example.com").await;

    // With a cookie: revoke the family, clear the cookie, 204 with no body.
    let response = app
        .clone()
        .oneshot(logout_request(Some(&token)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let cookie = common::set_cookie(&response);
    assert!(cookie.starts_with("rs_admin_refresh="), "cookie: {cookie}");
    for attribute in [
        "Max-Age=0",
        "HttpOnly",
        "Secure",
        "SameSite=Strict",
        "Path=/api/auth",
    ] {
        assert!(cookie.contains(attribute), "missing {attribute}: {cookie}");
    }
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(bytes.is_empty(), "204 carries no body");

    // The family is gone.
    let response = app
        .clone()
        .oneshot(refresh_request(Some(&token)))
        .await
        .unwrap();
    assert_error_contract(response, StatusCode::UNAUTHORIZED, "REFRESH_INVALID").await;

    // No cookie is still a successful logout — twice, and for an unknown
    // cookie too.
    for request in [
        logout_request(None),
        logout_request(None),
        logout_request(Some("unknown")),
    ] {
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }
}

#[sqlx::test]
async fn refresh_and_logout_require_the_custom_header(pool: PgPool) {
    seed_user(&pool, "root@example.com", "user", true).await;
    let app = test_app(pool);
    let token = sign_in(&app, "root@example.com").await;

    // Missing or wrong header values are rejected before anything happens.
    for csrf in [None, Some("fetch")] {
        let response = app
            .clone()
            .oneshot(auth_request("/api/auth/refresh", Some(&token), csrf))
            .await
            .unwrap();
        assert_error_contract(response, StatusCode::UNAUTHORIZED, "REFRESH_INVALID").await;

        let response = app
            .clone()
            .oneshot(auth_request("/api/auth/logout", Some(&token), csrf))
            .await
            .unwrap();
        assert_error_contract(response, StatusCode::UNAUTHORIZED, "REFRESH_INVALID").await;
    }

    // Neither rejected call rotated or revoked anything.
    let rotated = refresh_ok(&app, &token).await;

    // The proper header is the exact value, not just any present header.
    let response = app
        .clone()
        .oneshot(auth_request(
            "/api/auth/refresh",
            Some(&rotated),
            Some(CSRF_VALUE),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[sqlx::test]
async fn refresh_without_a_cookie_returns_401(pool: PgPool) {
    let app = test_app(pool);

    let response = app.oneshot(refresh_request(None)).await.unwrap();
    assert_error_contract(response, StatusCode::UNAUTHORIZED, "REFRESH_INVALID").await;
}

#[sqlx::test]
async fn refresh_after_the_user_is_deleted_returns_401(pool: PgPool) {
    // Deleting a user cascades their token families away (spec §2.2), so the
    // presented refresh token no longer exists.
    let user_id = seed_user(&pool, "root@example.com", "user", true).await;
    let app = test_app(pool.clone());
    let token = sign_in(&app, "root@example.com").await;

    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();

    let response = app.oneshot(refresh_request(Some(&token))).await.unwrap();
    assert_error_contract(response, StatusCode::UNAUTHORIZED, "REFRESH_INVALID").await;
}

#[sqlx::test]
async fn refresh_reissues_access_with_the_current_user_row(pool: PgPool) {
    // The refresh path re-reads the user row (ADR 0001): a role change is
    // reflected in the next access token, not the one already issued.
    let user_id = seed_user(&pool, "root@example.com", "user", true).await;
    let app = test_app(pool.clone());
    let token = sign_in(&app, "root@example.com").await;

    sqlx::query("UPDATE users SET role = 'admin', display_name = 'Renamed' WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();

    let response = app.oneshot(refresh_request(Some(&token))).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;

    assert_eq!(body["user"]["role"], "admin");
    assert_eq!(body["user"]["displayName"], "Renamed");
    let claims = decode_access(body["accessToken"].as_str().unwrap());
    assert_eq!(claims["role"], "admin", "new access carries the new role");
}

#[sqlx::test]
async fn refresh_for_a_disabled_user_revokes_the_family_and_returns_401(pool: PgPool) {
    // The branch is part of this ticket; end-to-end coverage through the
    // disabling API lands with the user-management ticket (issue #16 AC5).
    let user_id = seed_user(&pool, "suspended@example.com", "user", true).await;
    let app = test_app(pool.clone());
    let token = sign_in(&app, "suspended@example.com").await;

    sqlx::query("UPDATE users SET is_enabled = FALSE WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();

    let response = app
        .clone()
        .oneshot(refresh_request(Some(&token)))
        .await
        .unwrap();
    assert_error_contract(response, StatusCode::UNAUTHORIZED, "USER_DISABLED").await;

    // The refusal revoked the family as well.
    let response = app.oneshot(refresh_request(Some(&token))).await.unwrap();
    assert_error_contract(response, StatusCode::UNAUTHORIZED, "REFRESH_INVALID").await;
}

#[sqlx::test]
async fn expired_refresh_token_returns_401(pool: PgPool) {
    let user_id = seed_user(&pool, "root@example.com", "user", true).await;
    let app = test_app(pool.clone());
    let token = sign_in(&app, "root@example.com").await;

    sqlx::query(
        "UPDATE refresh_tokens SET expires_at = now() - interval '1 minute' WHERE user_id = $1",
    )
    .bind(user_id)
    .execute(&pool)
    .await
    .unwrap();

    let response = app.oneshot(refresh_request(Some(&token))).await.unwrap();
    assert_error_contract(response, StatusCode::UNAUTHORIZED, "REFRESH_INVALID").await;

    // §4.2: an expired token also cleans up its family, not just refuses.
    let remaining: i64 =
        sqlx::query_scalar("SELECT count(*) FROM refresh_tokens WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(remaining, 0, "the expired family is deleted");
}

#[sqlx::test]
async fn refresh_waits_for_the_family_lock(pool: PgPool) {
    seed_user(&pool, "root@example.com", "user", true).await;
    let app = test_app(pool.clone());
    let token = sign_in(&app, "root@example.com").await;
    let family_id = family_of(&pool, &token).await;

    // Hold the advisory lock the handler uses. Without it, a revocation can
    // run with a snapshot taken before a concurrent rotation commits, and the
    // successor row survives (see tokens::lock_family).
    let mut lock_tx = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(family_lock_key(family_id))
        .execute(&mut *lock_tx)
        .await
        .unwrap();

    let mut request = Box::pin(app.clone().oneshot(refresh_request(Some(&token))));
    assert!(
        tokio::time::timeout(Duration::from_millis(300), &mut request)
            .await
            .is_err(),
        "refresh must wait for the family lock"
    );

    // Releasing the lock lets the queued rotation finish.
    lock_tx.commit().await.unwrap();
    let response = request.await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[sqlx::test]
async fn logout_waits_for_the_family_lock(pool: PgPool) {
    seed_user(&pool, "root@example.com", "user", true).await;
    let app = test_app(pool.clone());
    let token = sign_in(&app, "root@example.com").await;
    let family_id = family_of(&pool, &token).await;

    let mut lock_tx = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(family_lock_key(family_id))
        .execute(&mut *lock_tx)
        .await
        .unwrap();

    let mut request = Box::pin(app.clone().oneshot(logout_request(Some(&token))));
    assert!(
        tokio::time::timeout(Duration::from_millis(300), &mut request)
            .await
            .is_err(),
        "logout must wait for the family lock"
    );

    lock_tx.commit().await.unwrap();
    let response = request.await.unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

#[sqlx::test]
async fn dev_swagger_documents_refresh_and_logout(pool: PgPool) {
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
    for path in ["/api/auth/refresh", "/api/auth/logout"] {
        assert!(
            doc["paths"][path]["post"].is_object(),
            "openapi.json must document POST {path}: {}",
            doc["paths"]
        );
    }
}
