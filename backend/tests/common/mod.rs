//! Shared helpers for the integration tests (ADR 0004).
//!
//! Seeding is Rust-parameterized with a pre-computed argon2 hash so the suite
//! pays zero hashing cost (spec §7). Tests speak HTTP at the assembled router
//! (`oneshot`); nothing reaches into handlers.

#![allow(dead_code)] // each test file picks what it needs

use axum::Router;
use axum::body::Body;
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, COOKIE, SET_COOKIE};
use axum::http::{Method, Request, StatusCode};
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

use rs_admin_backend::app_router;
use rs_admin_backend::auth::tokens::REFRESH_COOKIE_NAME;
use rs_admin_backend::state::{AppState, JwtKeys};

/// Test signing secret — ≥32 bytes, known to both issuer and tests.
pub const JWT_SECRET: &[u8] = b"integration-test-secret-32-bytes-xx";

/// The password every seeded user answers to.
pub const TEST_PASSWORD: &str = "login-test-passw0rd";

/// CSRF second line of defense (spec §4.1): refresh and logout must carry
/// this exact header or answer 401.
pub const CSRF_HEADER: &str = "X-Requested-With";
pub const CSRF_VALUE: &str = "XMLHttpRequest";

/// Pre-computed argon2id PHC hash of [`TEST_PASSWORD`] (spec §7: constants,
/// not per-test hashing).
pub const TEST_PASSWORD_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$e9b1BrPWxPjY5E0oH6xP/Q$KSK1haBOn3QzctR3Af+L/UrUFbMvV1i4E+COhFCcxm0";

/// The [`AppState`] behind every test shape — one signing secret, one pool.
pub fn test_state(pool: PgPool) -> AppState {
    AppState {
        pool,
        jwt: JwtKeys::from_secret(JWT_SECRET).expect("test secret is 32+ bytes"),
    }
}

/// The app exactly as the server serves it in development (Swagger mounted,
/// no static dir — Vite owns the pages on the host).
pub fn test_app(pool: PgPool) -> Router {
    app_router(test_state(pool), true, None)
}

/// Insert a user and return its id. Login identifier, fixed display name and
/// [`TEST_PASSWORD`] hash; role and enabled flag are the scenario knobs.
pub async fn seed_user(pool: &PgPool, email: &str, role: &str, is_enabled: bool) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO users (email, display_name, password_hash, role, is_enabled, \
                must_change_password) \
         VALUES ($1, 'Seeded User', $2, $3::user_role, $4, FALSE) RETURNING id",
    )
    .bind(email)
    .bind(TEST_PASSWORD_HASH)
    .bind(role)
    .bind(is_enabled)
    .fetch_one(pool)
    .await
    .expect("seed user")
}

/// A `POST /api/auth/login` request with a JSON body.
pub fn login_request(email: &str, password: &str) -> Request<Body> {
    json_request(
        "/api/auth/login",
        serde_json::json!({ "email": email, "password": password }),
    )
}

/// A JSON request of any method-path-body shape.
pub fn json_request(path: &str, body: serde_json::Value) -> Request<Body> {
    Request::post(path)
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// Read a response body as JSON.
pub async fn body_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    serde_json::from_slice(&bytes).expect("body is JSON")
}

/// The single Set-Cookie header value (panics if absent).
pub fn set_cookie(response: &axum::response::Response) -> String {
    response
        .headers()
        .get(SET_COOKIE)
        .expect("response must carry a Set-Cookie header")
        .to_str()
        .expect("cookie header is ASCII")
        .to_owned()
}

/// Assert the refresh cookie is the §4.1 wire form and return it.
pub fn assert_refresh_cookie(response: &axum::response::Response) -> String {
    let cookie = set_cookie(response);
    assert!(cookie.starts_with("rs_admin_refresh="), "cookie: {cookie}");
    // HttpOnly; Secure; SameSite=Strict; Path=/api/auth; 7-day Max-Age.
    for attribute in [
        "HttpOnly",
        "Secure",
        "SameSite=Strict",
        "Path=/api/auth",
        "Max-Age=604800",
    ] {
        assert!(cookie.contains(attribute), "missing {attribute}: {cookie}");
    }
    cookie
}

/// The refresh plaintext from a login / refresh response's Set-Cookie.
pub fn refresh_token(response: &axum::response::Response) -> String {
    let cookie = set_cookie(response);
    let pair = cookie.split(';').next().unwrap_or_default();
    pair.strip_prefix(&format!("{REFRESH_COOKIE_NAME}="))
        .expect("Set-Cookie must name the refresh cookie")
        .to_owned()
}

/// A cookie-bearing `POST /api/auth/{refresh|logout}` request. `csrf` is the
/// `X-Requested-With` value; `None` omits the header (negative cases).
pub fn auth_request(path: &str, cookie: Option<&str>, csrf: Option<&str>) -> Request<Body> {
    let mut builder = Request::post(path);
    if let Some(value) = csrf {
        builder = builder.header(CSRF_HEADER, value);
    }
    if let Some(value) = cookie {
        builder = builder.header(COOKIE, format!("{REFRESH_COOKIE_NAME}={value}"));
    }
    builder.body(Body::empty()).unwrap()
}

/// A valid `POST /api/auth/refresh` request (CSRF header included).
pub fn refresh_request(cookie: Option<&str>) -> Request<Body> {
    auth_request("/api/auth/refresh", cookie, Some(CSRF_VALUE))
}

/// A valid `POST /api/auth/logout` request (CSRF header included).
pub fn logout_request(cookie: Option<&str>) -> Request<Body> {
    auth_request("/api/auth/logout", cookie, Some(CSRF_VALUE))
}

/// Sign in and return `(access token, refresh plaintext)`.
pub async fn sign_in_session(app: &Router, email: &str) -> (String, String) {
    let response = app
        .clone()
        .oneshot(login_request(email, TEST_PASSWORD))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "sign-in precondition");
    let refresh = refresh_token(&response);
    let body = body_json(response).await;
    let access = body["accessToken"]
        .as_str()
        .expect("login returns an accessToken")
        .to_owned();
    (access, refresh)
}

/// Sign in and return the refresh plaintext.
pub async fn sign_in(app: &Router, email: &str) -> String {
    sign_in_session(app, email).await.1
}

/// A Bearer-authenticated JSON request (body optional).
pub fn authenticated_request(
    method: Method,
    path: &str,
    access: &str,
    body: Option<serde_json::Value>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(AUTHORIZATION, format!("Bearer {access}"));
    let payload = match body {
        Some(body) => {
            builder = builder.header(CONTENT_TYPE, "application/json");
            Body::from(body.to_string())
        }
        None => Body::empty(),
    };
    builder.body(payload).unwrap()
}

/// `GET` with an access token.
pub fn get_with_token(path: &str, access: &str) -> Request<Body> {
    authenticated_request(Method::GET, path, access, None)
}

/// `POST` with an access token and a JSON body.
pub fn post_json_with_token(path: &str, access: &str, body: serde_json::Value) -> Request<Body> {
    authenticated_request(Method::POST, path, access, Some(body))
}

/// Send a Bearer-authenticated management request (any method, optional JSON
/// body) through the assembled router — the shape the lifecycle tests drive
/// `PATCH` / `DELETE` / password-reset with.
pub async fn manage_request(
    app: &Router,
    method: Method,
    path: &str,
    access: &str,
    body: Option<serde_json::Value>,
) -> axum::response::Response {
    app.clone()
        .oneshot(authenticated_request(method, path, access, body))
        .await
        .expect("manage request reaches the router")
}

/// Assert the §5.2 error contract: status + `{code, title}` shape.
pub async fn assert_error_contract(
    response: axum::response::Response,
    status: StatusCode,
    code: &str,
) -> serde_json::Value {
    assert_eq!(response.status(), status, "HTTP status");
    let body = body_json(response).await;
    assert_eq!(body["code"], code, "error code in {body}");
    assert!(
        body["title"]
            .as_str()
            .is_some_and(|title| !title.is_empty()),
        "title is a non-empty English fallback: {body}"
    );
    body
}

/// Decode and verify an access JWT against the test secret; returns claims.
pub fn decode_access(token: &str) -> serde_json::Value {
    jsonwebtoken::decode::<serde_json::Value>(
        token,
        &DecodingKey::from_secret(JWT_SECRET),
        &Validation::new(Algorithm::HS256),
    )
    .expect("access token verifies against the test secret")
    .claims
}
