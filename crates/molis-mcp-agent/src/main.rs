//! polis MCP-Agent server entry point.
//!
//! Initialises tracing, loads configuration from environment variables
//! (prefixed with `polis_AGENT_`), connects to Valkey with ACL auth,
//! and starts a Streamable-HTTP MCP server exposing 5 read-only tools.

mod state;
mod tools;

use std::sync::Arc;

use anyhow::{Context, Result};
use axum::http::StatusCode;
use serde::Deserialize;
use tracing_subscriber::EnvFilter;

use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpService,
};

use crate::state::AppState;
use crate::tools::polisAgentTools;

// ===================================================================
// Configuration
// ===================================================================

/// Server configuration loaded from environment variables via `envy`.
///
/// Each field maps to `polis_AGENT_<FIELD>`:
///   - `polis_AGENT_LISTEN_ADDR`  (default `0.0.0.0:8080`)
///   - `polis_AGENT_VALKEY_URL`   (default `redis://valkey:6379`)
///   - `polis_AGENT_VALKEY_USER`  (required)
///   - `polis_AGENT_VALKEY_PASS`  (required)
#[derive(Debug, Deserialize)]
struct Config {
    /// Socket address to bind the HTTP server to.
    #[serde(default = "default_listen_addr")]
    listen_addr: String,

    /// Valkey (Redis-compatible) connection URL.
    #[serde(default = "default_valkey_url")]
    valkey_url: String,

    /// ACL username for Valkey authentication.
    valkey_user: String,

    /// ACL password for Valkey authentication.
    valkey_pass: String,
}

fn default_listen_addr() -> String {
    "0.0.0.0:8080".to_string()
}

fn default_valkey_url() -> String {
    "redis://valkey:6379".to_string()
}

// ===================================================================
// Health endpoint
// ===================================================================

/// Minimal health-check handler for Docker / load-balancer probes.
async fn health() -> StatusCode {
    StatusCode::OK
}

// ===================================================================
// Entry point
// ===================================================================

#[tokio::main]
async fn main() -> Result<()> {
    // Install the default crypto provider (ring) for rustls 0.23+
    // Ignore error if already installed (e.g. by other dep), but ideally we do it first.
    let _ = rustls::crypto::ring::default_provider().install_default();

    // 1. Initialise tracing with RUST_LOG env filter.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!("polis-mcp-agent starting");

    // 2. Load configuration from polis_AGENT_* env vars.
    let config: Config = envy::prefixed("polis_AGENT_")
        .from_env()
        .context(
            "failed to load config from polis_AGENT_* env vars \
             (polis_AGENT_VALKEY_USER and polis_AGENT_VALKEY_PASS \
             are required)",
        )?;

    tracing::info!(
        listen_addr = %config.listen_addr,
        valkey_url  = %config.valkey_url,
        valkey_user = %config.valkey_user,
        "configuration loaded",
    );

    // 3. Create AppState — connects to Valkey with ACL auth and
    //    verifies connectivity via PING (Requirement 3.1-3.4).
    let app_state = AppState::new(
        &config.valkey_url,
        &config.valkey_user,
        &config.valkey_pass,
    )
    .await
    .context("failed to initialise Valkey connection")?;

    let state = Arc::new(app_state);

    // 4. Build the Streamable-HTTP MCP service.
    //    The factory closure creates a fresh polisAgentTools per
    //    session, each sharing the same Arc<AppState>.
    let state_for_factory = state.clone();
    let service = StreamableHttpService::new(
        move || Ok(polisAgentTools::new(state_for_factory.clone())),
        LocalSessionManager::default().into(),
        Default::default(),
    );

    // 5. Compose the axum router:
    //    - `/mcp`    → MCP Streamable-HTTP transport
    //    - `/health` → Docker health-check probe
    let router = axum::Router::new()
        .nest_service("/mcp", service)
        .route("/health", axum::routing::get(health));

    // 6. Bind and serve.
    tracing::info!(
        listen_addr = %config.listen_addr,
        "starting Streamable-HTTP MCP server",
    );

    let listener = tokio::net::TcpListener::bind(&config.listen_addr)
        .await
        .context("failed to bind TCP listener")?;

    tracing::info!(
        "MCP server ready — http://{}/mcp",
        config.listen_addr,
    );
    tracing::info!(
        "Health check — http://{}/health",
        config.listen_addr,
    );

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("HTTP server error")?;

    tracing::info!("polis-mcp-agent shut down");
    Ok(())
}

/// Wait for SIGINT (Ctrl-C) for graceful shutdown.
async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install Ctrl-C handler");
    tracing::info!("received shutdown signal");
}
