//! polis Toolbox server entry point.
//!
//! Initialises tracing, loads configuration from environment variables
//! (prefixed with `polis_AGENT_`), connects to Valkey with ACL auth,
//! and starts a Streamable-HTTP MCP server exposing 5 read-only tools.

mod state;
mod tools;

use std::sync::Arc;

use anyhow::{Context, Result};
use axum::http::StatusCode;
use axum_server::tls_rustls::RustlsConfig;
use serde::Deserialize;
use tracing_subscriber::EnvFilter;

use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpService,
};

use crate::state::AppState;
use crate::tools::PolisAgentTools;

// ===================================================================
// Configuration
// ===================================================================

/// Server configuration loaded from environment variables via `envy`.
///
/// Each field maps to `polis_AGENT_<FIELD>`:
///   - `polis_AGENT_LISTEN_ADDR`     (default `0.0.0.0:8080`)
///   - `polis_AGENT_VALKEY_URL`      (default `redis://valkey:6379`)
///   - `polis_AGENT_VALKEY_USER`     (required)
///   - `polis_AGENT_VALKEY_PASS_FILE` (required, path to Docker secret)
///   - `polis_AGENT_TLS_CERT`        (optional, path to TLS cert)
///   - `polis_AGENT_TLS_KEY`         (optional, path to TLS key)
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

    /// Path to file containing ACL password (Docker secret).
    valkey_pass_file: String,

    /// Path to TLS certificate (enables HTTPS when set).
    tls_cert: Option<String>,

    /// Path to TLS private key.
    tls_key: Option<String>,
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
    // 1. Initialise tracing with RUST_LOG env filter.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!("polis-hitl-agent starting");

    // 2. Load configuration from polis_AGENT_* env vars.
    let config: Config = envy::prefixed("polis_AGENT_").from_env().context(
        "failed to load config from polis_AGENT_* env vars \
             (polis_AGENT_VALKEY_USER and polis_AGENT_VALKEY_PASS_FILE \
             are required)",
    )?;

    // 3. Read password from Docker secret file
    let valkey_pass = std::fs::read_to_string(&config.valkey_pass_file)
        .with_context(|| format!("failed to read password from {}", config.valkey_pass_file))?
        .trim()
        .to_string();

    tracing::info!(
        listen_addr = %config.listen_addr,
        valkey_url  = %config.valkey_url,
        valkey_user = %config.valkey_user,
        tls_enabled = config.tls_cert.is_some(),
        "configuration loaded",
    );

    // 4. Create AppState — connects to Valkey with ACL auth and
    //    verifies connectivity via PING (Requirement 3.1-3.4).
    let app_state = AppState::new(&config.valkey_url, &config.valkey_user, &valkey_pass)
        .await
        .context("failed to initialise Valkey connection")?;

    let state = Arc::new(app_state);

    // 4. Build the Streamable-HTTP MCP service.
    //    The factory closure creates a fresh PolisAgentTools per
    //    session, each sharing the same Arc<AppState>.
    let state_for_factory = state.clone();
    let service = StreamableHttpService::new(
        move || Ok(PolisAgentTools::new(state_for_factory.clone())),
        LocalSessionManager::default().into(),
        Default::default(),
    );

    // 5. Compose the axum router:
    //    - `/mcp`    → MCP Streamable-HTTP transport
    //    - `/health` → Docker health-check probe
    let router = axum::Router::new()
        .nest_service("/mcp", service)
        .route("/health", axum::routing::get(health));

    // 6. Bind and serve (TLS or plaintext).
    let addr: std::net::SocketAddr = config
        .listen_addr
        .parse()
        .context("invalid listen address")?;

    if let (Some(cert_path), Some(key_path)) = (&config.tls_cert, &config.tls_key) {
        tracing::info!("TLS enabled — loading cert from {}", cert_path);
        let tls_config = RustlsConfig::from_pem_file(cert_path, key_path)
            .await
            .context("failed to load TLS certificates")?;

        tracing::info!("MCP server ready — https://{}/mcp", config.listen_addr,);

        axum_server::bind_rustls(addr, tls_config)
            .serve(router.into_make_service())
            .await
            .context("HTTPS server error")?;
    } else {
        tracing::info!(
            "MCP server ready — http://{}/mcp (TLS disabled)",
            config.listen_addr,
        );

        let listener = tokio::net::TcpListener::bind(&config.listen_addr)
            .await
            .context("failed to bind TCP listener")?;

        axum::serve(listener, router)
            .with_graceful_shutdown(shutdown_signal())
            .await
            .context("HTTP server error")?;
    }

    tracing::info!("polis-hitl-agent shut down");
    Ok(())
}

/// Wait for SIGINT (Ctrl-C) for graceful shutdown.
async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install Ctrl-C handler");
    tracing::info!("received shutdown signal");
}
