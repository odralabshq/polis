use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Reason why a request was blocked by the security layer
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BlockReason {
    CredentialDetected,
    MalwareDomain,
    UrlBlocked,
    FileInfected,
}

/// Security level configuration
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum SecurityLevel {
    Relaxed,
    #[default]
    Balanced,
    Strict,
}

/// Status of a security request
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RequestStatus {
    Pending,
    Approved,
    Denied,
}

/// A blocked request awaiting approval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockedRequest {
    pub request_id: String,
    pub reason: BlockReason,
    pub destination: String,
    pub pattern: Option<String>,
    pub blocked_at: DateTime<Utc>,
    pub status: RequestStatus,
}

/// User confirmation for approval requests
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UserConfirmation {
    Yes,
    No,
    Approve,
    Allow,
    Deny,
}

/// Auto-approve rule action
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AutoApproveAction {
    Allow,
    Prompt,
    Block,
}

/// Source channel for an approval action (for audit logging)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalSource {
    /// Approved via proxy RESPMOD interception (user typed command in chat)
    ProxyInterception,
    /// Approved via CLI tool on host/gateway
    Cli,
    /// Approved via MCP-Admin server (IDE integration, post-MVP)
    McpAdmin,
}

/// Auto-approve configuration rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoApproveRule {
    pub pattern: String,
    pub action: AutoApproveAction,
}

/// Single log entry for security events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityLogEntry {
    pub timestamp: DateTime<Utc>,
    pub event_type: String,
    pub request_id: Option<String>,
    pub details: String,
}

/// One-Time Token mapping created by REQMOD code rewriting.
/// Maps the OTT code (visible to user) back to the original request_id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OttMapping {
    /// The OTT code that replaced the request_id in the outbound message
    pub ott_code: String,
    /// The original request_id this OTT maps to
    pub request_id: String,
    /// Timestamp after which this OTT becomes valid (time-gate)
    pub armed_after: DateTime<Utc>,
    /// The destination host that triggered OTT generation (context binding)
    pub origin_host: String,
    /// When this mapping was created
    pub created_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    // --- BlockReason serde round-trip ---
    #[test]
    fn block_reason_serde_round_trip() {
        let variants = [
            (BlockReason::CredentialDetected, "\"credential_detected\""),
            (BlockReason::MalwareDomain, "\"malware_domain\""),
            (BlockReason::UrlBlocked, "\"url_blocked\""),
            (BlockReason::FileInfected, "\"file_infected\""),
        ];
        for (variant, expected_json) in &variants {
            let json = serde_json::to_string(variant).unwrap();
            assert_eq!(&json, expected_json);
            let deserialized: BlockReason = serde_json::from_str(&json).unwrap();
            assert_eq!(&deserialized, variant);
        }
    }

    // --- SecurityLevel serde round-trip ---
    #[test]
    fn security_level_serde_round_trip() {
        let variants = [
            (SecurityLevel::Relaxed, "\"relaxed\""),
            (SecurityLevel::Balanced, "\"balanced\""),
            (SecurityLevel::Strict, "\"strict\""),
        ];
        for (variant, expected_json) in &variants {
            let json = serde_json::to_string(variant).unwrap();
            assert_eq!(&json, expected_json);
            let deserialized: SecurityLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(&deserialized, variant);
        }
    }

    // --- SecurityLevel default ---
    #[test]
    fn security_level_default_is_balanced() {
        assert_eq!(SecurityLevel::default(), SecurityLevel::Balanced);
    }

    // --- RequestStatus serde round-trip ---
    #[test]
    fn request_status_serde_round_trip() {
        let variants = [
            (RequestStatus::Pending, "\"pending\""),
            (RequestStatus::Approved, "\"approved\""),
            (RequestStatus::Denied, "\"denied\""),
        ];
        for (variant, expected_json) in &variants {
            let json = serde_json::to_string(variant).unwrap();
            assert_eq!(&json, expected_json);
            let deserialized: RequestStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(&deserialized, variant);
        }
    }

    // --- ApprovalSource serializes to snake_case ---
    #[test]
    fn approval_source_serde_snake_case() {
        let cases = [
            (ApprovalSource::ProxyInterception, "\"proxy_interception\""),
            (ApprovalSource::Cli, "\"cli\""),
            (ApprovalSource::McpAdmin, "\"mcp_admin\""),
        ];
        for (variant, expected_json) in &cases {
            let json = serde_json::to_string(variant).unwrap();
            assert_eq!(&json, expected_json);
            let deserialized: ApprovalSource = serde_json::from_str(&json).unwrap();
            assert_eq!(&deserialized, variant);
        }
    }

    // --- BlockedRequest serde round-trip ---
    #[test]
    fn blocked_request_serde_round_trip() {
        let req = BlockedRequest {
            request_id: "req-abc12345".to_string(),
            reason: BlockReason::CredentialDetected,
            destination: "https://example.com".to_string(),
            pattern: Some("password=.*".to_string()),
            blocked_at: Utc::now(),
            status: RequestStatus::Pending,
        };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: BlockedRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.request_id, req.request_id);
        assert_eq!(deserialized.reason, req.reason);
        assert_eq!(deserialized.destination, req.destination);
        assert_eq!(deserialized.pattern, req.pattern);
        assert_eq!(deserialized.blocked_at, req.blocked_at);
        assert_eq!(deserialized.status, req.status);
    }

    // --- OttMapping serde round-trip ---
    #[test]
    fn ott_mapping_serde_round_trip() {
        let now = Utc::now();
        let mapping = OttMapping {
            ott_code: "ott-x7k9m2p4".to_string(),
            request_id: "req-abc12345".to_string(),
            armed_after: now,
            origin_host: "api.telegram.org".to_string(),
            created_at: now,
        };
        let json = serde_json::to_string(&mapping).unwrap();
        let deserialized: OttMapping = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.ott_code, mapping.ott_code);
        assert_eq!(deserialized.request_id, mapping.request_id);
        assert_eq!(deserialized.armed_after, mapping.armed_after);
        assert_eq!(deserialized.origin_host, mapping.origin_host);
        assert_eq!(deserialized.created_at, mapping.created_at);
    }

    // --- AutoApproveRule serde round-trip ---
    #[test]
    fn auto_approve_rule_serde_round_trip() {
        let rule = AutoApproveRule {
            pattern: "*.example.com".to_string(),
            action: AutoApproveAction::Allow,
        };
        let json = serde_json::to_string(&rule).unwrap();
        let deserialized: AutoApproveRule = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.pattern, rule.pattern);
        assert_eq!(deserialized.action, rule.action);
    }

    // --- SecurityLogEntry serde round-trip ---
    #[test]
    fn security_log_entry_serde_round_trip() {
        let entry = SecurityLogEntry {
            timestamp: Utc::now(),
            event_type: "block".to_string(),
            request_id: Some("req-abc12345".to_string()),
            details: "Credential detected in request body".to_string(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: SecurityLogEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.timestamp, entry.timestamp);
        assert_eq!(deserialized.event_type, entry.event_type);
        assert_eq!(deserialized.request_id, entry.request_id);
        assert_eq!(deserialized.details, entry.details);
    }

    #[test]
    fn security_log_entry_serde_none_request_id() {
        let entry = SecurityLogEntry {
            timestamp: Utc::now(),
            event_type: "system_startup".to_string(),
            request_id: None,
            details: "Server started".to_string(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: SecurityLogEntry = serde_json::from_str(&json).unwrap();
        assert!(deserialized.request_id.is_none());
        assert_eq!(deserialized.event_type, "system_startup");
    }
}
