use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use polis_common::{AutoApproveAction, SecurityLevel};
use redis::AsyncCommands;
use std::time::{SystemTime, UNIX_EPOCH};

/// Default paths for Valkey TLS certificates inside the toolbox container.
/// These match the volume mount `./certs/valkey:/etc/valkey/tls:ro` in docker-compose.yml.
const DEFAULT_TLS_CA: &str = "/etc/valkey/tls/ca.crt";
const DEFAULT_TLS_CERT: &str = "/etc/valkey/tls/client.crt";
const DEFAULT_TLS_KEY: &str = "/etc/valkey/tls/client.key";

/// polis HITL approval CLI tool.
///
/// Manages blocked-request approvals, security levels, and auto-approve
/// rules via a TLS-secured Valkey connection authenticated as `mcp-admin`.
#[derive(Parser, Debug)]
#[command(name = "polis-approve", version, about)]
struct Cli {
    /// Valkey URL (must use rediss:// for TLS)
    #[arg(long, default_value = "rediss://valkey:6379")]
    valkey_url: String,

    /// Valkey ACL username
    #[arg(long, default_value = "mcp-admin")]
    valkey_user: String,

    /// Valkey ACL password — loaded from polis_VALKEY_PASS env var (CWE-214).
    /// Never passed as a CLI argument.
    #[arg(skip)]
    valkey_pass: String,

    /// Path to the CA certificate (PEM) for Valkey TLS verification
    #[arg(long, default_value = DEFAULT_TLS_CA)]
    tls_ca: String,

    /// Path to the client certificate (PEM) for Valkey mTLS
    #[arg(long, default_value = DEFAULT_TLS_CERT)]
    tls_cert: String,

    /// Path to the client private key (PEM) for Valkey mTLS
    #[arg(long, default_value = DEFAULT_TLS_KEY)]
    tls_key: String,

    #[command(subcommand)]
    command: Commands,
}

/// Available subcommands for the approval CLI.
#[derive(Subcommand, Debug)]
enum Commands {
    /// Approve a blocked request by its request_id
    Approve {
        /// The request ID to approve (format: req-[a-f0-9]{8})
        request_id: String,
    },
    /// Deny a blocked request by its request_id
    Deny {
        /// The request ID to deny (format: req-[a-f0-9]{8})
        request_id: String,
    },
    /// List all pending (blocked) requests
    ListPending,
    /// Set the global security level
    SetSecurityLevel {
        /// Security level: relaxed, balanced, or strict
        level: String,
    },
    /// Configure an auto-approve rule for a destination pattern
    AutoApprove {
        /// Destination pattern to match (e.g., "*.example.com")
        pattern: String,
        /// Action to take: allow, prompt, or block
        action: String,
    },
    /// Persistently allow a credential fingerprint for the blocked request destination
    AllowCredential {
        /// The request ID to allow (format: req-[a-f0-9]{8})
        request_id: String,
    },
    /// Add a runtime bypass domain based on a blocked request
    BypassDomain {
        /// The request ID whose destination should be bypassed
        request_id: String,
    },
    /// List persistent credential allow rules
    ListCredentialAllows,
    /// Delete a persistent credential allow rule
    DeleteCredentialAllow {
        /// Credential pattern name (for example: aws_access)
        pattern: String,
        /// Host name used by the rule
        host: String,
        /// 16-hex credential fingerprint
        fingerprint: String,
    },
}

/// Parse a string into a [`SecurityLevel`], case-insensitive.
fn parse_security_level(s: &str) -> Result<SecurityLevel> {
    match s.to_lowercase().as_str() {
        "relaxed" => Ok(SecurityLevel::Relaxed),
        "balanced" => Ok(SecurityLevel::Balanced),
        "strict" => Ok(SecurityLevel::Strict),
        other => bail!(
            "invalid security level '{}': expected relaxed, balanced, or strict",
            other
        ),
    }
}

/// Parse a string into an [`AutoApproveAction`], case-insensitive.
fn parse_auto_approve_action(s: &str) -> Result<AutoApproveAction> {
    match s.to_lowercase().as_str() {
        "allow" => Ok(AutoApproveAction::Allow),
        "prompt" => Ok(AutoApproveAction::Prompt),
        "block" => Ok(AutoApproveAction::Block),
        other => bail!(
            "invalid auto-approve action '{}': expected allow, prompt, or block",
            other
        ),
    }
}

fn blocked_request_host(blocked_request: &polis_common::BlockedRequest) -> Result<String> {
    polis_common::normalize_approval_host(&blocked_request.destination)
        .map_err(|message| anyhow::anyhow!(message))
}

fn blocked_request_credential_context(
    blocked_request: &polis_common::BlockedRequest,
) -> Result<(String, String, String)> {
    if blocked_request.reason != polis_common::BlockReason::CredentialDetected {
        bail!("blocked request is not a credential-detected item");
    }

    let pattern = blocked_request
        .pattern
        .clone()
        .ok_or_else(|| anyhow::anyhow!("blocked credential cannot be approved at runtime"))?;
    let fingerprint = blocked_request
        .fingerprint
        .clone()
        .ok_or_else(|| anyhow::anyhow!("blocked credential cannot be approved at runtime"))?;
    let host = blocked_request_host(blocked_request)?;
    Ok((pattern, fingerprint, host))
}

/// Fetch blocked request data after validating the request ID.
/// Returns (blocked_key, blocked_data, timestamp) on success.
async fn fetch_blocked(
    con: &mut redis::aio::MultiplexedConnection,
    request_id: &str,
) -> Result<(String, String, u64)> {
    polis_common::validate_request_id(request_id).map_err(|e| anyhow::anyhow!(e))?;

    let blocked_key = polis_common::blocked_key(request_id);
    let blocked_data: Option<String> = con
        .get(&blocked_key)
        .await
        .context("failed to GET blocked request")?;
    let blocked_data = blocked_data
        .ok_or_else(|| anyhow::anyhow!("no blocked request found for {}", request_id))?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock error")?
        .as_secs();

    Ok((blocked_key, blocked_data, now))
}

fn queue_audit_entry(
    pipeline: &mut redis::Pipeline,
    event_type: &str,
    request_id: &str,
    blocked_data: &str,
    timestamp: u64,
) {
    let audit_entry = serde_json::json!({
        "event_type": event_type,
        "request_id": request_id,
        "timestamp": timestamp,
        "blocked_request": blocked_data,
    });
    pipeline
        .cmd("ZADD")
        .arg(polis_common::keys::EVENT_LOG)
        .arg(timestamp as f64)
        .arg(audit_entry.to_string())
        .ignore();
}

async fn handle_approve(
    con: &mut redis::aio::MultiplexedConnection,
    request_id: &str,
) -> Result<()> {
    let (blocked_key, blocked_data, now) = fetch_blocked(con, request_id).await?;
    let approved_key = polis_common::approved_key(request_id);
    let blocked_request = serde_json::from_str::<polis_common::BlockedRequest>(&blocked_data)
        .context("failed to parse blocked request")?;
    let approval_target = if blocked_request.reason == polis_common::BlockReason::CredentialDetected
    {
        let (pattern, fingerprint, host) = blocked_request_credential_context(&blocked_request)?;
        Some((
            polis_common::approved_fingerprint_key(&pattern, &host, &fingerprint)
                .map_err(|message| anyhow::anyhow!(message))?,
            "approved".to_string(),
        ))
    } else if !blocked_request.destination.is_empty() {
        Some((
            polis_common::approved_host_key(&blocked_request_host(&blocked_request)?),
            "1".to_string(),
        ))
    } else {
        None
    };

    let mut pipeline = redis::pipe();
    pipeline
        .atomic()
        .cmd("DEL")
        .arg(&blocked_key)
        .ignore()
        .cmd("SETEX")
        .arg(&approved_key)
        .arg(polis_common::ttl::APPROVED_REQUEST_SECS)
        .arg("approved")
        .ignore();
    if let Some((key, value)) = approval_target {
        pipeline
            .cmd("SETEX")
            .arg(&key)
            .arg(polis_common::ttl::APPROVED_REQUEST_SECS)
            .arg(value)
            .ignore();
    }
    queue_audit_entry(
        &mut pipeline,
        "approved_via_cli",
        request_id,
        &blocked_data,
        now,
    );
    pipeline
        .query_async::<()>(con)
        .await
        .context("failed to atomically approve blocked request")?;

    println!("approved {}", request_id);
    Ok(())
}

async fn handle_allow_credential(
    con: &mut redis::aio::MultiplexedConnection,
    request_id: &str,
) -> Result<()> {
    let (blocked_key, blocked_data, now) = fetch_blocked(con, request_id).await?;
    let blocked_request = serde_json::from_str::<polis_common::BlockedRequest>(&blocked_data)
        .context("failed to parse blocked request")?;
    let (pattern, fingerprint, host) = blocked_request_credential_context(&blocked_request)?;
    let allow_key = polis_common::credential_allow_key(&pattern, &host, &fingerprint)
        .map_err(|message| anyhow::anyhow!(message))?;

    let mut pipeline = redis::pipe();
    pipeline
        .atomic()
        .cmd("DEL")
        .arg(&blocked_key)
        .ignore()
        .cmd("SET")
        .arg(&allow_key)
        .arg("1")
        .ignore();
    queue_audit_entry(
        &mut pipeline,
        "credential_allowed_via_cli",
        request_id,
        &blocked_data,
        now,
    );
    pipeline
        .query_async::<()>(con)
        .await
        .context("failed to atomically create credential allow rule")?;

    println!(
        "remembered credential allow: {} {} {}",
        pattern, host, fingerprint
    );
    Ok(())
}

async fn handle_bypass_domain(
    con: &mut redis::aio::MultiplexedConnection,
    request_id: &str,
) -> Result<()> {
    let (blocked_key, blocked_data, now) = fetch_blocked(con, request_id).await?;
    let blocked_request = serde_json::from_str::<polis_common::BlockedRequest>(&blocked_data)
        .context("failed to parse blocked request")?;
    let host = blocked_request_host(&blocked_request)?;
    let bypass_key = format!("polis:config:bypass:{host}");

    let mut pipeline = redis::pipe();
    pipeline
        .atomic()
        .cmd("DEL")
        .arg(&blocked_key)
        .ignore()
        .cmd("SET")
        .arg(&bypass_key)
        .arg("bypass")
        .ignore();
    queue_audit_entry(
        &mut pipeline,
        "bypass_domain_via_cli",
        request_id,
        &blocked_data,
        now,
    );
    pipeline
        .query_async::<()>(con)
        .await
        .context("failed to atomically add bypass domain")?;

    println!("added bypass domain {}", host);
    Ok(())
}

async fn handle_deny(con: &mut redis::aio::MultiplexedConnection, request_id: &str) -> Result<()> {
    let (blocked_key, blocked_data, now) = fetch_blocked(con, request_id).await?;
    let mut pipeline = redis::pipe();
    pipeline.atomic().cmd("DEL").arg(&blocked_key).ignore();
    queue_audit_entry(
        &mut pipeline,
        "denied_via_cli",
        request_id,
        &blocked_data,
        now,
    );
    pipeline
        .query_async::<()>(con)
        .await
        .context("failed to atomically deny blocked request")?;

    println!("denied {}", request_id);
    Ok(())
}

async fn handle_list_pending(con: &mut redis::aio::MultiplexedConnection) -> Result<()> {
    let match_pattern = format!("{}:*", polis_common::keys::BLOCKED);
    let mut cursor: u64 = 0;
    let mut found = 0u64;

    loop {
        let (next_cursor, batch): (u64, Vec<String>) = redis::cmd("SCAN")
            .arg(cursor)
            .arg("MATCH")
            .arg(&match_pattern)
            .arg("COUNT")
            .arg(100)
            .query_async(con)
            .await
            .context("failed to SCAN blocked keys")?;

        for key in &batch {
            if let Some(data) = con
                .get::<_, Option<String>>(key)
                .await
                .context("failed to GET blocked request")?
            {
                println!("{}: {}", key, data);
                found += 1;
            }
        }

        cursor = next_cursor;
        if cursor == 0 {
            break;
        }
    }

    if found == 0 {
        println!("no pending requests");
    }
    Ok(())
}

async fn handle_list_credential_allows(con: &mut redis::aio::MultiplexedConnection) -> Result<()> {
    let match_pattern = format!("{}:*", polis_common::keys::CREDENTIAL_ALLOW);
    let mut cursor: u64 = 0;
    let mut found = 0u64;

    loop {
        let (next_cursor, batch): (u64, Vec<String>) = redis::cmd("SCAN")
            .arg(cursor)
            .arg("MATCH")
            .arg(&match_pattern)
            .arg("COUNT")
            .arg(100)
            .query_async(con)
            .await
            .context("failed to SCAN credential allow keys")?;

        for key in &batch {
            if let Some((pattern, host, fingerprint)) =
                polis_common::parse_credential_allow_key(key)
            {
                println!("{pattern}\t{host}\t{fingerprint}");
                found += 1;
            }
        }

        cursor = next_cursor;
        if cursor == 0 {
            break;
        }
    }

    if found == 0 {
        println!("no credential allow rules");
    }
    Ok(())
}

async fn handle_delete_credential_allow(
    con: &mut redis::aio::MultiplexedConnection,
    pattern: &str,
    host: &str,
    fingerprint: &str,
) -> Result<()> {
    let key = polis_common::credential_allow_key(pattern, host, fingerprint)
        .map_err(|message| anyhow::anyhow!(message))?;
    let deleted: i64 = con
        .del(&key)
        .await
        .context("failed to DEL credential allow rule")?;
    if deleted == 0 {
        bail!("no credential allow rule found for {} on {}", pattern, host);
    }
    let normalized_host =
        polis_common::normalize_approval_host(host).map_err(|message| anyhow::anyhow!(message))?;
    println!(
        "deleted credential allow: {} {} {}",
        pattern, normalized_host, fingerprint
    );
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut cli = Cli::parse();

    // Load Valkey password from environment variable only (CWE-214).
    // The password MUST NOT be accepted as a CLI argument.
    cli.valkey_pass =
        std::env::var("polis_VALKEY_PASS").context("polis_VALKEY_PASS env var is required")?;

    // Build the Valkey connection URL with ACL credentials.
    // Uses rediss:// (TLS) per requirement 5.6.
    let conn_url = build_connection_url(&cli.valkey_url, &cli.valkey_user, &cli.valkey_pass)?;

    // Load TLS certificates for mTLS authentication with Valkey.
    let tls_certs = load_tls_certificates(&cli.tls_ca, &cli.tls_cert, &cli.tls_key)?;

    let client = redis::Client::build_with_tls(conn_url.as_str(), tls_certs)
        .context("failed to create Valkey client with mTLS")?;

    let mut con = client
        .get_multiplexed_async_connection()
        .await
        .context("failed to connect to Valkey")?;

    match cli.command {
        Commands::Approve { ref request_id } => handle_approve(&mut con, request_id).await,
        Commands::AllowCredential { ref request_id } => {
            handle_allow_credential(&mut con, request_id).await
        }
        Commands::BypassDomain { ref request_id } => {
            handle_bypass_domain(&mut con, request_id).await
        }
        Commands::Deny { ref request_id } => handle_deny(&mut con, request_id).await,
        Commands::ListPending => handle_list_pending(&mut con).await,
        Commands::ListCredentialAllows => handle_list_credential_allows(&mut con).await,
        Commands::DeleteCredentialAllow {
            ref pattern,
            ref host,
            ref fingerprint,
        } => handle_delete_credential_allow(&mut con, pattern, host, fingerprint).await,
        Commands::SetSecurityLevel { ref level } => {
            let _level = parse_security_level(level)?;
            let level_str = level.to_lowercase();
            let _: () = con
                .set(polis_common::keys::SECURITY_LEVEL, &level_str)
                .await
                .context("failed to SET security level")?;
            println!("security level set to {}", level_str);
            Ok(())
        }
        Commands::AutoApprove {
            ref pattern,
            ref action,
        } => {
            let _action = parse_auto_approve_action(action)?;
            let action_str = action.to_lowercase();
            let key = polis_common::auto_approve_key(pattern);
            let _: () = con
                .set(&key, &action_str)
                .await
                .context("failed to SET auto-approve rule")?;
            println!("auto-approve rule set: {} → {}", pattern, action_str);
            Ok(())
        }
    }
}

/// Load TLS certificates for mTLS authentication with Valkey.
///
/// Reads the CA certificate, client certificate, and client private key from
/// PEM files and returns a [`redis::TlsCertificates`] suitable for
/// [`redis::Client::build_with_tls`].
fn load_tls_certificates(
    ca_path: &str,
    cert_path: &str,
    key_path: &str,
) -> Result<redis::TlsCertificates> {
    let root_cert =
        std::fs::read(ca_path).with_context(|| format!("failed to read CA cert: {ca_path}"))?;
    let client_cert = std::fs::read(cert_path)
        .with_context(|| format!("failed to read client cert: {cert_path}"))?;
    let client_key = std::fs::read(key_path)
        .with_context(|| format!("failed to read client key: {key_path}"))?;

    Ok(redis::TlsCertificates {
        client_tls: Some(redis::ClientTlsConfig {
            client_cert,
            client_key,
        }),
        root_cert: Some(root_cert),
    })
}

/// Build a Valkey connection URL with ACL credentials embedded.
///
/// Transforms `rediss://host:port` into `rediss://user:pass@host:port`.
/// Ensures the URL uses the `rediss://` scheme for TLS (requirement 5.6).
fn build_connection_url(base_url: &str, user: &str, pass: &str) -> Result<String> {
    if !base_url.starts_with("rediss://") {
        bail!("Valkey URL must use rediss:// for TLS, got: {}", base_url);
    }
    // Strip the scheme, insert credentials, reassemble.
    let host_part = &base_url["rediss://".len()..];
    Ok(format!("rediss://{}:{}@{}", user, pass, host_part))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- build_connection_url ---

    #[test]
    fn build_url_inserts_credentials() {
        let url = build_connection_url("rediss://valkey:6379", "mcp-admin", "s3cret").unwrap();
        assert_eq!(url, "rediss://mcp-admin:s3cret@valkey:6379");
    }

    #[test]
    fn build_url_rejects_non_tls() {
        let err = build_connection_url("redis://valkey:6379", "mcp-admin", "s3cret");
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("rediss://"));
    }

    // --- parse_security_level ---

    #[test]
    fn parse_security_level_valid_variants() {
        assert_eq!(
            parse_security_level("relaxed").unwrap(),
            SecurityLevel::Relaxed
        );
        assert_eq!(
            parse_security_level("balanced").unwrap(),
            SecurityLevel::Balanced
        );
        assert_eq!(
            parse_security_level("strict").unwrap(),
            SecurityLevel::Strict
        );
    }

    #[test]
    fn parse_security_level_case_insensitive() {
        assert_eq!(
            parse_security_level("STRICT").unwrap(),
            SecurityLevel::Strict
        );
        assert_eq!(
            parse_security_level("Balanced").unwrap(),
            SecurityLevel::Balanced
        );
        assert_eq!(
            parse_security_level("RELAXED").unwrap(),
            SecurityLevel::Relaxed
        );
    }

    #[test]
    fn parse_security_level_rejects_invalid() {
        assert!(parse_security_level("unknown").is_err());
        assert!(parse_security_level("").is_err());
    }

    // --- parse_auto_approve_action ---

    #[test]
    fn parse_auto_approve_action_valid_variants() {
        assert_eq!(
            parse_auto_approve_action("allow").unwrap(),
            AutoApproveAction::Allow
        );
        assert_eq!(
            parse_auto_approve_action("prompt").unwrap(),
            AutoApproveAction::Prompt
        );
        assert_eq!(
            parse_auto_approve_action("block").unwrap(),
            AutoApproveAction::Block
        );
    }

    #[test]
    fn parse_auto_approve_action_case_insensitive() {
        assert_eq!(
            parse_auto_approve_action("ALLOW").unwrap(),
            AutoApproveAction::Allow
        );
        assert_eq!(
            parse_auto_approve_action("Block").unwrap(),
            AutoApproveAction::Block
        );
    }

    #[test]
    fn parse_auto_approve_action_rejects_invalid() {
        assert!(parse_auto_approve_action("deny").is_err());
        assert!(parse_auto_approve_action("").is_err());
    }
}
