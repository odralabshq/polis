//! Unit tests for `polis security` command handler.

use anyhow::Result;
use polis_cli::app::{AppContext, AppFlags, BehaviourFlags, OutputFlags};
use polis_cli::application::ports::SecurityGateway;
use polis_cli::commands::security::SecurityCommand;
use polis_cli::domain::security::{AllowAction, SecurityLevel};
use std::process::ExitCode;

fn app() -> Result<AppContext> {
    AppContext::new(&AppFlags {
        output: OutputFlags {
            no_color: true,
            quiet: true,
            json: false,
        },
        behaviour: BehaviourFlags { yes: true },
    })
}

// ── Mock ──────────────────────────────────────────────────────────────────────

struct MockGateway {
    pending: Vec<String>,
}

impl MockGateway {
    fn empty() -> Self {
        Self { pending: vec![] }
    }
    fn with_pending(pending: Vec<String>) -> Self {
        Self { pending }
    }
}

impl SecurityGateway for MockGateway {
    async fn list_pending(&self) -> Result<Vec<String>> {
        Ok(self.pending.clone())
    }
    async fn approve(&self, _id: &str) -> Result<String> {
        Ok("approved".into())
    }
    async fn deny(&self, _id: &str) -> Result<String> {
        Ok("denied".into())
    }
    async fn set_level(&self, _level: SecurityLevel) -> Result<String> {
        Ok("level set".into())
    }
    async fn add_domain_rule(&self, _pattern: &str, _action: AllowAction) -> Result<String> {
        Ok("rule added".into())
    }
    async fn get_log(&self) -> Result<Vec<String>> {
        Ok(vec!["event1".into()])
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_security_status_returns_success() -> Result<()> {
    let result =
        polis_cli::commands::security::run(&app()?, SecurityCommand::Status, &MockGateway::empty())
            .await?;
    assert_eq!(result, ExitCode::SUCCESS);
    Ok(())
}

#[tokio::test]
async fn test_security_pending_empty_returns_success() -> Result<()> {
    let result = polis_cli::commands::security::run(
        &app()?,
        SecurityCommand::Pending,
        &MockGateway::empty(),
    )
    .await;
    assert!(result.is_ok());
    Ok(())
}

#[tokio::test]
async fn test_security_pending_non_empty_returns_success() -> Result<()> {
    let gw = MockGateway::with_pending(vec!["req-aabbccdd pending example.com".into()]);
    let result = polis_cli::commands::security::run(&app()?, SecurityCommand::Pending, &gw).await;
    assert!(result.is_ok());
    Ok(())
}

#[tokio::test]
async fn test_security_approve_valid_id_returns_success() -> Result<()> {
    let result = polis_cli::commands::security::run(
        &app()?,
        SecurityCommand::Approve {
            request_id: "req-aabbccdd".into(),
        },
        &MockGateway::empty(),
    )
    .await;
    assert!(result.is_ok());
    Ok(())
}

#[tokio::test]
async fn test_security_approve_invalid_id_returns_err() -> Result<()> {
    let result = polis_cli::commands::security::run(
        &app()?,
        SecurityCommand::Approve {
            request_id: "bad-id".into(),
        },
        &MockGateway::empty(),
    )
    .await;
    assert!(result.is_err());
    assert!(
        result
            .err()
            .is_some_and(|e| e.to_string().contains("Invalid request ID"))
    );
    Ok(())
}

#[tokio::test]
async fn test_security_deny_valid_id_returns_success() -> Result<()> {
    let result = polis_cli::commands::security::run(
        &app()?,
        SecurityCommand::Deny {
            request_id: "req-aabbccdd".into(),
        },
        &MockGateway::empty(),
    )
    .await;
    assert!(result.is_ok());
    Ok(())
}

#[tokio::test]
async fn test_security_deny_invalid_id_returns_err() -> Result<()> {
    let result = polis_cli::commands::security::run(
        &app()?,
        SecurityCommand::Deny {
            request_id: "not-a-req-id".into(),
        },
        &MockGateway::empty(),
    )
    .await;
    assert!(result.is_err());
    Ok(())
}

#[tokio::test]
async fn test_security_log_returns_success() -> Result<()> {
    let result =
        polis_cli::commands::security::run(&app()?, SecurityCommand::Log, &MockGateway::empty())
            .await;
    assert!(result.is_ok());
    Ok(())
}

#[tokio::test]
async fn test_security_rule_allow_returns_success() -> Result<()> {
    let result = polis_cli::commands::security::run(
        &app()?,
        SecurityCommand::Rule {
            pattern: "*.example.com".into(),
            action: AllowAction::Allow,
        },
        &MockGateway::empty(),
    )
    .await;
    assert!(result.is_ok());
    Ok(())
}

#[tokio::test]
async fn test_security_rule_block_returns_success() -> Result<()> {
    let result = polis_cli::commands::security::run(
        &app()?,
        SecurityCommand::Rule {
            pattern: "evil.com".into(),
            action: AllowAction::Block,
        },
        &MockGateway::empty(),
    )
    .await;
    assert!(result.is_ok());
    Ok(())
}

#[tokio::test]
async fn test_security_level_returns_success() -> Result<()> {
    // YamlConfigStore writes to ~/.polis/config.yaml; acceptable in a dev/CI environment.
    let result = polis_cli::commands::security::run(
        &app()?,
        SecurityCommand::Level {
            level: SecurityLevel::Strict,
        },
        &MockGateway::empty(),
    )
    .await;
    assert!(result.is_ok());
    Ok(())
}
