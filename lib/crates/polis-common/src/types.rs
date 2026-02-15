use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

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
    /// SHA-256 hash of the matched credential value (hex, 64 chars).
    /// None for non-credential blocks (e.g., new_domain_prompt).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_hash: Option<String>,
    /// First 4 chars of the credential for display (e.g., "sk-a").
    /// None for non-credential blocks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_prefix: Option<String>,
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

/// Action type for OTT — distinguishes approve from except
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OttAction {
    Approve,
    Except,
}

impl Default for OttAction {
    fn default() -> Self {
        OttAction::Approve
    }
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
    /// What action this OTT triggers (approve or except).
    /// Defaults to Approve for backward compatibility with pre-upgrade OTTs.
    #[serde(default)]
    pub action: OttAction,
}

/// Source channel for exception creation (for audit logging)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExceptionSource {
    /// Created via proxy RESPMOD interception (user typed /polis-except in chat)
    ProxyInterception,
    /// Created via CLI tool on host/gateway
    Cli,
}

/// A persistent value-based exception allowing a specific credential
/// hash to reach a specific destination without DLP blocking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValueException {
    /// Full SHA-256 hash of the credential (64 lowercase hex chars)
    pub credential_hash: String,
    /// First 4 chars of credential for human-readable display
    pub credential_prefix: String,
    /// Destination host, or "*" for wildcard (CLI only)
    pub destination: String,
    /// DLP pattern name that originally matched (e.g., "anthropic")
    pub pattern_name: String,
    /// When this exception was created
    pub created_at: DateTime<Utc>,
    /// How this exception was created
    pub source: ExceptionSource,
    /// TTL in seconds (None = permanent, CLI only)
    pub ttl_secs: Option<u64>,
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
            credential_hash: Some("a".repeat(64)),
            credential_prefix: Some("sk-a".to_string()),
        };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: BlockedRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.request_id, req.request_id);
        assert_eq!(deserialized.reason, req.reason);
        assert_eq!(deserialized.destination, req.destination);
        assert_eq!(deserialized.pattern, req.pattern);
        assert_eq!(deserialized.blocked_at, req.blocked_at);
        assert_eq!(deserialized.status, req.status);
        assert_eq!(deserialized.credential_hash, req.credential_hash);
        assert_eq!(deserialized.credential_prefix, req.credential_prefix);
    }

    // --- BlockedRequest backward compat (no credential_hash) ---
    #[test]
    fn blocked_request_backward_compat_no_hash() {
        let json = r#"{"request_id":"req-abc12345","reason":"credential_detected","destination":"https://example.com","pattern":null,"blocked_at":"2026-01-01T00:00:00Z","status":"pending"}"#;
        let req: BlockedRequest = serde_json::from_str(json).unwrap();
        assert!(req.credential_hash.is_none());
        assert!(req.credential_prefix.is_none());
    }

    // --- OttAction serde round-trip ---
    #[test]
    fn ott_action_serde_round_trip() {
        let cases = [
            (OttAction::Approve, "\"approve\""),
            (OttAction::Except, "\"except\""),
        ];
        for (variant, expected_json) in &cases {
            let json = serde_json::to_string(variant).unwrap();
            assert_eq!(&json, expected_json);
            let deserialized: OttAction = serde_json::from_str(&json).unwrap();
            assert_eq!(&deserialized, variant);
        }
    }

    // --- OttAction default is Approve ---
    #[test]
    fn ott_action_default_is_approve() {
        assert_eq!(OttAction::default(), OttAction::Approve);
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
            action: OttAction::Except,
        };
        let json = serde_json::to_string(&mapping).unwrap();
        let deserialized: OttMapping = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.ott_code, mapping.ott_code);
        assert_eq!(deserialized.request_id, mapping.request_id);
        assert_eq!(deserialized.armed_after, mapping.armed_after);
        assert_eq!(deserialized.origin_host, mapping.origin_host);
        assert_eq!(deserialized.created_at, mapping.created_at);
        assert_eq!(deserialized.action, OttAction::Except);
    }

    // --- OttMapping backward compat (no action field) ---
    #[test]
    fn ott_mapping_backward_compat_no_action() {
        let json = r#"{"ott_code":"ott-x7k9m2p4","request_id":"req-abc12345","armed_after":"2026-01-01T00:00:00Z","origin_host":"api.telegram.org","created_at":"2026-01-01T00:00:00Z"}"#;
        let mapping: OttMapping = serde_json::from_str(json).unwrap();
        assert_eq!(mapping.action, OttAction::Approve);
    }

    // --- ExceptionSource serde round-trip ---
    #[test]
    fn exception_source_serde_round_trip() {
        let cases = [
            (ExceptionSource::ProxyInterception, "\"proxy_interception\""),
            (ExceptionSource::Cli, "\"cli\""),
        ];
        for (variant, expected_json) in &cases {
            let json = serde_json::to_string(variant).unwrap();
            assert_eq!(&json, expected_json);
            let deserialized: ExceptionSource = serde_json::from_str(&json).unwrap();
            assert_eq!(&deserialized, variant);
        }
    }

    // --- ValueException serde round-trip ---
    #[test]
    fn value_exception_serde_round_trip() {
        let exc = ValueException {
            credential_hash: "a".repeat(64),
            credential_prefix: "sk-a".to_string(),
            destination: "api.openai.com".to_string(),
            pattern_name: "openai".to_string(),
            created_at: Utc::now(),
            source: ExceptionSource::Cli,
            ttl_secs: Some(2592000),
        };
        let json = serde_json::to_string(&exc).unwrap();
        let deserialized: ValueException = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.credential_hash, exc.credential_hash);
        assert_eq!(deserialized.credential_prefix, exc.credential_prefix);
        assert_eq!(deserialized.destination, exc.destination);
        assert_eq!(deserialized.pattern_name, exc.pattern_name);
        assert_eq!(deserialized.source, exc.source);
        assert_eq!(deserialized.ttl_secs, exc.ttl_secs);
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
