//! Same-origin static serving (issue #21, spec §9.3).
//!
//! In the production shape the backend serves the frontend build itself:
//! `FRONTEND_DIST` points at a Vite output directory and the router's
//! fallback becomes `ServeDir` with an SPA fallback to `index.html`. The API
//! contract must survive that catch-all — unmatched `/api` paths and the
//! unmounted Swagger paths keep answering the §5.2 JSON error body, never
//! `index.html`.

mod common;

use std::fs;

use axum::Router;
use axum::body::Body;
use axum::http::header::CONTENT_TYPE;
use axum::http::{Method, Request, StatusCode, header};
use common::{TEST_PASSWORD, body_json, login_request, seed_user, test_state};
use rs_admin_backend::app_router;
use sqlx::PgPool;
use tower::ServiceExt;

/// Marker baked into every fixture `index.html` — responses prove they are
/// the SPA shell, not some other handler's body.
const INDEX_MARKER: &str = "<!-- rs-admin spa shell -->";

/// A dist-like directory: SPA shell + one hashed asset per content type.
fn dist_fixture(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rs-admin-static-{tag}-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(dir.join("assets")).expect("create dist dirs");
    fs::write(
        dir.join("index.html"),
        format!("<!doctype html><html><body>{INDEX_MARKER}</body></html>"),
    )
    .expect("write index.html");
    fs::write(dir.join("assets").join("app-K1f2.js"), "console.log('app')").expect("write js");
    fs::write(dir.join("assets").join("app-K1f2.css"), "body{}").expect("write css");
    dir
}

/// The production shape: no Swagger, frontend dist served same-origin.
fn prod_app(pool: PgPool) -> Router {
    app_router(test_state(pool), false, Some(dist_fixture("prod")))
}

async fn get(app: &Router, path: &str) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(path)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("request reaches the router")
}

async fn body_text(response: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    String::from_utf8(bytes.to_vec()).expect("body is UTF-8")
}

/// The SPA shell answer: 200, HTML, and the marker proving it is index.html.
async fn assert_spa_shell(response: axum::response::Response, path: &str) {
    assert_eq!(response.status(), StatusCode::OK, "GET {path}");
    assert!(
        response.headers()[CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/html"),
        "GET {path} answers as HTML"
    );
    assert!(
        body_text(response).await.contains(INDEX_MARKER),
        "GET {path} serves the SPA shell"
    );
}

#[sqlx::test]
async fn root_serves_the_spa_shell(pool: PgPool) {
    assert_spa_shell(get(&prod_app(pool), "/").await, "/").await;
}

#[sqlx::test]
async fn deep_links_fall_back_to_the_spa_shell(pool: PgPool) {
    // /users is a client-side route: hard-refreshing it must re-enter the app
    // (issue #21: SPA 回退), both at the root and nested depth.
    let app = prod_app(pool);
    for path in ["/users", "/settings/profile", "/some/never/built/route"] {
        assert_spa_shell(get(&app, path).await, path).await;
    }
}

#[sqlx::test]
async fn built_assets_are_served_with_their_real_content_types(pool: PgPool) {
    let app = prod_app(pool);

    let js = get(&app, "/assets/app-K1f2.js").await;
    assert_eq!(js.status(), StatusCode::OK);
    assert!(
        js.headers()[CONTENT_TYPE]
            .to_str()
            .unwrap()
            .contains("javascript"),
        "js asset: {:?}",
        js.headers()[CONTENT_TYPE]
    );
    assert_eq!(body_text(js).await, "console.log('app')");

    let css = get(&app, "/assets/app-K1f2.css").await;
    assert_eq!(css.status(), StatusCode::OK);
    assert!(
        css.headers()[CONTENT_TYPE]
            .to_str()
            .unwrap()
            .contains("css"),
        "css asset: {:?}",
        css.headers()[CONTENT_TYPE]
    );
}

#[sqlx::test]
async fn unmatched_api_paths_keep_the_json_error_contract(pool: PgPool) {
    let app = prod_app(pool);

    for path in ["/api", "/api/", "/api/nope", "/api/auth/nope"] {
        let response = get(&app, path).await;
        assert!(
            response.headers()[CONTENT_TYPE]
                .to_str()
                .unwrap()
                .starts_with("application/json"),
            "GET {path} must answer JSON, not the SPA shell"
        );
        common::assert_error_contract(response, StatusCode::NOT_FOUND, "NOT_FOUND").await;
    }

    // The catch-all must not swallow methods either: a JSON POST to a dead
    // API path still speaks the error contract.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/nope")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn swagger_paths_404_when_swagger_is_not_mounted(pool: PgPool) {
    // APP_ENV=production mounts nothing (spec §9.2): the Swagger paths must
    // be a real 404, not a masked 200 index.html (issue #21 AC).
    let app = prod_app(pool);
    for path in ["/swagger-ui", "/swagger-ui/", "/api-docs/openapi.json"] {
        common::assert_error_contract(get(&app, path).await, StatusCode::NOT_FOUND, "NOT_FOUND")
            .await;
    }
}

#[sqlx::test]
async fn swagger_is_served_when_mounted_even_alongside_static(pool: PgPool) {
    // Development shape keeps Swagger (spec §9.2) — and mounting it wins over
    // the SPA catch-all for its own paths.
    let app = app_router(test_state(pool), true, Some(dist_fixture("dev")));
    let response = get(&app, "/swagger-ui").await;
    assert!(
        response.status().is_success() || response.status().is_redirection(),
        "swagger-ui is mounted, got {}",
        response.status()
    );
    let response = get(&app, "/api-docs/openapi.json").await;
    assert_eq!(response.status(), StatusCode::OK, "openapi document");
}

#[sqlx::test]
async fn without_a_dist_unmatched_paths_remain_json_404s(pool: PgPool) {
    // Development on the host: Vite owns the pages, FRONTEND_DIST is unset,
    // and the router's fallback stays the JSON 404 it always was.
    let app = common::test_app(pool);
    for path in ["/", "/users"] {
        let response = get(&app, path).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "GET {path}");
        let body = body_json(response).await;
        assert_eq!(body["code"], "NOT_FOUND");
    }
}

#[sqlx::test]
async fn login_works_through_the_same_origin_router(pool: PgPool) {
    // Pages and /api share one port and one router (ADR 0001: 同源, no CORS).
    // The full auth flow must behave identically behind the static fallback.
    seed_user(&pool, "admin@example.com", "superadmin", true).await;
    let app = prod_app(pool);

    let response = app
        .clone()
        .oneshot(login_request("admin@example.com", TEST_PASSWORD))
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "login through same-origin router"
    );
    assert!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none(),
        "same origin needs no CORS headers"
    );
    let body = body_json(response).await;
    assert!(body["accessToken"].as_str().is_some(), "login body: {body}");
}
