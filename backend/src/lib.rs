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

use std::time::Duration;

use anyhow::{Context, Result};
use axum::Router;
use axum::http::StatusCode;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tower_http::{compression::CompressionLayer, timeout::TimeoutLayer};
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::state::AppState;

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
        .with_context(|| format!("failed to connect to Postgres ({database_url})"))?;
    tracing::info!(%database_url, "postgres connection pool established");

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
pub fn app_router(state: AppState, mount_swagger: bool) -> Router {
    // Endpoint registration happens per module; OpenApiRouter::nest folds the
    // /api prefix into both the routes and the OpenAPI paths (spec §5.1), so
    // the document names real URLs.
    let auth_routes = OpenApiRouter::<AppState>::new().routes(routes!(auth::login));

    let (router, openapi) = OpenApiRouter::with_openapi(docs::ApiDoc::openapi())
        .nest("/api", auth_routes)
        .split_for_parts();

    let app = router.with_state(state).fallback(|| async {
        // Unmatched paths already speak the §5.2 error body.
        crate::error::AppError::NotFound
    });

    let app = app
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(30),
        ))
        .layer(CompressionLayer::new());

    if mount_swagger {
        return app.merge(
            utoipa_swagger_ui::SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", openapi),
        );
    }
    app
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
