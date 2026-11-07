//! rs-admin backend entrypoint.
//!
//! One binary, two faces (spec §1): no arguments starts the API server;
//! `seed-superadmin` is the trusted CLI bootstrap of the first super admin
//! (issue #14 / spec §2.3). Everything downstream — auth (M2), users (M3) —
//! lives in the library crate and hangs off `rs_admin_backend::app_router`.

use std::net::SocketAddr;

use anyhow::{Context, Result};
use axum::Router;
use rs_admin_backend::state::{AppState, JwtKeys};
use rs_admin_backend::{app_router, bootstrap, init_tracing};
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let app_env = std::env::var("APP_ENV").unwrap_or_else(|_| "development".into());
    init_tracing(&app_env);

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => serve(&app_env).await,
        Some("seed-superadmin") => rs_admin_backend::seed::run(&args[1..]).await,
        Some(other) => anyhow::bail!(
            "unknown subcommand `{other}` — available: seed-superadmin \
             (no arguments starts the API server)"
        ),
    }
}

/// The API server path: pool, JWT keys, router, listener.
async fn serve(app_env: &str) -> Result<()> {
    let pool = bootstrap().await?;
    let jwt = JwtKeys::from_env()?;
    let state = AppState { pool, jwt };

    // Swagger only ships outside production (spec §9.2; user story 32).
    let mount_swagger = app_env != "production";

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);

    let app: Router = app_router(state, mount_swagger).layer(TraceLayer::new_for_http());

    let addr: SocketAddr = format!("127.0.0.1:{port}")
        .parse()
        .with_context(|| format!("invalid bind address for port {port}"))?;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;
    tracing::info!(%addr, swagger = mount_swagger, "rs-admin backend listening");

    axum::serve(listener, app).await.context("server error")?;
    Ok(())
}
