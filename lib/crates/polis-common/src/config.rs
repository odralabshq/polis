use serde::Deserialize;
use std::net::SocketAddr;

/// MCP-Agent server configuration
#[derive(Debug, Deserialize)]
pub struct AgentServerConfig {
    /// Listen address (default: 0.0.0.0:8080)
    #[serde(default = "default_agent_addr")]
    pub listen_addr: SocketAddr,

    /// Redis connection URL
    #[serde(default = "default_redis_url")]
    pub redis_url: String,
}

/// MCP-Admin server configuration
#[derive(Debug, Deserialize)]
pub struct AdminServerConfig {
    /// Listen address (default: 127.0.0.1:8765)
    /// ⚠️ SECURITY: MUST be localhost only. The MCP-Admin server MUST validate
    /// at startup that listen_addr.ip().is_loopback() == true and hard-fail
    /// if bound to a non-loopback interface (CWE-1327). This validation
    /// belongs in the server binary, not in this types crate.
    #[serde(default = "default_admin_addr")]
    pub listen_addr: SocketAddr,

    /// Redis connection URL
    #[serde(default = "default_redis_url")]
    pub redis_url: String,
}

fn default_agent_addr() -> SocketAddr {
    "0.0.0.0:8080".parse().unwrap()
}

fn default_admin_addr() -> SocketAddr {
    "127.0.0.1:8765".parse().unwrap()
}

fn default_redis_url() -> String {
    "redis://valkey:6379".to_string()
}

impl Default for AgentServerConfig {
    fn default() -> Self {
        Self {
            listen_addr: default_agent_addr(),
            redis_url: default_redis_url(),
        }
    }
}

impl Default for AdminServerConfig {
    fn default() -> Self {
        Self {
            listen_addr: default_admin_addr(),
            redis_url: default_redis_url(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;

    #[test]
    fn agent_default_listen_addr() {
        let cfg = AgentServerConfig::default();
        assert_eq!(
            cfg.listen_addr,
            "0.0.0.0:8080".parse::<SocketAddr>().unwrap()
        );
    }

    #[test]
    fn agent_default_redis_url() {
        let cfg = AgentServerConfig::default();
        assert_eq!(cfg.redis_url, "redis://valkey:6379");
    }

    #[test]
    fn admin_default_listen_addr() {
        let cfg = AdminServerConfig::default();
        assert_eq!(
            cfg.listen_addr,
            "127.0.0.1:8765".parse::<SocketAddr>().unwrap()
        );
    }

    #[test]
    fn admin_default_listen_addr_is_loopback() {
        let cfg = AdminServerConfig::default();
        assert!(
            cfg.listen_addr.ip().is_loopback(),
            "CWE-1327: admin must bind to loopback"
        );
    }

    #[test]
    fn admin_default_redis_url() {
        let cfg = AdminServerConfig::default();
        assert_eq!(cfg.redis_url, "redis://valkey:6379");
    }
}
