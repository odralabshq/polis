//! MCP tool implementations for the polis agent server.
//!
//! Exposes exactly 5 read-only tools via the `rmcp` `#[tool]` macro:
//!   - `report_block`
//!   - `get_security_status`
//!   - `list_pending_approvals`
//!   - `get_security_log`
//!   - `check_request_status`
//!
//! **Security constraint**: No `approve_request`, `deny_request`,
//! `configure_auto_approve`, or `set_security_level` tools are exposed.
//! These operations are reserved for the CLI / MCP-Admin (spec 10).

use std::sync::Arc;

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::ServerInfo,
    tool, tool_handler, tool_router, ServerHandler,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use polis_common::{
    redis_keys::approval::approval_command, validate_request_id, BlockedRequest, RequestStatus,
    SecurityLogEntry,
};

use crate::state::AppState;

// ===================================================================
// Input structs
// ===================================================================

/// Input parameters for the `report_block` tool.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ReportBlockInput {
    /// The request ID (format: `req-[a-f0-9]{8}`, exactly 12 chars).
    pub request_id: String,
    /// Human-readable reason the request was blocked.
    pub reason: String,
    /// The destination URL or host that was blocked.
    pub destination: String,
    /// Optional DLP pattern that triggered the block.
    /// Stored in Valkey for admin use but **never** returned to the agent.
    pub pattern: Option<String>,
}

/// Input parameters for the `check_request_status` tool.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CheckRequestStatusInput {
    /// The request ID to check (format: `req-[a-f0-9]{8}`).
    pub request_id: String,
}

// ===================================================================
// Output structs
// ===================================================================

/// Output returned by the `report_block` tool.
#[derive(Debug, Clone, Serialize)]
pub struct ReportBlockOutput {
    /// Human-readable message (pattern is **redacted** — CWE-200).
    pub message: String,
    /// The request ID that was stored.
    pub request_id: String,
    /// Whether the request requires human approval.
    pub requires_approval: bool,
    /// CLI command the user can run to approve the request.
    pub approval_command: String,
}

/// Output returned by the `get_security_status` tool.
#[derive(Debug, Clone, Serialize)]
pub struct SecurityStatusOutput {
    /// Overall status label.
    pub status: String,
    /// Number of pending (blocked) requests.
    pub pending_approvals: usize,
    /// Number of recently approved requests.
    pub recent_approvals: usize,
    /// Current security level (relaxed / balanced / strict).
    pub security_level: String,
}

/// Output returned by the `list_pending_approvals` tool.
#[derive(Debug, Clone, Serialize)]
pub struct PendingApprovalsOutput {
    /// Blocked requests awaiting approval.
    /// The `pattern` field is set to `None` on every entry (CWE-200).
    pub pending: Vec<BlockedRequest>,
}

/// Output returned by the `get_security_log` tool.
#[derive(Debug, Clone, Serialize)]
pub struct SecurityLogOutput {
    /// Most recent security events (up to 50).
    pub entries: Vec<SecurityLogEntry>,
    /// Total number of entries returned.
    pub total_count: usize,
}

/// Output returned by the `check_request_status` tool.
#[derive(Debug, Clone, Serialize)]
pub struct CheckRequestStatusOutput {
    /// The request ID that was checked.
    pub request_id: String,
    /// Status string: `"approved"`, `"pending"`, or `"not_found"`.
    pub status: String,
    /// Human-readable explanation.
    pub message: String,
}

// ===================================================================
// PolisAgentTools — the MCP server handler
// ===================================================================

/// MCP server handler exposing 5 read-only tools to the workspace agent.
///
/// Holds a shared reference to [`AppState`] for Valkey operations.
#[derive(Clone)]
pub struct PolisAgentTools {
    state: Arc<AppState>,
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for PolisAgentTools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PolisAgentTools")
            .field("state", &"<AppState>")
            .finish()
    }
}

impl PolisAgentTools {
    /// Create a new `PolisAgentTools` with the given application state.
    pub fn new(state: Arc<AppState>) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }
}

// -------------------------------------------------------------------
// Tool implementations
// -------------------------------------------------------------------

#[tool_router]
impl PolisAgentTools {
    /// Report a blocked request to the security system.
    ///
    /// Validates the request_id format, stores the blocked request in
    /// Valkey with a 1-hour TTL, logs a security event, and returns
    /// an approval command. The DLP pattern is **never** included in
    /// the agent-facing response (CWE-200).
    #[tool(description = "Report a blocked outbound request. \
        Returns an approval command the user can run.")]
    async fn report_block(&self, params: Parameters<ReportBlockInput>) -> Result<String, String> {
        let input = params.0;

        // Validate request_id format (CWE-20).
        validate_request_id(&input.request_id).map_err(|e| format!("Invalid request_id: {e}"))?;

        // Parse the reason string into a BlockReason enum.
        let reason = parse_block_reason(&input.reason)?;

        let blocked = BlockedRequest {
            request_id: input.request_id.clone(),
            reason,
            destination: input.destination.clone(),
            pattern: input.pattern.clone(),
            blocked_at: chrono::Utc::now(),
            status: RequestStatus::Pending,
        };

        // Store in Valkey (SETEX with 1h TTL).
        self.state
            .store_blocked_request(&blocked)
            .await
            .map_err(|e| format!("Failed to store blocked request: {e}"))?;

        // Log security event
        let log_entry = polis_common::types::SecurityLogEntry {
            timestamp: chrono::Utc::now(),
            event_type: "block_reported".to_string(),
            request_id: Some(input.request_id.clone()),
            details: format!("Blocked request to {}", input.destination),
        };
        self.state
            .log_security_event(&log_entry)
            .await
            .map_err(|e| format!("Failed to log event: {e}"))?;

        // Build agent-facing output — pattern is REDACTED (CWE-200).
        let output = ReportBlockOutput {
            message: format!(
                "Request {} to {} has been blocked (reason: {}). \
                 A human must approve it before it can proceed.",
                input.request_id, input.destination, input.reason,
            ),
            request_id: input.request_id.clone(),
            requires_approval: true,
            approval_command: approval_command(&input.request_id),
        };

        serde_json::to_string(&output).map_err(|e| format!("Serialization error: {e}"))
    }

    /// Query the current security status.
    ///
    /// Returns counts of pending and recently approved requests,
    /// plus the current security level.
    #[tool(description = "Get the current security status including \
        pending approvals, recent approvals, and security level.")]
    async fn get_security_status(&self) -> Result<String, String> {
        let pending = self
            .state
            .count_pending_approvals()
            .await
            .map_err(|e| format!("Failed to count pending: {e}"))?;

        let recent = self
            .state
            .count_recent_approvals()
            .await
            .map_err(|e| format!("Failed to count recent: {e}"))?;

        let level = self
            .state
            .get_security_level()
            .await
            .map_err(|e| format!("Failed to get level: {e}"))?;

        let output = SecurityStatusOutput {
            status: "ok".to_string(),
            pending_approvals: pending,
            recent_approvals: recent,
            security_level: format!("{:?}", level).to_lowercase(),
        };

        serde_json::to_string(&output).map_err(|e| format!("Serialization error: {e}"))
    }

    /// List all pending (blocked) requests awaiting approval.
    ///
    /// The `pattern` field is set to `None` on every returned entry
    /// to prevent DLP ruleset exfiltration (CWE-200).
    #[tool(description = "List all blocked requests that are \
        pending human approval.")]
    async fn list_pending_approvals(&self) -> Result<String, String> {
        let pending = self
            .state
            .get_pending_approvals()
            .await
            .map_err(|e| format!("Failed to list pending: {e}"))?;

        let output = PendingApprovalsOutput { pending };

        serde_json::to_string(&output).map_err(|e| format!("Serialization error: {e}"))
    }

    /// Retrieve the most recent security log events (up to 50).
    #[tool(description = "Get the most recent security log events \
        (up to 50 entries).")]
    async fn get_security_log(&self) -> Result<String, String> {
        let entries = self
            .state
            .get_security_log(50)
            .await
            .map_err(|e| format!("Failed to get log: {e}"))?;

        let total_count = entries.len();
        let output = SecurityLogOutput {
            entries,
            total_count,
        };

        serde_json::to_string(&output).map_err(|e| format!("Serialization error: {e}"))
    }

    /// Check the approval status of a specific request.
    ///
    /// Validates the request_id format before querying Valkey.
    /// Returns `"approved"`, `"pending"`, or `"not_found"`.
    #[tool(description = "Check the approval status of a blocked \
        request by its request_id.")]
    async fn check_request_status(
        &self,
        params: Parameters<CheckRequestStatusInput>,
    ) -> Result<String, String> {
        let input = params.0;

        // Validate request_id format (CWE-20).
        validate_request_id(&input.request_id).map_err(|e| format!("Invalid request_id: {e}"))?;

        let status = self
            .state
            .check_request_status(&input.request_id)
            .await
            .map_err(|e| format!("Failed to check status: {e}"))?;

        let (status_str, message) = match status {
            RequestStatus::Approved => (
                "approved",
                format!("Request {} has been approved.", input.request_id,),
            ),
            RequestStatus::Pending => (
                "pending",
                format!("Request {} is pending approval.", input.request_id,),
            ),
            // Denied is used as "not_found" (see state.rs).
            RequestStatus::Denied => (
                "not_found",
                format!(
                    "Request {} was not found (expired or never stored).",
                    input.request_id,
                ),
            ),
        };

        let output = CheckRequestStatusOutput {
            request_id: input.request_id,
            status: status_str.to_string(),
            message,
        };

        serde_json::to_string(&output).map_err(|e| format!("Serialization error: {e}"))
    }
}

// -------------------------------------------------------------------
// ServerHandler implementation (via tool_handler macro)
// -------------------------------------------------------------------

#[tool_handler]
impl ServerHandler for PolisAgentTools {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "polis Toolbox — read-only security tools. \
                 Use report_block to report blocked requests, \
                 check_request_status to query approval state."
                    .into(),
            ),
            ..Default::default()
        }
    }
}

// -------------------------------------------------------------------
// Helpers
// -------------------------------------------------------------------

/// Parse a reason string into a `BlockReason` enum variant.
///
/// Accepts snake_case strings matching the serde representation.
fn parse_block_reason(reason: &str) -> Result<polis_common::BlockReason, String> {
    // Try serde deserialization from a JSON string value.
    let json_str = format!("\"{}\"", reason);
    serde_json::from_str::<polis_common::BlockReason>(&json_str).map_err(|_| {
        format!(
            "Unknown block reason '{}'. Expected one of: \
                 credential_detected, malware_domain, \
                 url_blocked, file_infected",
            reason,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_block_reason_valid() {
        assert!(parse_block_reason("credential_detected").is_ok());
        assert!(parse_block_reason("malware_domain").is_ok());
        assert!(parse_block_reason("url_blocked").is_ok());
        assert!(parse_block_reason("file_infected").is_ok());
    }

    #[test]
    fn parse_block_reason_invalid() {
        assert!(parse_block_reason("unknown").is_err());
        assert!(parse_block_reason("").is_err());
    }
}
