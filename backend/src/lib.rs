//! rs-admin backend library.
//!
//! The library target exists so integration tests (ADR 0004) can assemble the
//! same router the server serves and drive it in-process with
//! `tower::ServiceExt::oneshot`. `main.rs` stays a thin CLI dispatcher.

pub mod auth;
pub mod docs;
pub mod error;
pub mod password;
pub mod seed;
pub mod state;
pub mod users;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::IntoResponse;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::{compression::CompressionLayer, timeout::TimeoutLayer};
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::state::AppState;

/// Mask the password of a `scheme://user:password@host/...` URL for logging.
/// URLs without credentials pass through; a password containing a literal
/// `@` over-redacts (safer than the alternative of leaking).
fn redact_credentials(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_owned();
    };
    let rest = &url[scheme_end + 3..];
    let Some(at) = rest.rfind('@') else {
        return url.to_owned();
    };
    let user = rest[..at].split(':').next().unwrap_or("");
    format!("{}://{}:***{}", &url[..scheme_end], user, &rest[at..])
}

/// The shared startup path of both faces: resolve `DATABASE_URL`, connect the
/// pool, apply migrations. Idempotent, so the server and the seed CLI can
/// both run it on every start.
pub async fn bootstrap() -> Result<PgPool> {
    let database_url = std::env::var("DATABASE_URL").context(
        "DATABASE_URL is not set — copy backend/.env.example to backend/.env \
         and start the database with `docker compose up -d db`",
    )?;
    let pool = PgPoolOptions::new()
        .connect(&database_url)
        .await
        .with_context(|| {
            format!(
                "failed to connect to Postgres ({})",
                redact_credentials(&database_url)
            )
        })?;
    tracing::info!(database_url = %redact_credentials(&database_url), "postgres connection pool established");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("failed to run database migrations")?;
    tracing::info!("database migrations applied");
    Ok(pool)
}

/// The assembled application. This is the pre-agreed test seam (ADR 0004):
/// every request the server can serve goes through the Router returned here.
///
/// `mount_swagger` wires Swagger UI + the OpenAPI document — development
/// only (spec §9.2: `APP_ENV=production` mounts nothing).
///
/// `frontend_dist` switches the fallback to same-origin static serving
/// (spec §9.3, issue #21): `ServeDir` over the frontend build with an SPA
/// fallback to `index.html`, so pages and `/api` share one port with no
/// CORS. `None` (the development norm) keeps Vite as the page server and
/// the fallback a plain JSON 404.
pub fn app_router(state: AppState, mount_swagger: bool, frontend_dist: Option<PathBuf>) -> Router {
    // Endpoint registration happens per module; OpenApiRouter::nest folds the
    // /api prefix into both the routes and the OpenAPI paths (spec §5.1), so
    // the document names real URLs.
    // One `.routes(...)` per distinct path: utoipa-axum's multi-handler
    // `routes!` only merges handlers that share a path.
    let auth_routes = OpenApiRouter::<AppState>::new()
        .routes(routes!(auth::login))
        .routes(routes!(auth::session::refresh))
        .routes(routes!(auth::session::logout))
        .routes(routes!(auth::password::change_password));

    let users_routes = OpenApiRouter::<AppState>::new()
        .routes(routes!(users::list, users::create))
        // update and delete share the /users/{id} path; password-reset is
        // its own path. /users/me is static and therefore wins over {id}.
        .routes(routes!(users::me::update_me))
        .routes(routes!(users::manage::update, users::manage::delete))
        .routes(routes!(users::manage::reset_password));

    let (router, openapi) = OpenApiRouter::with_openapi(docs::ApiDoc::openapi())
        .nest("/api", auth_routes)
        .nest("/api", users_routes)
        .split_for_parts();

    let app = match frontend_dist {
        Some(dist) => {
            // Real files win; anything else re-enters the app through
            // index.html so client-side routes survive a hard refresh.
            // `.fallback` (not `not_found_service`) keeps the shell a 200 —
            // `not_found_service` would relabel the same body as a 404.
            let spa = ServeDir::new(&dist).fallback(ServeFile::new(dist.join("index.html")));
            router
                .with_state(state)
                .fallback(move |request: Request<Body>| {
                    let spa = spa.clone();
                    async move {
                        match fallback_kind(request.uri().path()) {
                            // The SPA catch-all must not mask the API contract or
                            // a disabled Swagger: those paths answer the §5.2
                            // error body, never index.html.
                            FallbackKind::Error404 => {
                                crate::error::AppError::NotFound.into_response()
                            }
                            FallbackKind::Spa => {
                                use tower::ServiceExt;
                                spa.oneshot(request).await.into_response()
                            }
                        }
                    }
                })
        }
        None => router.with_state(state).fallback(|| async {
            // Unmatched paths already speak the §5.2 error body.
            crate::error::AppError::NotFound
        }),
    };

    let app = app
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(30),
        ))
        .layer(CompressionLayer::new());

    if mount_swagger {
        return app.merge(
            utoipa_swagger_ui::SwaggerUi::new(SWAGGER_UI_PATH)
                .url(format!("{API_DOCS_PREFIX}/openapi.json"), openapi),
        );
    }
    app
}

/// Where Swagger UI and the OpenAPI document mount in development (spec
/// §9.2) — and the exact prefixes [`fallback_kind`] keeps on the JSON error
/// contract when they are not mounted (`APP_ENV=production`).
const SWAGGER_UI_PATH: &str = "/swagger-ui";
const API_DOCS_PREFIX: &str = "/api-docs";

/// Which side of the same-origin fallback a path belongs to.
enum FallbackKind {
    /// Keep the §5.2 JSON error body: unmatched `/api` paths, plus the
    /// Swagger paths when Swagger is not mounted (`APP_ENV=production`
    /// must 404 there, not serve the SPA shell — issue #21).
    Error404,
    /// Everything else is frontend territory.
    Spa,
}

fn fallback_kind(path: &str) -> FallbackKind {
    // Literal-prefix matching only: axum never normalizes `..`, and
    // ServeDir refuses traversal itself, so no path can dodge into /api.
    for prefix in ["/api", SWAGGER_UI_PATH, API_DOCS_PREFIX] {
        if path == prefix || path.starts_with(&format!("{prefix}/")) {
            return FallbackKind::Error404;
        }
    }
    FallbackKind::Spa
}

/// `RUST_LOG` when set, otherwise `info,sqlx=warn` (spec §9.2).
/// Pretty output in development, JSON in production.
pub fn init_tracing(app_env: &str) {
    use tracing_subscriber::{EnvFilter, fmt, prelude::*};

    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn"));

    match app_env {
        "production" => tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().json())
            .init(),
        _ => tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().pretty())
            .init(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Startup logs must never carry the database password (it sits in
    /// DATABASE_URL, which used to be logged verbatim — an issue the
    /// production baseline made visible, issue #21).
    #[test]
    fn redact_credentials_hides_the_password() {
        let url = "postgres://rs_admin:s3cret-pw@db:5432/rs_admin";
        assert_eq!(
            redact_credentials(url),
            "postgres://rs_admin:***@db:5432/rs_admin"
        );
        // no credentials → unchanged; trailing @ in a password over-redacts
        // rather than leaking
        assert_eq!(
            redact_credentials("postgres://localhost:5432/db"),
            "postgres://localhost:5432/db"
        );
        assert_eq!(
            redact_credentials("postgres://u:p@ss@host/db"),
            "postgres://u:***@host/db"
        );
    }

    /// The SPA fallback's one rule: API- and Swagger-shaped paths keep the
    /// JSON error contract; everything else is frontend territory.
    #[test]
    fn fallback_kind_splits_api_and_swagger_from_the_spa() {
        for path in [
            "/api",
            "/api/",
            "/api/nope",
            "/api/auth/login",
            "/swagger-ui",
            "/swagger-ui/",
            "/api-docs/openapi.json",
        ] {
            assert!(
                matches!(fallback_kind(path), FallbackKind::Error404),
                "{path} must keep the error contract"
            );
        }
        for path in [
            "/",
            "/users",
            "/settings/profile",
            "/apifoo",
            "/assets/app.js",
        ] {
            assert!(
                matches!(fallback_kind(path), FallbackKind::Spa),
                "{path} belongs to the SPA"
            );
        }
    }
}
