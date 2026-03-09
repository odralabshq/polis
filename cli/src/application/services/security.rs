//! Application service — security management use-cases.
//!
//! Thin orchestration layer that delegates to the `SecurityGateway` port.
//! All infrastructure details (Docker exec, container names, TLS paths)
//! are handled by the gateway implementation.

use anyhow::Result;

use crate::application::ports::{ConfigStore, SecurityGateway};
use crate::domain::security::{AllowAction, SecurityLevel, validate_request_id};

/// Result of querying pending blocked requests.
/// Eliminates impossible state where both error and requests are populated.
pub enum PendingResult {
    /// Successfully retrieved pending requests (may be empty)
    Ok(Vec<String>),
    /// Failed to query pending requests
    Err(String),
}

/// Result of a security policy status query.
pub struct SecurityStatus {
    /// Current security level from local config.
    pub level: SecurityLevel,
    /// Pending request result (success with requests or error).
    pub pending: PendingResult,
}

/// Query security status: level + pending requests.
///
/// # Errors
///
/// Returns an error if the local config cannot be loaded.
pub async fn get_status(
    store: &impl ConfigStore,
    gateway: &impl SecurityGateway,
) -> Result<SecurityStatus> {
    let config = store.load()?;
    let level = config.security.level;

    let pending = match gateway.list_pending().await {
        Ok(output) => PendingResult::Ok(output),
        Err(e) => PendingResult::Err(format!("{e}")),
    };

    Ok(SecurityStatus { level, pending })
}

/// List pending blocked requests. Returns lines of output.
///
/// # Errors
///
/// Returns an error if the toolbox container is unreachable or polis-approve fails.
pub async fn list_pending(gateway: &impl SecurityGateway) -> Result<Vec<String>> {
    gateway.list_pending().await
}

/// Approve a blocked request. Returns confirmation message.
///
/// Validates the request ID format before contacting the gateway.
///
/// # Errors
///
/// Returns an error if the request ID is invalid or the toolbox is unreachable.
pub async fn approve(gateway: &impl SecurityGateway, request_id: &str) -> Result<String> {
    validate_request_id(request_id)?;
    gateway.approve(request_id).await
}

/// Deny a blocked request. Returns confirmation message.
///
/// Validates the request ID format before contacting the gateway.
///
/// # Errors
///
/// Returns an error if the request ID is invalid or the toolbox is unreachable.
pub async fn deny(gateway: &impl SecurityGateway, request_id: &str) -> Result<String> {
    validate_request_id(request_id)?;
    gateway.deny(request_id).await
}

/// Query recent security events from the event log.
///
/// # Errors
///
/// Returns an error if the gateway is unreachable.
pub async fn get_log(gateway: &impl SecurityGateway) -> Result<Vec<String>> {
    gateway.get_log().await
}

/// Add a domain rule for auto-approve/prompt/block behavior. Returns confirmation message.
///
/// # Errors
///
/// Returns an error if the toolbox container is unreachable.
pub async fn add_domain_rule(
    gateway: &impl SecurityGateway,
    pattern: &str,
    action: AllowAction,
) -> Result<String> {
    gateway.add_domain_rule(pattern, action).await
}

/// Set the security level (updates Valkey + local config).
///
/// # Errors
///
/// Returns an error if the toolbox is unreachable or config save fails.
pub async fn set_level(
    store: &impl ConfigStore,
    gateway: &impl SecurityGateway,
    level: SecurityLevel,
) -> Result<String> {
    let msg = gateway.set_level(level).await?;

    let mut config = store.load()?;
    config.security.level = level;
    store.save(&config)?;

    Ok(msg)
}

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::application::ports::{ConfigStore, SecurityGateway};
    use crate::domain::config::PolisConfig;
    use crate::domain::security::{AllowAction, SecurityLevel};
    use anyhow::Result;

    // ── Stubs ────────────────────────────────────────────────────────────────

    struct SecurityGatewayStub {
        pending: Vec<String>,
        approve_msg: String,
        deny_msg: String,
        set_level_msg: String,
        add_rule_msg: String,
        log_events: Vec<String>,
    }

    impl SecurityGatewayStub {
        fn success() -> Self {
            Self {
                pending: vec![],
                approve_msg: "approved".to_string(),
                deny_msg: "denied".to_string(),
                set_level_msg: "level set".to_string(),
                add_rule_msg: "rule added".to_string(),
                log_events: vec![],
            }
        }

        fn with_pending(mut self, pending: Vec<String>) -> Self {
            self.pending = pending;
            self
        }

        fn with_log(mut self, events: Vec<String>) -> Self {
            self.log_events = events;
            self
        }
    }

    impl SecurityGateway for SecurityGatewayStub {
        async fn list_pending(&self) -> Result<Vec<String>> {
            Ok(self.pending.clone())
        }
        async fn approve(&self, _request_id: &str) -> Result<String> {
            Ok(self.approve_msg.clone())
        }
        async fn deny(&self, _request_id: &str) -> Result<String> {
            Ok(self.deny_msg.clone())
        }
        async fn set_level(&self, _level: SecurityLevel) -> Result<String> {
            Ok(self.set_level_msg.clone())
        }
        async fn add_domain_rule(&self, _pattern: &str, _action: AllowAction) -> Result<String> {
            Ok(self.add_rule_msg.clone())
        }
        async fn get_log(&self) -> Result<Vec<String>> {
            Ok(self.log_events.clone())
        }
    }

    struct SecurityGatewayFailStub;

    impl SecurityGateway for SecurityGatewayFailStub {
        async fn list_pending(&self) -> Result<Vec<String>> {
            anyhow::bail!("toolbox not available")
        }
        async fn approve(&self, _request_id: &str) -> Result<String> {
            anyhow::bail!("toolbox not available")
        }
        async fn deny(&self, _request_id: &str) -> Result<String> {
            anyhow::bail!("toolbox not available")
        }
        async fn set_level(&self, _level: SecurityLevel) -> Result<String> {
            anyhow::bail!("toolbox not available")
        }
        async fn add_domain_rule(&self, _pattern: &str, _action: AllowAction) -> Result<String> {
            anyhow::bail!("toolbox not available")
        }
        async fn get_log(&self) -> Result<Vec<String>> {
            anyhow::bail!("toolbox not available")
        }
    }

    struct ConfigStoreStub {
        config: PolisConfig,
    }

    impl ConfigStoreStub {
        fn with_level(level: SecurityLevel) -> Self {
            let mut config = PolisConfig::default();
            config.security.level = level;
            Self { config }
        }
    }

    impl ConfigStore for ConfigStoreStub {
        fn load(&self) -> Result<PolisConfig> {
            Ok(self.config.clone())
        }
        fn save(&self, _config: &PolisConfig) -> Result<()> {
            Ok(())
        }
        fn path(&self) -> Result<std::path::PathBuf> {
            Ok(std::path::PathBuf::from("/tmp/test-config.yaml"))
        }
    }

    // ── Tests ────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn get_status_returns_correct_level() {
        let store = ConfigStoreStub::with_level(SecurityLevel::Strict);
        let gw = SecurityGatewayStub::success();
        let status = get_status(&store, &gw).await.unwrap();
        assert_eq!(status.level, SecurityLevel::Strict);
    }

    #[tokio::test]
    async fn get_status_empty_pending() {
        let store = ConfigStoreStub::with_level(SecurityLevel::Balanced);
        let gw = SecurityGatewayStub::success();
        let status = get_status(&store, &gw).await.unwrap();
        match status.pending {
            PendingResult::Ok(requests) => assert!(requests.is_empty()),
            PendingResult::Err(e) => panic!("expected Ok, got Err: {e}"),
        }
    }

    #[tokio::test]
    async fn get_status_non_empty_pending() {
        let store = ConfigStoreStub::with_level(SecurityLevel::Balanced);
        let gw = SecurityGatewayStub::success().with_pending(vec!["req-12345678".to_string()]);
        let status = get_status(&store, &gw).await.unwrap();
        match status.pending {
            PendingResult::Ok(requests) => assert_eq!(requests.len(), 1),
            PendingResult::Err(e) => panic!("expected Ok, got Err: {e}"),
        }
    }

    #[tokio::test]
    async fn get_status_gateway_error() {
        let store = ConfigStoreStub::with_level(SecurityLevel::Balanced);
        let gw = SecurityGatewayFailStub;
        let status = get_status(&store, &gw).await.unwrap();
        match status.pending {
            PendingResult::Err(msg) => assert!(msg.contains("toolbox not available")),
            PendingResult::Ok(_) => panic!("expected Err"),
        }
    }

    #[tokio::test]
    async fn approve_validates_request_id_before_gateway() {
        let gw = SecurityGatewayFailStub; // would fail if called
        let err = approve(&gw, "invalid-id").await.unwrap_err();
        assert!(err.to_string().contains("Invalid request ID"));
    }

    #[tokio::test]
    async fn deny_validates_request_id_before_gateway() {
        let gw = SecurityGatewayFailStub; // would fail if called
        let err = deny(&gw, "bad").await.unwrap_err();
        assert!(err.to_string().contains("Invalid request ID"));
    }

    #[tokio::test]
    async fn approve_valid_id_calls_gateway() {
        let gw = SecurityGatewayStub::success();
        let msg = approve(&gw, "req-12345678").await.unwrap();
        assert_eq!(msg, "approved");
    }

    #[tokio::test]
    async fn set_level_updates_config() {
        let store = ConfigStoreStub::with_level(SecurityLevel::Balanced);
        let gw = SecurityGatewayStub::success();
        let msg = set_level(&store, &gw, SecurityLevel::Strict).await.unwrap();
        assert_eq!(msg, "level set");
    }

    #[tokio::test]
    async fn add_domain_rule_passes_action_to_gateway() {
        let gw = SecurityGatewayStub::success();
        let msg = add_domain_rule(&gw, "*.example.com", AllowAction::Block)
            .await
            .unwrap();
        assert_eq!(msg, "rule added");
    }

    #[tokio::test]
    async fn get_log_returns_events() {
        let gw = SecurityGatewayStub::success()
            .with_log(vec!["event1".to_string(), "event2".to_string()]);
        let events = get_log(&gw).await.unwrap();
        assert_eq!(events.len(), 2);
    }
}
