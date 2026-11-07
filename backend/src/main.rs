//! rs-admin backend entrypoint.
//!
//! One binary, two faces (spec §1): no arguments starts the API server;
//! `seed-superadmin` is the trusted CLI bootstrap of the first super admin
//! (issue #14 / spec §2.3). Everything downstream — auth (M2), users (M3) —
//! hangs off the modules declared here.

mod error;
mod password;
mod seed;

use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::Router;
use axum::http::StatusCode;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tower_http::{compression::CompressionLayer, timeout::TimeoutLayer, trace::TraceLayer};

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let app_env = std::env::var("APP_ENV").unwrap_or_else(|_| "development".into());
    init_tracing(&app_env);

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => serve().await,
        Some("seed-superadmin") => seed::run(&args[1..]).await,
        Some(other) => anyhow::bail!(
            "unknown subcommand `{other}` — available: seed-superadmin \
             (no arguments starts the API server)"
        ),
    }
}

/// The API server path: pool, migrations, router, listener.
async fn serve() -> Result<()> {
    // Held for its lifetime; M2 wires it into the router state (spec §1).
    let _pool = bootstrap().await?;

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);

    let app = api_router().layer(TraceLayer::new_for_http());

    let addr: SocketAddr = format!("127.0.0.1:{port}")
        .parse()
        .with_context(|| format!("invalid bind address for port {port}"))?;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    tracing::info!(%addr, "rs-admin backend listening");

    axum::serve(listener, app).await.context("server error")?;
    Ok(())
}

/// The shared startup path of both faces: resolve `DATABASE_URL`, connect the
/// pool, apply migrations. Idempotent, so the server and the seed CLI can
/// both run it on every start.
pub(crate) async fn bootstrap() -> Result<PgPool> {
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

/// `/api` router. Endpoint-free until M2 (auth) and M3 (users); the only
/// behavior is the JSON 404 fallback, so unmatched API paths already speak
/// the §5.2 error body. Timeout + Compression sit inside the `/api` scope;
/// Trace wraps everything.
fn api_router() -> Router {
    Router::new().nest(
        "/api",
        Router::new()
            .fallback(|| async { crate::error::AppError::NotFound })
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
