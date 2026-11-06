//! rs-admin backend entrypoint.
//!
//! M0 skeleton (issue #13): tracing, PgPool, startup migration, empty router
//! listening on the configured port. No business behavior lives here yet —
//! auth/users modules and the seed CLI arrive with milestones M1–M3
//! (docs/spec.md §8).

use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::Router;
use axum::http::StatusCode;
use sqlx::postgres::PgPoolOptions;
use tower_http::{compression::CompressionLayer, timeout::TimeoutLayer, trace::TraceLayer};

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let app_env = std::env::var("APP_ENV").unwrap_or_else(|_| "development".into());
    init_tracing(&app_env);

    let database_url = std::env::var("DATABASE_URL").context(
        "DATABASE_URL is not set — copy backend/.env.example to backend/.env \
         and start the database with `docker compose up -d db`",
    )?;
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);

    let pool = PgPoolOptions::new()
        .connect(&database_url)
        .await
        .with_context(|| format!("failed to connect to Postgres ({database_url})"))?;
    tracing::info!(%database_url, "postgres connection pool established");

    // Migrations run at startup; the set is intentionally empty at M0
    // (first real migration lands with M1 / issue #14).
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("failed to run database migrations")?;
    tracing::info!("database migrations applied");

    let app = api_router().layer(TraceLayer::new_for_http());

    let addr: SocketAddr = format!("127.0.0.1:{port}")
        .parse()
        .with_context(|| format!("invalid bind address for port {port}"))?;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    tracing::info!(%addr, app_env, "rs-admin backend listening");

    axum::serve(listener, app).await.context("server error")?;
    Ok(())
}

/// `/api` router. Empty at M0; endpoints arrive with M2 (auth) and M3 (users).
/// Timeout + Compression sit inside the `/api` scope; Trace wraps everything.
fn api_router() -> Router {
    Router::new().nest(
        "/api",
        Router::new()
            .layer(TimeoutLayer::with_status_code(
                StatusCode::REQUEST_TIMEOUT,
                Duration::from_secs(30),
            ))
            .layer(CompressionLayer::new()),
    )
}

/// `RUST_LOG` when set, otherwise `info,sqlx=warn` (spec §9.2).
/// Pretty output in development, JSON in production.
fn init_tracing(app_env: &str) {
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
