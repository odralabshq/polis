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

/// Security level configuration.
/// Controls how the DLP module handles requests to new/unknown domains.
///
/// - `Relaxed`: All domains auto-allowed (no prompts for new domains)
/// - `Balanced` (default): Known domains auto-allowed, new domains prompt user
/// - `Strict`: All domains prompt user
///
/// Credentials are always prompted regardless of level.
/// Malware is always blocked regardless of level.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum SecurityLevel {
    Relaxed,
    #[default]
    Balanced,
    Strict,
}

impl SecurityLevel {
    /// Returns human-readable description for CLI help.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Relaxed => "All domains auto-allowed, no prompts for new domains",
            Self::Balanced => "Known domains auto-allowed, new domains prompt",
            Self::Strict => "All domains prompt for approval",
        }
    }

    /// Returns whether new domains should prompt the user.
    #[must_use]
    pub const fn prompt_new_domains(self) -> bool {
        match self {
            Self::Relaxed => false,
            Self::Balanced | Self::Strict => true,
        }
    }

    /// Returns whether known domains should auto-allow.
    #[must_use]
    pub const fn auto_allow_known(self) -> bool {
        match self {
            Self::Relaxed | Self::Balanced => true,
            Self::Strict => false,
        }
    }
}

/// Migrate legacy security level values.
/// Returns the migrated level and whether migration occurred.
#[must_use]
pub fn migrate_security_level(value: &str) -> (SecurityLevel, bool) {
    match value.to_lowercase().as_str() {
        "relaxed" => (SecurityLevel::Relaxed, false),
        "balanced" => (SecurityLevel::Balanced, false),
        "strict" => (SecurityLevel::Strict, false),
        _ => (SecurityLevel::Balanced, true),
    }
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

// ============================================================================
// Foundation Types (Issue 01)
// ============================================================================

/// Stage in the workspace provisioning pipeline.
/// Stages are ordered — each stage implies all previous stages completed.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum RunStage {
    /// VM/workspace image downloaded and verified
    ImageReady,
    /// Workspace created (VM launched or container started)
    WorkspaceCreated,
    /// TLS certificates and Valkey credentials configured
    CredentialsSet,
    /// Workspace provisioned (CA trust, proxy env, services started)
    Provisioned,
    /// Agent installed and running
    AgentReady,
}

impl RunStage {
    /// Returns the next stage in the pipeline, or None if at final stage.
    #[must_use]
    pub const fn next(self) -> Option<Self> {
        match self {
            Self::ImageReady => Some(Self::WorkspaceCreated),
            Self::WorkspaceCreated => Some(Self::CredentialsSet),
            Self::CredentialsSet => Some(Self::Provisioned),
            Self::Provisioned => Some(Self::AgentReady),
            Self::AgentReady => None,
        }
    }

    /// Returns human-readable description for progress display.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::ImageReady => "Image ready",
            Self::WorkspaceCreated => "Workspace created",
            Self::CredentialsSet => "Credentials configured",
            Self::Provisioned => "Workspace provisioned",
            Self::AgentReady => "Agent ready",
        }
    }
}

/// Type of activity event in the Valkey stream.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ActivityEventType {
    /// HTTP request intercepted by gate
    Request,
    /// HTTP response returned through gate
    Response,
    /// Content scanned by sentinel (clean)
    Scan,
    /// Content blocked by sentinel (credential/malware)
    Block,
    /// Agent lifecycle event
    Agent,
}

/// Status of a scanned/inspected item.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InspectionStatus {
    /// Traffic inspected, no issues
    Inspected,
    /// Content scanned, clean
    Clean,
    /// Content blocked by policy
    Blocked,
}

/// Single activity event from the Valkey stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEvent {
    /// Event timestamp (ISO 8601)
    pub ts: DateTime<Utc>,
    /// Event type
    #[serde(rename = "type")]
    pub event_type: ActivityEventType,
    /// Destination host (e.g., "api.anthropic.com")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dest: Option<String>,
    /// HTTP method (e.g., "POST")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// Request path (e.g., "/v1/messages")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Inspection status
    pub status: InspectionStatus,
    /// Block reason (only if status == Blocked)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<BlockReason>,
    /// Additional detail message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Complete status output for `polis status --json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusOutput {
    pub workspace: WorkspaceStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentStatus>,
    pub security: SecurityStatus,
    pub events: SecurityEvents,
}
/// Workspace state enum.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceState {
    Running,
    Stopped,
    Starting,
    Stopping,
    Error,
}

/// Workspace status for CLI display and JSON output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceStatus {
    /// Current status (running, stopped, etc.)
    pub status: WorkspaceState,
    /// Uptime in seconds (only if running)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime_seconds: Option<u64>,
}

/// Agent health enum.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AgentHealth {
    Healthy,
    Unhealthy,
    Starting,
    Stopped,
}

/// Agent status for CLI display and JSON output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatus {
    /// Agent name (e.g., "claude-dev")
    pub name: String,
    /// Agent health status
    pub status: AgentHealth,
}

/// Security status for CLI display and JSON output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityStatus {
    /// Traffic inspection active
    pub traffic_inspection: bool,
    /// Credential protection enabled
    pub credential_protection: bool,
    /// Malware scanning enabled
    pub malware_scanning: bool,
}

/// Event severity level.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum EventSeverity {
    None,
    Info,
    Warning,
    Error,
}

/// Security events summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityEvents {
    /// Number of security events in window
    pub count: u32,
    /// Highest severity level
    pub severity: EventSeverity,
}

/// Persisted run state for checkpoint/resume.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunState {
    /// Current stage in the pipeline
    pub stage: RunStage,
    /// Agent being run
    pub agent: String,
    /// Workspace identifier
    pub workspace_id: String,
    /// When the run started (ISO 8601)
    pub started_at: DateTime<Utc>,
    /// SHA-256 hash of the workspace image
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_sha256: Option<String>,
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
    fn test_security_level_serde_round_trip() {
        let variants = [
            (SecurityLevel::Relaxed, "\"relaxed\""),
            (SecurityLevel::Balanced, "\"balanced\""),
            (SecurityLevel::Strict, "\"strict\""),
        ];
        for (variant, expected_json) in &variants {
            let json = serde_json::to_string(variant).expect("serialize SecurityLevel");
            assert_eq!(&json, expected_json);
            let deserialized: SecurityLevel = serde_json::from_str(&json).expect("deserialize SecurityLevel");
            assert_eq!(&deserialized, variant);
        }
    }

    #[test]
    fn test_security_level_default_is_balanced() {
        assert_eq!(SecurityLevel::default(), SecurityLevel::Balanced);
    }

    #[test]
    fn test_security_level_description_balanced() {
        assert_eq!(
            SecurityLevel::Balanced.description(),
            "Known domains auto-allowed, new domains prompt"
        );
    }

    #[test]
    fn test_security_level_description_strict() {
        assert_eq!(
            SecurityLevel::Strict.description(),
            "All domains prompt for approval"
        );
    }

    #[test]
    fn test_security_level_auto_allow_known_balanced_true() {
        assert!(SecurityLevel::Balanced.auto_allow_known());
    }

    #[test]
    fn test_security_level_auto_allow_known_strict_false() {
        assert!(!SecurityLevel::Strict.auto_allow_known());
    }

    #[test]
    fn test_security_level_prompt_new_domains_both_true() {
        assert!(SecurityLevel::Balanced.prompt_new_domains());
        assert!(SecurityLevel::Strict.prompt_new_domains());
        assert!(!SecurityLevel::Relaxed.prompt_new_domains());
    }

    #[test]
    fn test_migrate_security_level_relaxed_returns_relaxed_no_migration() {
        let (level, migrated) = migrate_security_level("relaxed");
        assert_eq!(level, SecurityLevel::Relaxed);
        assert!(!migrated, "relaxed is a valid level, no migration needed");
    }

    #[test]
    fn test_migrate_security_level_balanced_returns_balanced_no_migration() {
        let (level, migrated) = migrate_security_level("balanced");
        assert_eq!(level, SecurityLevel::Balanced);
        assert!(!migrated, "balanced should not trigger migration");
    }

    #[test]
    fn test_migrate_security_level_strict_returns_strict_no_migration() {
        let (level, migrated) = migrate_security_level("strict");
        assert_eq!(level, SecurityLevel::Strict);
        assert!(!migrated, "strict should not trigger migration");
    }

    #[test]
    fn test_migrate_security_level_unknown_returns_balanced_with_migration() {
        let (level, migrated) = migrate_security_level("invalid_value");
        assert_eq!(level, SecurityLevel::Balanced);
        assert!(migrated, "unknown value should trigger migration");
    }

    #[test]
    fn test_migrate_security_level_case_insensitive() {
        let (level1, _) = migrate_security_level("BALANCED");
        let (level2, _) = migrate_security_level("Strict");
        let (level3, _) = migrate_security_level("RELAXED");
        assert_eq!(level1, SecurityLevel::Balanced);
        assert_eq!(level2, SecurityLevel::Strict);
        assert_eq!(level3, SecurityLevel::Relaxed);
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

    // --- RunStage ordering tests ---
    #[test]
    fn test_run_stage_ordering() {
        assert!(RunStage::ImageReady < RunStage::WorkspaceCreated);
        assert!(RunStage::WorkspaceCreated < RunStage::CredentialsSet);
        assert!(RunStage::CredentialsSet < RunStage::Provisioned);
        assert!(RunStage::Provisioned < RunStage::AgentReady);

        // Verify full ordering chain
        let stages = [
            RunStage::ImageReady,
            RunStage::WorkspaceCreated,
            RunStage::CredentialsSet,
            RunStage::Provisioned,
            RunStage::AgentReady,
        ];
        for i in 0..stages.len() - 1 {
            assert!(stages[i] < stages[i + 1]);
        }
    }

    #[test]
    fn test_run_stage_next() {
        assert_eq!(RunStage::ImageReady.next(), Some(RunStage::WorkspaceCreated));
        assert_eq!(RunStage::WorkspaceCreated.next(), Some(RunStage::CredentialsSet));
        assert_eq!(RunStage::CredentialsSet.next(), Some(RunStage::Provisioned));
        assert_eq!(RunStage::Provisioned.next(), Some(RunStage::AgentReady));
        assert_eq!(RunStage::AgentReady.next(), None);
    }

    // --- ActivityEvent serde round-trip ---
    #[test]
    fn test_activity_event_serde_round_trip() {
        let event = ActivityEvent {
            ts: Utc::now(),
            event_type: ActivityEventType::Request,
            dest: Some("api.anthropic.com".to_string()),
            method: Some("POST".to_string()),
            path: Some("/v1/messages".to_string()),
            status: InspectionStatus::Inspected,
            reason: None,
            detail: None,
        };
        let json = serde_json::to_string(&event).expect("serialize ActivityEvent");
        let deserialized: ActivityEvent = serde_json::from_str(&json).expect("deserialize ActivityEvent");
        assert_eq!(deserialized.ts, event.ts);
        assert_eq!(deserialized.event_type, event.event_type);
        assert_eq!(deserialized.dest, event.dest);
        assert_eq!(deserialized.status, event.status);

        // Verify None fields are omitted
        assert!(!json.contains("\"reason\""));
        assert!(!json.contains("\"detail\""));
    }

    // --- StatusOutput serde round-trip ---
    #[test]
    fn test_status_output_serde_round_trip() {
        let status = StatusOutput {
            workspace: WorkspaceStatus {
                status: WorkspaceState::Running,
                uptime_seconds: Some(3600),
            },
            agent: Some(AgentStatus {
                name: "claude-dev".to_string(),
                status: AgentHealth::Healthy,
            }),
            security: SecurityStatus {
                traffic_inspection: true,
                credential_protection: true,
                malware_scanning: true,
            },
            events: SecurityEvents {
                count: 0,
                severity: EventSeverity::None,
            },
        };
        let json = serde_json::to_string(&status).expect("serialize StatusOutput");
        let deserialized: StatusOutput = serde_json::from_str(&json).expect("deserialize StatusOutput");
        assert_eq!(deserialized.workspace.status, WorkspaceState::Running);
        assert_eq!(deserialized.workspace.uptime_seconds, Some(3600));
        assert_eq!(deserialized.agent.as_ref().map(|a| a.name.as_str()), Some("claude-dev"));
        assert!(deserialized.security.traffic_inspection);
    }

    // --- RunState serde round-trip ---
    #[test]
    fn test_run_state_serde_round_trip() {
        let state = RunState {
            stage: RunStage::Provisioned,
            agent: "claude-dev".to_string(),
            workspace_id: "ws-abc123".to_string(),
            started_at: Utc::now(),
            image_sha256: Some("sha256:abc123def456".to_string()),
        };
        let json = serde_json::to_string(&state).expect("serialize RunState");
        let deserialized: RunState = serde_json::from_str(&json).expect("deserialize RunState");
        assert_eq!(deserialized.stage, RunStage::Provisioned);
        assert_eq!(deserialized.agent, "claude-dev");
        assert_eq!(deserialized.workspace_id, "ws-abc123");
        assert_eq!(deserialized.image_sha256, state.image_sha256);

        // Test with None image_sha256
        let state_no_hash = RunState {
            stage: RunStage::ImageReady,
            agent: "test-agent".to_string(),
            workspace_id: "ws-xyz".to_string(),
            started_at: Utc::now(),
            image_sha256: None,
        };
        let json2 = serde_json::to_string(&state_no_hash).expect("serialize RunState without hash");
        assert!(!json2.contains("\"image_sha256\""));
    }
}

// ============================================================================
// Property-Based Tests (Foundation Types)
// ============================================================================

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn arb_run_stage() -> impl Strategy<Value = RunStage> {
        prop_oneof![
            Just(RunStage::ImageReady),
            Just(RunStage::WorkspaceCreated),
            Just(RunStage::CredentialsSet),
            Just(RunStage::Provisioned),
            Just(RunStage::AgentReady),
        ]
    }

    fn arb_event_severity() -> impl Strategy<Value = EventSeverity> {
        prop_oneof![
            Just(EventSeverity::None),
            Just(EventSeverity::Info),
            Just(EventSeverity::Warning),
            Just(EventSeverity::Error),
        ]
    }

    fn arb_workspace_state() -> impl Strategy<Value = WorkspaceState> {
        prop_oneof![
            Just(WorkspaceState::Running),
            Just(WorkspaceState::Stopped),
            Just(WorkspaceState::Starting),
            Just(WorkspaceState::Stopping),
            Just(WorkspaceState::Error),
        ]
    }

    fn arb_security_level() -> impl Strategy<Value = SecurityLevel> {
        prop_oneof![
            Just(SecurityLevel::Relaxed),
            Just(SecurityLevel::Balanced),
            Just(SecurityLevel::Strict),
        ]
    }

    proptest! {
        /// RunStage::next() eventually terminates at None
        #[test]
        fn prop_run_stage_next_terminates(stage in arb_run_stage()) {
            let mut current = Some(stage);
            let mut steps = 0;
            while let Some(s) = current {
                current = s.next();
                steps += 1;
                prop_assert!(steps <= 5, "next() should terminate within 5 steps");
            }
        }

        /// RunStage ordering is consistent with next()
        #[test]
        fn prop_run_stage_next_increases_order(stage in arb_run_stage()) {
            if let Some(next) = stage.next() {
                prop_assert!(stage < next, "next stage should be greater");
            }
        }

        /// RunStage serde round-trip is identity
        #[test]
        fn prop_run_stage_serde_roundtrip(stage in arb_run_stage()) {
            let json = serde_json::to_string(&stage).expect("serialize");
            let back: RunStage = serde_json::from_str(&json).expect("deserialize");
            prop_assert_eq!(stage, back);
        }

        /// EventSeverity serde round-trip is identity
        #[test]
        fn prop_event_severity_serde_roundtrip(sev in arb_event_severity()) {
            let json = serde_json::to_string(&sev).expect("serialize");
            let back: EventSeverity = serde_json::from_str(&json).expect("deserialize");
            prop_assert_eq!(sev, back);
        }

        /// WorkspaceState serde round-trip is identity
        #[test]
        fn prop_workspace_state_serde_roundtrip(state in arb_workspace_state()) {
            let json = serde_json::to_string(&state).expect("serialize");
            let back: WorkspaceState = serde_json::from_str(&json).expect("deserialize");
            prop_assert_eq!(state, back);
        }

        /// RunState serde round-trip preserves all fields
        #[test]
        fn prop_run_state_serde_roundtrip(
            stage in arb_run_stage(),
            agent in "[a-z][a-z0-9-]{0,20}",
            ws_id in "[a-z]{2}-[a-z0-9]{6}",
            hash in proptest::option::of("[a-f0-9]{64}"),
        ) {
            let state = RunState {
                stage,
                agent: agent.clone(),
                workspace_id: ws_id.clone(),
                started_at: Utc::now(),
                image_sha256: hash.clone(),
            };
            let json = serde_json::to_string(&state).expect("serialize");
            let back: RunState = serde_json::from_str(&json).expect("deserialize");
            prop_assert_eq!(state.stage, back.stage);
            prop_assert_eq!(state.agent, back.agent);
            prop_assert_eq!(state.workspace_id, back.workspace_id);
            prop_assert_eq!(state.image_sha256, back.image_sha256);
        }

        /// SecurityLevel serde round-trip is identity
        #[test]
        fn prop_security_level_serde_roundtrip(level in arb_security_level()) {
            let json = serde_json::to_string(&level).expect("serialize");
            let back: SecurityLevel = serde_json::from_str(&json).expect("deserialize");
            prop_assert_eq!(level, back);
        }

        /// SecurityLevel methods return consistent values
        #[test]
        fn prop_security_level_auto_allow_known_relaxed_and_balanced(level in arb_security_level()) {
            match level {
                SecurityLevel::Relaxed | SecurityLevel::Balanced => prop_assert!(level.auto_allow_known()),
                SecurityLevel::Strict => prop_assert!(!level.auto_allow_known()),
            }
        }

        /// migrate_security_level always returns a valid SecurityLevel
        #[test]
        fn prop_migrate_security_level_returns_valid(input in "\\PC{0,50}") {
            let (level, _) = migrate_security_level(&input);
            // Must be one of the two valid variants
            prop_assert!(level == SecurityLevel::Balanced || level == SecurityLevel::Strict);
        }

        /// migrate_security_level is case-insensitive for valid inputs
        #[test]
        fn prop_migrate_security_level_case_insensitive(level in arb_security_level()) {
            let name = match level {
                SecurityLevel::Relaxed => "relaxed",
                SecurityLevel::Balanced => "balanced",
                SecurityLevel::Strict => "strict",
            };
            let (result_lower, migrated_lower) = migrate_security_level(name);
            let (result_upper, migrated_upper) = migrate_security_level(&name.to_uppercase());
            prop_assert_eq!(result_lower, result_upper);
            prop_assert_eq!(migrated_lower, migrated_upper);
            prop_assert!(!migrated_lower, "valid input should not trigger migration");
        }
    }
}
