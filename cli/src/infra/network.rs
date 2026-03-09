//! Network infrastructure — implements `NetworkProbe` using `spawn_blocking`.

use anyhow::Result;

use crate::application::ports::NetworkProbe;
use crate::infra::blocking::spawn_blocking_io;

/// Production implementation that performs real network checks.
#[allow(dead_code)] // Not yet wired from command handlers
pub struct TokioNetworkProbe;

impl NetworkProbe for TokioNetworkProbe {
    /// # Errors
    ///
    /// This function will return an error if the underlying operations fail.
    async fn check_tcp_connectivity(&self, host: &str, port: u16) -> Result<bool> {
        let addr = format!("{host}:{port}");
        spawn_blocking_io("tcp connectivity check", move || {
            use std::time::Duration;
            let addr: std::net::SocketAddr = addr
                .parse()
                .map_err(|e| anyhow::anyhow!("invalid address {addr}: {e}"))?;
            Ok::<bool, anyhow::Error>(
                std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(3)).is_ok(),
            )
        })
        .await
    }

    /// # Errors
    ///
    /// This function will return an error if the underlying operations fail.
    async fn check_dns_resolution(&self, hostname: &str) -> Result<bool> {
        let addr = format!("{hostname}:443");
        spawn_blocking_io("dns resolution check", move || {
            use std::net::ToSocketAddrs;
            Ok::<bool, anyhow::Error>(addr.to_socket_addrs().is_ok())
        })
        .await
    }
}
