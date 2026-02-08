use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use molis_mcp_common::{AutoApproveAction, SecurityLevel};
use redis::AsyncCommands;
use std::time::{SystemTime, UNIX_EPOCH};

/// Molis HITL approval CLI tool.
///
/// Manages blocked-request approvals, security levels, and auto-approve
/// rules via a TLS-secured Valkey connection authenticated as `mcp-admin`.
#[derive(Parser, Debug)]
#[command(name = "molis-approve", version, about)]
struct Cli {
    /// Valkey URL (must use rediss:// for TLS)
    #[arg(long, default_value = "rediss://valkey:6379")]
    valkey_url: String,

    /// Valkey ACL username
    #[arg(long, default_value = "mcp-admin")]
    valkey_user: String,

    /// Valkey ACL password — loaded from MOLIS_VALKEY_PASS env var (CWE-214).
    /// Never passed as a CLI argument.
    #[arg(skip)]
    valkey_pass: String,

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

#[tokio::main]
async fn main() -> Result<()> {
    let mut cli = Cli::parse();

    // Load Valkey password from environment variable only (CWE-214).
    // The password MUST NOT be accepted as a CLI argument.
    cli.valkey_pass = std::env::var("MOLIS_VALKEY_PASS")
        .context("MOLIS_VALKEY_PASS env var is required")?;

    // Build the Valkey connection URL with ACL credentials.
    // Uses rediss:// (TLS) per requirement 5.6.
    let conn_url = build_connection_url(
        &cli.valkey_url,
        &cli.valkey_user,
        &cli.valkey_pass,
    )?;

    let client = redis::Client::open(conn_url.as_str())
        .context("failed to create Valkey client")?;

    let mut con = client
        .get_multiplexed_async_connection()
        .await
        .context("failed to connect to Valkey")?;

    match cli.command {
        Commands::Approve { ref request_id } => {
            molis_mcp_common::validate_request_id(request_id)
                .map_err(|e| anyhow::anyhow!(e))?;

            let blocked_key = molis_mcp_common::blocked_key(request_id);
            let approved_key = molis_mcp_common::approved_key(request_id);

            // Check blocked request exists and GET data for audit preservation (Req 5.4)
            let blocked_data: Option<String> = con
                .get(&blocked_key)
                .await
                .context("failed to GET blocked request")?;

            let blocked_data = match blocked_data {
                Some(data) => data,
                None => bail!(
                    "no blocked request found for {}",
                    request_id
                ),
            };

            // ZADD audit log FIRST — before destructive operations.
            // This ensures audit data is persisted even if the process
            // crashes between ZADD and DEL (Finding 1 fix).
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("system clock error")?
                .as_secs();

            let audit_entry = serde_json::json!({
                "event_type": "approved_via_cli",
                "request_id": request_id,
                "timestamp": now,
                "blocked_request": blocked_data,
            });

            let _: () = con
                .zadd(
                    molis_mcp_common::keys::EVENT_LOG,
                    audit_entry.to_string(),
                    now as f64,
                )
                .await
                .context("failed to ZADD audit log entry")?;

            // Use pipeline (MULTI/EXEC) for atomic DEL + SETEX (Finding 4 fix).
            // Prevents partial state where blocked is deleted but approved is not set.
            redis::pipe()
                .atomic()
                .del(&blocked_key)
                .set_ex(
                    &approved_key,
                    "approved",
                    molis_mcp_common::ttl::APPROVED_REQUEST_SECS,
                )
                .query_async::<Vec<redis::Value>>(&mut con)
                .await
                .context("failed to atomically DEL blocked + SETEX approved")?;

            println!("approved {}", request_id);
            Ok(())
        }
        Commands::Deny { ref request_id } => {
            molis_mcp_common::validate_request_id(request_id)
                .map_err(|e| anyhow::anyhow!(e))?;

            let blocked_key = molis_mcp_common::blocked_key(request_id);

            // Check blocked request exists and GET data for audit preservation (Req 5.4)
            let blocked_data: Option<String> = con
                .get(&blocked_key)
                .await
                .context("failed to GET blocked request")?;

            let blocked_data = match blocked_data {
                Some(data) => data,
                None => bail!(
                    "no blocked request found for {}",
                    request_id
                ),
            };

            // ZADD audit log FIRST — before destructive DEL (Finding 1 fix).
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("system clock error")?
                .as_secs();

            let audit_entry = serde_json::json!({
                "event_type": "denied_via_cli",
                "request_id": request_id,
                "timestamp": now,
                "blocked_request": blocked_data,
            });

            let _: () = con
                .zadd(
                    molis_mcp_common::keys::EVENT_LOG,
                    audit_entry.to_string(),
                    now as f64,
                )
                .await
                .context("failed to ZADD audit log entry")?;

            // DEL blocked key (deny does not create an approved key)
            let _: () = con
                .del(&blocked_key)
                .await
                .context("failed to DEL blocked key")?;

            println!("denied {}", request_id);
            Ok(())
        }
        Commands::ListPending => {
            // SCAN for molis:blocked:* keys using cursor-based iteration.
            let match_pattern = format!("{}:*", molis_mcp_common::keys::BLOCKED);
            let mut cursor: u64 = 0;
            let mut found = 0u64;

            loop {
                // SCAN cursor MATCH pattern COUNT 100
                let (next_cursor, batch): (u64, Vec<String>) =
                    redis::cmd("SCAN")
                        .arg(cursor)
                        .arg("MATCH")
                        .arg(&match_pattern)
                        .arg("COUNT")
                        .arg(100)
                        .query_async(&mut con)
                        .await
                        .context("failed to SCAN blocked keys")?;

                for key in &batch {
                    let value: Option<String> = con
                        .get(key)
                        .await
                        .context("failed to GET blocked request")?;

                    if let Some(data) = value {
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
        Commands::SetSecurityLevel { ref level } => {
            let _level = parse_security_level(level)?;

            // Store the validated, lowercase level string in Valkey.
            let level_str = level.to_lowercase();
            let _: () = con
                .set(
                    molis_mcp_common::keys::SECURITY_LEVEL,
                    &level_str,
                )
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

            // Store the validated, lowercase action string in Valkey
            // at the key molis:config:auto_approve:{pattern}.
            let action_str = action.to_lowercase();
            let key = molis_mcp_common::auto_approve_key(pattern);
            let _: () = con
                .set(&key, &action_str)
                .await
                .context("failed to SET auto-approve rule")?;

            println!(
                "auto-approve rule set: {} → {}",
                pattern, action_str
            );
            Ok(())
        }
    }
}

/// Build a Valkey connection URL with ACL credentials embedded.
///
/// Transforms `rediss://host:port` into `rediss://user:pass@host:port`.
/// Ensures the URL uses the `rediss://` scheme for TLS (requirement 5.6).
fn build_connection_url(
    base_url: &str,
    user: &str,
    pass: &str,
) -> Result<String> {
    if !base_url.starts_with("rediss://") {
        bail!(
            "Valkey URL must use rediss:// for TLS, got: {}",
            base_url
        );
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
        let url = build_connection_url(
            "rediss://valkey:6379",
            "mcp-admin",
            "s3cret",
        )
        .unwrap();
        assert_eq!(url, "rediss://mcp-admin:s3cret@valkey:6379");
    }

    #[test]
    fn build_url_rejects_non_tls() {
        let err = build_connection_url(
            "redis://valkey:6379",
            "mcp-admin",
            "s3cret",
        );
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
