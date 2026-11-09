//! Users list / create / lifecycle contract tests (issues #17–#18, spec §3 /
//! §5.3 / §5.4).
//!
//! The RBAC matrix, manage-down rule, list query contract, one-time initial
//! password, and the update / delete / password-reset endpoints are asserted
//! at the assembled HTTP boundary (ADR 0004): a request goes in, a status +
//! JSON body come out.

mod common;

use axum::http::{Method, StatusCode};
use chrono::DateTime;
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use common::{
    assert_error_contract, body_json, get_with_token, logout_request, manage_request,
    post_json_with_token, refresh_request, refresh_token, seed_user, sign_in_session, test_app,
};

fn create_body(email: &str, display_name: &str, role: &str) -> serde_json::Value {
    serde_json::json!({
        "email": email,
        "displayName": display_name,
        "role": role,
    })
}

async fn set_created_at(pool: &PgPool, id: Uuid, timestamp: &str) {
    sqlx::query("UPDATE users SET created_at = $1::timestamptz WHERE id = $2")
        .bind(timestamp)
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
}

/// Seed the three tiers and return one access token per role.
async fn three_tier_tokens(app: &axum::Router, pool: &PgPool) -> [(String, String); 3] {
    for (email, role) in [
        ("plain@example.com", "user"),
        ("manager@example.com", "admin"),
        ("root@example.com", "superadmin"),
    ] {
        seed_user(pool, email, role, true).await;
    }
    [
        (
            "user".to_owned(),
            sign_in_session(app, "plain@example.com").await.0,
        ),
        (
            "admin".to_owned(),
            sign_in_session(app, "manager@example.com").await.0,
        ),
        (
            "superadmin".to_owned(),
            sign_in_session(app, "root@example.com").await.0,
        ),
    ]
}

#[sqlx::test]
async fn three_tier_role_matrix_governs_the_users_endpoints(pool: PgPool) {
    let app = test_app(pool.clone());
    let tokens = three_tier_tokens(&app, &pool).await;

    // (role, GET /users, POST role=user, POST role=admin, POST role=superadmin)
    // Manage-down: every created role must be strictly below the actor, so
    // even the superadmin cannot mint another superadmin (spec §3).
    let matrix = [
        ("user", 403, 403, 403, 403),
        ("admin", 200, 201, 403, 403),
        ("superadmin", 200, 201, 201, 403),
    ];

    for (role, get_status, create_user, create_admin, create_superadmin) in matrix {
        let (_, token) = tokens
            .iter()
            .find(|(name, _)| name == role)
            .expect("token for every role");

        let response = app
            .clone()
            .oneshot(get_with_token("/api/users", token))
            .await
            .unwrap();
        assert_eq!(
            response.status().as_u16(),
            get_status,
            "GET /users as {role}"
        );

        for (target_role, expected) in [
            ("user", create_user),
            ("admin", create_admin),
            ("superadmin", create_superadmin),
        ] {
            let email = format!("{role}-creates-{target_role}@example.com");
            let response = app
                .clone()
                .oneshot(post_json_with_token(
                    "/api/users",
                    token,
                    create_body(&email, "Created User", target_role),
                ))
                .await
                .unwrap();
            assert_eq!(
                response.status().as_u16(),
                expected,
                "POST /users role={target_role} as {role}"
            );
        }
    }

    // Only the three permitted creates landed: admin→user, superadmin→user,
    // superadmin→admin; the three seeded accounts are the rest.
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(total, 6, "rejected creates must not insert rows");
}

#[sqlx::test]
async fn users_endpoints_require_a_valid_access_token(pool: PgPool) {
    seed_user(&pool, "manager@example.com", "admin", true).await;
    let app = test_app(pool);

    let missing = axum::http::Request::get("/api/users")
        .body(axum::body::Body::empty())
        .unwrap();
    let malformed = get_with_token("/api/users", "not-a-jwt");

    for request in [missing, malformed] {
        let response = app.clone().oneshot(request).await.unwrap();
        assert_error_contract(response, StatusCode::UNAUTHORIZED, "INVALID_CREDENTIALS").await;
    }

    let missing_create = axum::http::Request::post("/api/users")
        .header("content-type", "application/json")
        .body(axum::body::Body::from(
            create_body("new@example.com", "New", "user").to_string(),
        ))
        .unwrap();
    let response = app.oneshot(missing_create).await.unwrap();
    assert_error_contract(response, StatusCode::UNAUTHORIZED, "INVALID_CREDENTIALS").await;
}

#[sqlx::test]
async fn role_check_precedes_payload_validation(pool: PgPool) {
    // The AdminUser extractor runs before the Query / Json extractors, so an
    // unauthorized caller always gets 403 — never a 422 that describes the
    // request shape.
    seed_user(&pool, "plain@example.com", "user", true).await;
    let app = test_app(pool);
    let (token, _) = sign_in_session(&app, "plain@example.com").await;

    let response = app
        .clone()
        .oneshot(get_with_token("/api/users?sort=bogus", &token))
        .await
        .unwrap();
    assert_error_contract(response, StatusCode::FORBIDDEN, "FORBIDDEN").await;

    let response = app
        .oneshot(post_json_with_token(
            "/api/users",
            &token,
            serde_json::json!({}),
        ))
        .await
        .unwrap();
    assert_error_contract(response, StatusCode::FORBIDDEN, "FORBIDDEN").await;
}

#[sqlx::test]
async fn three_roles_can_use_the_existing_auth_endpoints(pool: PgPool) {
    // The RBAC matrix's auth half: the pre-existing session endpoints are
    // role-agnostic and must stay usable by all three tiers (the users
    // endpoints are covered by the matrix above).
    let app = test_app(pool.clone());

    for (email, role) in [
        ("auth-user@example.com", "user"),
        ("auth-admin@example.com", "admin"),
        ("auth-root@example.com", "superadmin"),
    ] {
        seed_user(&pool, email, role, true).await;

        // sign_in_session asserts the 200 login response.
        let (_, refresh) = sign_in_session(&app, email).await;

        let response = app
            .clone()
            .oneshot(refresh_request(Some(&refresh)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "refresh as {role}");
        let rotated = refresh_token(&response);

        let response = app
            .clone()
            .oneshot(logout_request(Some(&rotated)))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::NO_CONTENT,
            "logout as {role}"
        );
    }
}

#[sqlx::test]
async fn authorization_reads_the_role_claim_not_the_live_row(pool: PgPool) {
    // Spec §3: the role is taken from the JWT claims; there is no per-request
    // DB lookup. A demoted admin keeps admin rights until the token expires;
    // a promoted user does not gain them until their next refresh.
    let admin_id = seed_user(&pool, "manager@example.com", "admin", true).await;
    let user_id = seed_user(&pool, "plain@example.com", "user", true).await;
    let app = test_app(pool.clone());

    let (admin_token, _) = sign_in_session(&app, "manager@example.com").await;
    let (user_token, _) = sign_in_session(&app, "plain@example.com").await;

    sqlx::query("UPDATE users SET role = 'user' WHERE id = $1")
        .bind(admin_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET role = 'superadmin' WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();

    let response = app
        .clone()
        .oneshot(get_with_token("/api/users", &admin_token))
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "the old admin claim still authorizes"
    );

    let response = app
        .oneshot(get_with_token("/api/users", &user_token))
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "the old user claim does not gain admin rights from the DB"
    );
}

#[sqlx::test]
async fn list_returns_data_and_meta_sorted_newest_first(pool: PgPool) {
    let app = test_app(pool.clone());
    let manager = seed_user(&pool, "manager@example.com", "admin", true).await;
    let old = seed_user(&pool, "old@example.com", "user", true).await;
    let new = seed_user(&pool, "new@example.com", "user", true).await;
    set_created_at(&pool, manager, "2026-01-02T00:00:00Z").await;
    set_created_at(&pool, old, "2026-01-01T00:00:00Z").await;
    set_created_at(&pool, new, "2026-01-03T00:00:00Z").await;
    let (token, _) = sign_in_session(&app, "manager@example.com").await;

    let response = app
        .oneshot(get_with_token("/api/users", &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;

    // §5.4: `{data, meta: {total, page, pageSize}}`, default sort createdAt.desc.
    assert_eq!(body["meta"]["total"], 3);
    assert_eq!(body["meta"]["page"], 1);
    assert_eq!(body["meta"]["pageSize"], 10);
    let emails: Vec<_> = body["data"]
        .as_array()
        .expect("data is an array")
        .iter()
        .map(|user| user["email"].as_str().unwrap())
        .collect();
    assert_eq!(
        emails,
        vec!["new@example.com", "manager@example.com", "old@example.com"]
    );

    // §5.1 user shape: camelCase, RFC 3339 timestamps, never the hash.
    for user in body["data"].as_array().unwrap() {
        assert_eq!(user["displayName"], "Seeded User");
        assert!(user["isEnabled"].is_boolean());
        assert!(user["mustChangePassword"].is_boolean());
        assert!(user["role"].is_string());
        for field in ["createdAt", "updatedAt"] {
            DateTime::parse_from_rfc3339(user[field].as_str().unwrap())
                .unwrap_or_else(|err| panic!("{field} must be RFC 3339: {err}"));
        }
        assert!(
            user.get("passwordHash").is_none(),
            "list items must never leak the hash"
        );
    }
}

#[sqlx::test]
async fn list_paginates_and_enforces_page_bounds(pool: PgPool) {
    let app = test_app(pool.clone());
    let admin = seed_user(&pool, "page-admin@example.com", "admin", true).await;
    set_created_at(&pool, admin, "2025-12-31T00:00:00Z").await;
    for (email, timestamp) in [
        ("page-a@example.com", "2026-01-01T00:00:00Z"),
        ("page-b@example.com", "2026-01-02T00:00:00Z"),
        ("page-c@example.com", "2026-01-03T00:00:00Z"),
        ("page-d@example.com", "2026-01-04T00:00:00Z"),
    ] {
        let id = seed_user(&pool, email, "user", true).await;
        set_created_at(&pool, id, timestamp).await;
    }
    let (token, _) = sign_in_session(&app, "page-admin@example.com").await;

    // Newest first: page-d, page-c, page-b, page-a, page-admin. Page 2 of
    // two-row pages therefore holds page-b and page-a.
    let response = app
        .clone()
        .oneshot(get_with_token("/api/users?page=2&pageSize=2", &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["meta"]["total"], 5);
    assert_eq!(body["meta"]["page"], 2);
    assert_eq!(body["meta"]["pageSize"], 2);
    let emails: Vec<_> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|user| user["email"].as_str().unwrap())
        .collect();
    assert_eq!(emails, vec!["page-b@example.com", "page-a@example.com"]);

    let response = app
        .clone()
        .oneshot(get_with_token("/api/users?pageSize=100", &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "pageSize=100 is the max");

    for (query, field, code) in [
        ("page=0", "page", "PAGE_INVALID"),
        ("pageSize=0", "pageSize", "PAGE_SIZE_INVALID"),
        ("pageSize=101", "pageSize", "PAGE_SIZE_INVALID"),
    ] {
        let response = app
            .clone()
            .oneshot(get_with_token(&format!("/api/users?{query}"), &token))
            .await
            .unwrap();
        let body = assert_error_contract(
            response,
            StatusCode::UNPROCESSABLE_ENTITY,
            "VALIDATION_ERROR",
        )
        .await;
        assert_eq!(body["errors"][field][0], code, "query {query}");
    }

    let response = app
        .oneshot(get_with_token("/api/users?page=not-a-number", &token))
        .await
        .unwrap();
    assert_error_contract(
        response,
        StatusCode::UNPROCESSABLE_ENTITY,
        "VALIDATION_ERROR",
    )
    .await;
}

#[sqlx::test]
async fn list_sorts_only_on_the_whitelist(pool: PgPool) {
    let app = test_app(pool.clone());
    seed_user(&pool, "sort-admin@example.com", "admin", true).await;
    for (email, display_name) in [
        ("sort-b@example.com", "Bravo"),
        ("sort-a@example.com", "Alpha"),
        ("sort-c@example.com", "Charlie"),
    ] {
        let id = seed_user(&pool, email, "user", true).await;
        sqlx::query("UPDATE users SET display_name = $1 WHERE id = $2")
            .bind(display_name)
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
    }
    let (token, _) = sign_in_session(&app, "sort-admin@example.com").await;

    for (sort, expected) in [
        (
            "email.asc",
            vec![
                "sort-a@example.com",
                "sort-b@example.com",
                "sort-c@example.com",
            ],
        ),
        (
            "email.desc",
            vec![
                "sort-c@example.com",
                "sort-b@example.com",
                "sort-a@example.com",
            ],
        ),
    ] {
        let response = app
            .clone()
            .oneshot(get_with_token(
                &format!("/api/users?role=user&sort={sort}"),
                &token,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_json(response).await;
        let emails: Vec<_> = body["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|user| user["email"].as_str().unwrap())
            .collect();
        assert_eq!(emails, expected, "sort={sort}");
    }

    // displayName is whitelisted too; the direction defaults to asc when the
    // `.direction` suffix is omitted.
    for (sort, expected) in [
        ("displayName", vec!["Alpha", "Bravo", "Charlie"]),
        ("displayName.desc", vec!["Charlie", "Bravo", "Alpha"]),
    ] {
        let response = app
            .clone()
            .oneshot(get_with_token(
                &format!("/api/users?role=user&sort={sort}"),
                &token,
            ))
            .await
            .unwrap();
        let body = body_json(response).await;
        let names: Vec<_> = body["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|user| user["displayName"].as_str().unwrap())
            .collect();
        assert_eq!(names, expected, "sort={sort}");
    }

    for query in [
        "sort=password_hash.desc",
        "sort=email.sideways",
        "sort=id.asc",
    ] {
        let response = app
            .clone()
            .oneshot(get_with_token(&format!("/api/users?{query}"), &token))
            .await
            .unwrap();
        let body = assert_error_contract(
            response,
            StatusCode::UNPROCESSABLE_ENTITY,
            "VALIDATION_ERROR",
        )
        .await;
        assert_eq!(body["errors"]["sort"][0], "SORT_INVALID", "query {query}");
    }
}

#[sqlx::test]
async fn list_filters_by_role_status_and_fuzzy_search(pool: PgPool) {
    let app = test_app(pool.clone());
    seed_user(&pool, "root@example.com", "superadmin", true).await;
    let alice = seed_user(&pool, "alice@example.com", "admin", true).await;
    let bob = seed_user(&pool, "bob@example.com", "user", false).await;
    let carol = seed_user(&pool, "carol@example.com", "user", true).await;
    let dave = seed_user(&pool, "dave@example.com", "superadmin", true).await;
    let erin = seed_user(&pool, "erin@example.com", "user", true).await;
    for (id, name) in [
        (alice, "Alice Admin"),
        (bob, "Bob Builder"),
        (carol, "Carol"),
        (dave, "a_b"),
        (erin, "axb"),
    ] {
        sqlx::query("UPDATE users SET display_name = $1 WHERE id = $2")
            .bind(name)
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
    }
    let (token, _) = sign_in_session(&app, "root@example.com").await;

    // role is repeatable: role=admin&role=user.
    let response = app
        .clone()
        .oneshot(get_with_token(
            "/api/users?role=admin&role=user&sort=email.asc",
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    let emails: Vec<_> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|user| user["email"].as_str().unwrap())
        .collect();
    assert_eq!(
        emails,
        vec![
            "alice@example.com",
            "bob@example.com",
            "carol@example.com",
            "erin@example.com"
        ],
        "superadmins are filtered out"
    );

    // status is single-valued.
    let response = app
        .clone()
        .oneshot(get_with_token("/api/users?status=disabled", &token))
        .await
        .unwrap();
    let body = body_json(response).await;
    assert_eq!(body["meta"]["total"], 1);
    assert_eq!(body["data"][0]["email"], "bob@example.com");

    // q matches login identifier or display name, case-insensitively.
    for (query, expected_email) in [
        ("q=ALICE", "alice@example.com"),
        ("q=builder", "bob@example.com"),
    ] {
        let response = app
            .clone()
            .oneshot(get_with_token(&format!("/api/users?{query}"), &token))
            .await
            .unwrap();
        let body = body_json(response).await;
        assert_eq!(body["meta"]["total"], 1, "query {query}");
        assert_eq!(body["data"][0]["email"], expected_email, "query {query}");
    }

    // `_` is a literal in q, not a single-character wildcard (§5.4).
    let response = app
        .clone()
        .oneshot(get_with_token("/api/users?q=a_b", &token))
        .await
        .unwrap();
    let body = body_json(response).await;
    assert_eq!(body["meta"]["total"], 1, "underscore must be escaped");
    assert_eq!(body["data"][0]["email"], "dave@example.com");

    // Filters combine AND-wise with pagination.
    let response = app
        .clone()
        .oneshot(get_with_token(
            "/api/users?role=user&status=enabled&sort=email.asc&page=1&pageSize=1",
            &token,
        ))
        .await
        .unwrap();
    let body = body_json(response).await;
    assert_eq!(body["meta"]["total"], 2, "carol and erin; bob is disabled");
    assert_eq!(body["meta"]["pageSize"], 1);
    assert_eq!(body["data"][0]["email"], "carol@example.com");
}

#[sqlx::test]
async fn create_user_returns_a_one_time_initial_password(pool: PgPool) {
    let app = test_app(pool.clone());
    seed_user(&pool, "manager@example.com", "admin", true).await;
    let (token, _) = sign_in_session(&app, "manager@example.com").await;

    let response = app
        .clone()
        .oneshot(post_json_with_token(
            "/api/users",
            &token,
            create_body("new@example.com", "New Person", "user"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = body_json(response).await;

    // 201 {user, initialPassword}; the password is generated, not chosen.
    let password = body["initialPassword"]
        .as_str()
        .expect("initialPassword")
        .to_owned();
    assert_eq!(password.chars().count(), 16, "spec §2.4: 16 characters");
    let user = &body["user"];
    assert_eq!(user["email"], "new@example.com");
    assert_eq!(user["displayName"], "New Person");
    assert_eq!(user["role"], "user");
    assert_eq!(user["isEnabled"], true);
    assert_eq!(
        user["mustChangePassword"], true,
        "the first sign-in must force a password change"
    );
    let user_id = Uuid::parse_str(user["id"].as_str().unwrap()).unwrap();

    // The stored form is an argon2 PHC hash of that one-time password.
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_ne!(hash, password, "plaintext must never be stored");
    assert!(rs_admin_backend::password::verify_password(&password, &hash).unwrap());

    // The user can sign in with it and is flagged for a forced change.
    let response = app
        .clone()
        .oneshot(common::login_request("new@example.com", &password))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let login = body_json(response).await;
    assert_eq!(login["user"]["mustChangePassword"], true);

    // The plaintext appears exactly once: no listing or login response
    // echoes it back.
    let response = app
        .oneshot(get_with_token("/api/users", &token))
        .await
        .unwrap();
    let list_text = body_json(response).await.to_string();
    assert!(
        !list_text.contains(&password),
        "the one-time password must never be readable again"
    );
    assert!(!login.to_string().contains(&password));
}

#[sqlx::test]
async fn create_rejects_duplicate_emails_and_invalid_fields(pool: PgPool) {
    let app = test_app(pool.clone());
    seed_user(&pool, "manager@example.com", "admin", true).await;
    let (token, _) = sign_in_session(&app, "manager@example.com").await;

    let response = app
        .clone()
        .oneshot(post_json_with_token(
            "/api/users",
            &token,
            create_body("taken@example.com", "Taken", "user"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    let response = app
        .clone()
        .oneshot(post_json_with_token(
            "/api/users",
            &token,
            create_body("taken@example.com", "Duplicate", "user"),
        ))
        .await
        .unwrap();
    let body = assert_error_contract(response, StatusCode::CONFLICT, "EMAIL_TAKEN").await;
    assert_eq!(body["errors"]["email"][0], "EMAIL_TAKEN");

    for (case, email, display_name, role, field, code) in [
        (
            "bad email",
            "not-an-email",
            "Valid Name",
            "user",
            "email",
            "EMAIL_INVALID",
        ),
        (
            "empty display name",
            "valid@example.com",
            "   ",
            "user",
            "displayName",
            "DISPLAY_NAME_REQUIRED",
        ),
        (
            "long display name",
            "valid2@example.com",
            &"x".repeat(33),
            "user",
            "displayName",
            "DISPLAY_NAME_TOO_LONG",
        ),
        (
            "unknown role",
            "valid3@example.com",
            "Valid Name",
            "root",
            "role",
            "ROLE_INVALID",
        ),
    ] {
        let response = app
            .clone()
            .oneshot(post_json_with_token(
                "/api/users",
                &token,
                create_body(email, display_name, role),
            ))
            .await
            .unwrap();
        let body = assert_error_contract(
            response,
            StatusCode::UNPROCESSABLE_ENTITY,
            "VALIDATION_ERROR",
        )
        .await;
        assert_eq!(body["errors"][field][0], code, "case: {case}");
    }
}

#[sqlx::test]
async fn superadmin_can_create_an_admin_who_can_then_sign_in(pool: PgPool) {
    let app = test_app(pool.clone());
    seed_user(&pool, "root@example.com", "superadmin", true).await;
    let (token, _) = sign_in_session(&app, "root@example.com").await;

    let response = app
        .clone()
        .oneshot(post_json_with_token(
            "/api/users",
            &token,
            create_body("deputy@example.com", "Deputy", "admin"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = body_json(response).await;
    assert_eq!(body["user"]["role"], "admin");
    let password = body["initialPassword"].as_str().unwrap();

    let response = app
        .oneshot(common::login_request("deputy@example.com", password))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let claims = common::decode_access(body_json(response).await["accessToken"].as_str().unwrap());
    assert_eq!(claims["role"], "admin");
}

#[sqlx::test]
async fn role_matrix_governs_the_manage_endpoints(pool: PgPool) {
    let app = test_app(pool.clone());
    // One target per tier; root (the acting superadmin) keeps a second
    // enabled superadmin alive, so no cell trips the last-superadmin 409 —
    // manage-down alone decides these cells (guardrails: guardrails.rs).
    let target = [
        seed_user(&pool, "t-user@example.com", "user", true).await,
        seed_user(&pool, "t-admin@example.com", "admin", true).await,
        seed_user(&pool, "t-super@example.com", "superadmin", true).await,
    ];
    let tokens = three_tier_tokens(&app, &pool).await;

    // (actor, [patch user/admin/super, reset user/admin/super,
    //          delete user/admin/super])
    // Delete is superadmin-only; everything else is manage-down.
    let matrix = [
        ("user", [403, 403, 403, 403, 403, 403, 403, 403, 403]),
        ("admin", [200, 403, 403, 200, 403, 403, 403, 403, 403]),
        ("superadmin", [200, 200, 403, 200, 200, 403, 204, 204, 403]),
    ];

    for (role, expected) in matrix {
        let (_, token) = tokens
            .iter()
            .find(|(name, _)| name == role)
            .expect("token for every role");

        let ops = [
            (
                Method::PATCH,
                "displayName edit",
                Some(serde_json::json!({
                    "displayName": "Matrix Edit"
                })),
            ),
            (Method::POST, "password reset", None),
            (Method::DELETE, "hard delete", None),
        ];
        let mut cell = 0;
        for (method, what, body) in ops {
            for (tier, id) in target.iter().enumerate() {
                let path = match method {
                    Method::POST => format!("/api/users/{id}/password-reset"),
                    _ => format!("/api/users/{id}"),
                };
                let response =
                    manage_request(&app, method.clone(), &path, token, body.clone()).await;
                let tier_name = ["user", "admin", "superadmin"][tier];
                assert_eq!(
                    response.status().as_u16(),
                    expected[cell],
                    "{what} on {tier_name} as {role}"
                );
                cell += 1;
            }
        }
    }

    // Only the superadmin's two deletes landed; the six seeded accounts and
    // the surviving peer superadmin remain.
    let total: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(total, 4, "three tiers + three targets minus two deletes");
}

#[sqlx::test]
async fn update_applies_each_field_under_its_rules(pool: PgPool) {
    let app = test_app(pool.clone());
    seed_user(&pool, "root@example.com", "superadmin", true).await;
    seed_user(&pool, "manager@example.com", "admin", true).await;
    let target = seed_user(&pool, "t-user@example.com", "user", true).await;
    let peer_admin = seed_user(&pool, "t-admin@example.com", "admin", true).await;
    let (root_token, _) = sign_in_session(&app, "root@example.com").await;
    let (manager_token, _) = sign_in_session(&app, "manager@example.com").await;
    let path = format!("/api/users/{target}");

    // An admin may edit a strictly-lower user's display name (trimmed, and
    // the response carries the full updated row).
    let response = manage_request(
        &app,
        Method::PATCH,
        &path,
        &manager_token,
        Some(serde_json::json!({ "displayName": "  Renamed  " })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["user"]["displayName"], "Renamed");
    assert_eq!(body["user"]["email"], "t-user@example.com");
    assert_eq!(body["user"]["role"], "user");
    assert_eq!(body["user"]["isEnabled"], true);

    // Role assignment is superadmin-exclusive — even a no-op value.
    let response = manage_request(
        &app,
        Method::PATCH,
        &path,
        &manager_token,
        Some(serde_json::json!({ "role": "user" })),
    )
    .await;
    assert_error_contract(response, StatusCode::FORBIDDEN, "FORBIDDEN").await;

    // A superadmin changes roles strictly below their own rank.
    let response = manage_request(
        &app,
        Method::PATCH,
        &path,
        &root_token,
        Some(serde_json::json!({ "role": "admin" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["user"]["role"], "admin");

    let response = manage_request(
        &app,
        Method::PATCH,
        &path,
        &root_token,
        Some(serde_json::json!({ "role": "superadmin" })),
    )
    .await;
    assert_error_contract(response, StatusCode::FORBIDDEN, "FORBIDDEN").await;

    // Manage-down: a peer admin is visible but untouchable.
    let response = manage_request(
        &app,
        Method::PATCH,
        &format!("/api/users/{peer_admin}"),
        &manager_token,
        Some(serde_json::json!({ "displayName": "Nope" })),
    )
    .await;
    assert_error_contract(response, StatusCode::FORBIDDEN, "FORBIDDEN").await;

    // Field validation mirrors create.
    for (case, body, field, code) in [
        (
            "blank display name",
            serde_json::json!({ "displayName": "   " }),
            "displayName",
            "DISPLAY_NAME_REQUIRED",
        ),
        (
            "long display name",
            serde_json::json!({ "displayName": "x".repeat(33) }),
            "displayName",
            "DISPLAY_NAME_TOO_LONG",
        ),
        (
            "unknown role",
            serde_json::json!({ "role": "root" }),
            "role",
            "ROLE_INVALID",
        ),
    ] {
        let response = manage_request(&app, Method::PATCH, &path, &root_token, Some(body)).await;
        let body = assert_error_contract(
            response,
            StatusCode::UNPROCESSABLE_ENTITY,
            "VALIDATION_ERROR",
        )
        .await;
        assert_eq!(body["errors"][field][0], code, "case: {case}");
    }

    // The PATCH vocabulary is closed: unknown fields are ignored, the login
    // identifier is not editable here.
    let response = manage_request(
        &app,
        Method::PATCH,
        &path,
        &root_token,
        Some(serde_json::json!({
            "email": "hijack@example.com",
            "displayName": "Kept",
        })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["user"]["displayName"], "Kept");
    assert_eq!(body["user"]["email"], "t-user@example.com");
}

#[sqlx::test]
async fn manage_endpoints_answer_404_for_unknown_targets(pool: PgPool) {
    let app = test_app(pool.clone());
    seed_user(&pool, "root@example.com", "superadmin", true).await;
    let (token, _) = sign_in_session(&app, "root@example.com").await;
    let unknown = Uuid::new_v4();

    let response = manage_request(
        &app,
        Method::PATCH,
        &format!("/api/users/{unknown}"),
        &token,
        Some(serde_json::json!({ "displayName": "Ghost" })),
    )
    .await;
    assert_error_contract(response, StatusCode::NOT_FOUND, "NOT_FOUND").await;

    let response = manage_request(
        &app,
        Method::DELETE,
        &format!("/api/users/{unknown}"),
        &token,
        None,
    )
    .await;
    assert_error_contract(response, StatusCode::NOT_FOUND, "NOT_FOUND").await;

    let response = manage_request(
        &app,
        Method::POST,
        &format!("/api/users/{unknown}/password-reset"),
        &token,
        None,
    )
    .await;
    assert_error_contract(response, StatusCode::NOT_FOUND, "NOT_FOUND").await;
}

#[sqlx::test]
async fn role_change_reaches_the_access_token_at_the_next_refresh(pool: PgPool) {
    let app = test_app(pool.clone());
    seed_user(&pool, "root@example.com", "superadmin", true).await;
    let target = seed_user(&pool, "t-user@example.com", "user", true).await;
    let (root_token, _) = sign_in_session(&app, "root@example.com").await;
    let (old_access, refresh) = sign_in_session(&app, "t-user@example.com").await;

    let response = manage_request(
        &app,
        Method::PATCH,
        &format!("/api/users/{target}"),
        &root_token,
        Some(serde_json::json!({ "role": "admin" })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    // The unexpired access token keeps its original claim — there is no
    // per-request users lookup (spec §3)…
    let response = app
        .clone()
        .oneshot(get_with_token("/api/users", &old_access))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN, "old user claim");

    // …the next rotation re-reads the row and signs the new role, without
    // having revoked the session (spec §4.2).
    let response = app
        .clone()
        .oneshot(refresh_request(Some(&refresh)))
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "role change keeps sessions"
    );
    let body = body_json(response).await;
    assert_eq!(body["user"]["role"], "admin");
    let claims = common::decode_access(body["accessToken"].as_str().unwrap());
    assert_eq!(
        claims["role"], "admin",
        "the new access token carries the new role"
    );
}

#[sqlx::test]
async fn superadmin_hard_deletes_a_user_and_their_sessions(pool: PgPool) {
    let app = test_app(pool.clone());
    seed_user(&pool, "root@example.com", "superadmin", true).await;
    let target = seed_user(&pool, "t-user@example.com", "user", true).await;
    let (root_token, _) = sign_in_session(&app, "root@example.com").await;
    let (_, refresh) = sign_in_session(&app, "t-user@example.com").await;

    let response = manage_request(
        &app,
        Method::DELETE,
        &format!("/api/users/{target}"),
        &root_token,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    // Hard delete: the login identifier is gone (anti-enumeration keeps the
    // same 401 as a wrong password)…
    let response = app
        .clone()
        .oneshot(common::login_request(
            "t-user@example.com",
            common::TEST_PASSWORD,
        ))
        .await
        .unwrap();
    assert_error_contract(response, StatusCode::UNAUTHORIZED, "INVALID_CREDENTIALS").await;

    // …and every session died with the row (families cascade, spec §2.2).
    let response = app
        .clone()
        .oneshot(refresh_request(Some(&refresh)))
        .await
        .unwrap();
    assert_error_contract(response, StatusCode::UNAUTHORIZED, "REFRESH_INVALID").await;

    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(remaining, 1, "no tombstone row survives a hard delete");
}

#[sqlx::test]
async fn password_reset_reissues_a_one_time_password_and_ends_all_sessions(pool: PgPool) {
    let app = test_app(pool.clone());
    seed_user(&pool, "manager@example.com", "admin", true).await;
    let target = seed_user(&pool, "t-user@example.com", "user", true).await;
    let frozen = seed_user(&pool, "frozen@example.com", "user", false).await;
    let (manager_token, _) = sign_in_session(&app, "manager@example.com").await;

    // Two browsers hold live sessions before the reset.
    let first = common::sign_in(&app, "t-user@example.com").await;
    let second = common::sign_in(&app, "t-user@example.com").await;

    let response = manage_request(
        &app,
        Method::POST,
        &format!("/api/users/{target}/password-reset"),
        &manager_token,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(
        body.as_object().expect("object body").len(),
        1,
        "the body is exactly {{initialPassword}}"
    );
    let initial_password = body["initialPassword"].as_str().expect("initialPassword");
    assert_eq!(initial_password.chars().count(), 16, "spec §2.4");

    // The stored form is an argon2 hash of the one-time password.
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(target)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_ne!(hash, initial_password, "plaintext must never be stored");
    assert!(rs_admin_backend::password::verify_password(initial_password, &hash).unwrap());

    // must_change_password is re-armed; isEnabled is untouched (重置 ≠ 解禁).
    let response = app
        .clone()
        .oneshot(get_with_token("/api/users?q=t-user", &manager_token))
        .await
        .unwrap();
    let body = body_json(response).await;
    assert_eq!(body["data"][0]["mustChangePassword"], true);
    assert_eq!(body["data"][0]["isEnabled"], true);

    // All of the target's sessions are dead.
    for token in [&first, &second] {
        let response = app
            .clone()
            .oneshot(refresh_request(Some(token)))
            .await
            .unwrap();
        assert_error_contract(response, StatusCode::UNAUTHORIZED, "REFRESH_INVALID").await;
    }

    // The old password no longer verifies; the initial password signs in and
    // forces its own replacement.
    let response = app
        .clone()
        .oneshot(common::login_request(
            "t-user@example.com",
            common::TEST_PASSWORD,
        ))
        .await
        .unwrap();
    assert_error_contract(response, StatusCode::UNAUTHORIZED, "INVALID_CREDENTIALS").await;
    let response = app
        .clone()
        .oneshot(common::login_request(
            "t-user@example.com",
            initial_password,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await["user"]["mustChangePassword"],
        true
    );

    // Resetting a disabled user works and keeps them disabled.
    let response = manage_request(
        &app,
        Method::POST,
        &format!("/api/users/{frozen}/password-reset"),
        &manager_token,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = app
        .clone()
        .oneshot(get_with_token("/api/users?q=frozen", &manager_token))
        .await
        .unwrap();
    let body = body_json(response).await;
    assert_eq!(
        body["data"][0]["isEnabled"], false,
        "reset does not un-disable"
    );
    assert_eq!(body["data"][0]["mustChangePassword"], true);
}

#[sqlx::test]
async fn dev_swagger_documents_the_users_endpoints(pool: PgPool) {
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
    for method in ["get", "post"] {
        assert!(
            doc["paths"]["/api/users"][method].is_object(),
            "openapi.json must document {method} /api/users: {}",
            doc["paths"]
        );
    }
    for method in ["patch", "delete"] {
        assert!(
            doc["paths"]["/api/users/{id}"][method].is_object(),
            "openapi.json must document {method} /api/users/{{id}}: {}",
            doc["paths"]
        );
    }
    assert!(
        doc["paths"]["/api/users/{id}/password-reset"]["post"].is_object(),
        "openapi.json must document post /api/users/{{id}}/password-reset: {}",
        doc["paths"]
    );
}
