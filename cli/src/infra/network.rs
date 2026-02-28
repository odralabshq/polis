//! Network infrastructure — implements `NetworkProbe` using `spawn_blocking`.

use anyhow::Result;

use crate::application::ports::NetworkProbe;

/// Production implementation that performs real network checks.
#[allow(dead_code)] // Not yet wired from command handlers
pub struct TokioNetworkProbe;

impl NetworkProbe for TokioNetworkProbe {
    async fn check_tcp_connectivity(&self, host: &str, port: u16) -> Result<bool> {
        let addr = format!("{host}:{port}");
        let result = tokio::task::spawn_blocking(move || {
            use std::time::Duration;
            let addr: std::net::SocketAddr = addr
                .parse()
                .map_err(|e| anyhow::anyhow!("invalid address {addr}: {e}"))?;
            Ok::<bool, anyhow::Error>(
                std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(3)).is_ok(),
            )
        })
        .await
        .map_err(|e| anyhow::anyhow!("spawn_blocking panicked: {e}"))??;
        Ok(result)
    }

    async fn check_dns_resolution(&self, hostname: &str) -> Result<bool> {
        let addr = format!("{hostname}:443");
        let result = tokio::task::spawn_blocking(move || {
            use std::net::ToSocketAddrs;
            Ok::<bool, anyhow::Error>(addr.to_socket_addrs().is_ok())
        })
        .await
        .map_err(|e| anyhow::anyhow!("spawn_blocking panicked: {e}"))??;
        Ok(result)
    }
}
