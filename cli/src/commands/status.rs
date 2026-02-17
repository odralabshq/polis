//! Status command implementation.
//!
//! Displays workspace state, agent health, security status, and metrics.

#![allow(dead_code)] // Functions will be used when command is wired up

use polis_common::types::{AgentHealth, MetricsSnapshot, WorkspaceState, WorkspaceStatus};

/// Format uptime seconds as human-readable string.
///
/// Returns "Xh Ym" if hours > 0, otherwise "Xm".
#[must_use]
pub fn format_uptime(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;

    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

/// Convert workspace state to display string.
#[must_use]
pub fn workspace_state_display(state: WorkspaceState) -> &'static str {
    match state {
        WorkspaceState::Running => "running",
        WorkspaceState::Stopped => "stopped",
        WorkspaceState::Starting => "starting",
        WorkspaceState::Stopping => "stopping",
        WorkspaceState::Error => "error",
    }
}

/// Convert agent health to display string.
#[must_use]
pub fn agent_health_display(health: AgentHealth) -> &'static str {
    match health {
        AgentHealth::Healthy => "healthy",
        AgentHealth::Unhealthy => "unhealthy",
        AgentHealth::Starting => "starting",
        AgentHealth::Stopped => "stopped",
    }
}

/// Format agent status line for human-readable output.
///
/// Returns "name (health)" format, e.g., "claude-dev (healthy)".
#[must_use]
pub fn format_agent_line(name: &str, health: AgentHealth) -> String {
    format!("{name} ({health})", health = agent_health_display(health))
}

/// Format security events warning message.
///
/// Returns warning text with count and hint to run `polis logs --security`.
#[must_use]
pub fn format_events_warning(count: u32) -> String {
    let noun = if count == 1 { "event" } else { "events" };
    format!("{count} security {noun}\nRun: polis logs --security")
}

/// Error type for status command failures.
#[derive(Debug, Clone, Copy)]
pub enum StatusError {
    /// Valkey/Redis connection failed
    ValkeyUnreachable,
    /// Could not determine workspace status
    WorkspaceUnknown,
}

impl std::fmt::Display for StatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ValkeyUnreachable => write!(f, "metrics unavailable: valkey unreachable"),
            Self::WorkspaceUnknown => write!(f, "workspace status unknown"),
        }
    }
}

/// Return default metrics when Valkey is unreachable.
#[must_use]
pub fn metrics_unavailable() -> MetricsSnapshot {
    MetricsSnapshot::default()
}

/// Return unknown workspace status when check fails.
#[must_use]
pub fn workspace_unknown() -> WorkspaceStatus {
    WorkspaceStatus {
        status: WorkspaceState::Error,
        uptime_seconds: None,
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)] // Tests use expect for clarity
mod tests {
    use super::*;
    use polis_common::types::{
        AgentHealth, AgentStatus, EventSeverity, MetricsSnapshot, SecurityEvents,
        SecurityStatus, StatusOutput, WorkspaceState, WorkspaceStatus,
    };

    // =========================================================================
    // format_uptime tests
    // =========================================================================

    #[test]
    fn test_format_uptime_hours_and_minutes() {
        // 2h 34m = 2*3600 + 34*60 = 7200 + 2040 = 9240
        assert_eq!(format_uptime(9240), "2h 34m");
    }

    #[test]
    fn test_format_uptime_minutes_only() {
        // 5m = 300s, should show "5m" not "0h 5m"
        assert_eq!(format_uptime(300), "5m");
    }

    #[test]
    fn test_format_uptime_zero_seconds() {
        assert_eq!(format_uptime(0), "0m");
    }

    #[test]
    fn test_format_uptime_exact_hour() {
        // 1h 0m = 3600s
        assert_eq!(format_uptime(3600), "1h 0m");
    }

    #[test]
    fn test_format_uptime_under_minute() {
        // 59 seconds should round down to 0m
        assert_eq!(format_uptime(59), "0m");
    }

    #[test]
    fn test_format_uptime_large_value() {
        // 24h = 86400s
        assert_eq!(format_uptime(86400), "24h 0m");
    }

    #[test]
    fn test_format_uptime_one_minute() {
        assert_eq!(format_uptime(60), "1m");
    }

    #[test]
    fn test_format_uptime_one_hour_one_minute() {
        // 1h 1m = 3660s
        assert_eq!(format_uptime(3660), "1h 1m");
    }

    // =========================================================================
    // workspace_state_display tests
    // =========================================================================

    #[test]
    fn test_workspace_state_display_running() {
        assert_eq!(workspace_state_display(WorkspaceState::Running), "running");
    }

    #[test]
    fn test_workspace_state_display_stopped() {
        assert_eq!(workspace_state_display(WorkspaceState::Stopped), "stopped");
    }

    #[test]
    fn test_workspace_state_display_starting() {
        assert_eq!(workspace_state_display(WorkspaceState::Starting), "starting");
    }

    #[test]
    fn test_workspace_state_display_stopping() {
        assert_eq!(workspace_state_display(WorkspaceState::Stopping), "stopping");
    }

    #[test]
    fn test_workspace_state_display_error() {
        assert_eq!(workspace_state_display(WorkspaceState::Error), "error");
    }

    // =========================================================================
    // agent_health_display tests
    // =========================================================================

    #[test]
    fn test_agent_health_display_healthy() {
        assert_eq!(agent_health_display(AgentHealth::Healthy), "healthy");
    }

    #[test]
    fn test_agent_health_display_unhealthy() {
        assert_eq!(agent_health_display(AgentHealth::Unhealthy), "unhealthy");
    }

    #[test]
    fn test_agent_health_display_starting() {
        assert_eq!(agent_health_display(AgentHealth::Starting), "starting");
    }

    #[test]
    fn test_agent_health_display_stopped() {
        assert_eq!(agent_health_display(AgentHealth::Stopped), "stopped");
    }

    // =========================================================================
    // format_agent_line tests
    // =========================================================================

    #[test]
    fn test_format_agent_line_healthy() {
        assert_eq!(
            format_agent_line("claude-dev", AgentHealth::Healthy),
            "claude-dev (healthy)"
        );
    }

    #[test]
    fn test_format_agent_line_unhealthy() {
        assert_eq!(
            format_agent_line("test-agent", AgentHealth::Unhealthy),
            "test-agent (unhealthy)"
        );
    }

    #[test]
    fn test_format_agent_line_starting() {
        assert_eq!(
            format_agent_line("my-agent", AgentHealth::Starting),
            "my-agent (starting)"
        );
    }

    // =========================================================================
    // format_events_warning tests
    // =========================================================================

    #[test]
    fn test_format_events_warning_single() {
        let warning = format_events_warning(1);
        assert!(warning.contains("1 security event"));
        assert!(warning.contains("polis logs --security"));
    }

    #[test]
    fn test_format_events_warning_plural() {
        let warning = format_events_warning(5);
        assert!(warning.contains("5 security events"));
        assert!(warning.contains("polis logs --security"));
    }

    #[test]
    fn test_format_events_warning_zero() {
        // Zero events should still format (caller decides whether to show)
        let warning = format_events_warning(0);
        assert!(warning.contains("0 security events"));
    }

    // =========================================================================
    // JSON output tests
    // =========================================================================

    #[test]
    fn test_status_output_json_contains_workspace_status() {
        let status = create_test_status();
        let json = serde_json::to_string(&status).expect("serialize StatusOutput");
        assert!(json.contains(r#""status":"running""#));
    }

    #[test]
    fn test_status_output_json_contains_uptime_seconds() {
        let status = create_test_status();
        let json = serde_json::to_string(&status).expect("serialize StatusOutput");
        assert!(json.contains(r#""uptime_seconds":9240"#));
    }

    #[test]
    fn test_status_output_json_contains_agent_name() {
        let status = create_test_status();
        let json = serde_json::to_string(&status).expect("serialize StatusOutput");
        assert!(json.contains(r#""name":"claude-dev""#));
    }

    #[test]
    fn test_status_output_json_contains_agent_status() {
        let status = create_test_status();
        let json = serde_json::to_string(&status).expect("serialize StatusOutput");
        assert!(json.contains(r#""status":"healthy""#));
    }

    #[test]
    fn test_status_output_json_contains_security_fields() {
        let status = create_test_status();
        let json = serde_json::to_string(&status).expect("serialize StatusOutput");
        assert!(json.contains(r#""traffic_inspection":true"#));
        assert!(json.contains(r#""credential_protection":true"#));
        assert!(json.contains(r#""malware_scanning":true"#));
    }

    #[test]
    fn test_status_output_json_contains_metrics() {
        let status = create_test_status();
        let json = serde_json::to_string(&status).expect("serialize StatusOutput");
        assert!(json.contains(r#""requests_inspected":142"#));
        assert!(json.contains(r#""blocked_credentials":0"#));
        assert!(json.contains(r#""blocked_malware":0"#));
    }

    #[test]
    fn test_status_output_json_contains_events() {
        let status = create_test_status();
        let json = serde_json::to_string(&status).expect("serialize StatusOutput");
        assert!(json.contains(r#""count":2"#));
        assert!(json.contains(r#""severity":"warning""#));
    }

    #[test]
    fn test_status_output_json_omits_agent_when_none() {
        let status = StatusOutput {
            workspace: WorkspaceStatus {
                status: WorkspaceState::Stopped,
                uptime_seconds: None,
            },
            agent: None,
            security: SecurityStatus {
                traffic_inspection: false,
                credential_protection: false,
                malware_scanning: false,
            },
            metrics: MetricsSnapshot::default(),
            events: SecurityEvents {
                count: 0,
                severity: EventSeverity::None,
            },
        };
        let json = serde_json::to_string(&status).expect("serialize StatusOutput");
        assert!(!json.contains(r#""agent""#), "agent field should be omitted when None");
    }

    #[test]
    fn test_status_output_json_omits_uptime_when_none() {
        let status = StatusOutput {
            workspace: WorkspaceStatus {
                status: WorkspaceState::Stopped,
                uptime_seconds: None,
            },
            agent: None,
            security: SecurityStatus {
                traffic_inspection: false,
                credential_protection: false,
                malware_scanning: false,
            },
            metrics: MetricsSnapshot::default(),
            events: SecurityEvents {
                count: 0,
                severity: EventSeverity::None,
            },
        };
        let json = serde_json::to_string(&status).expect("serialize StatusOutput");
        assert!(
            !json.contains("uptime_seconds"),
            "uptime_seconds should be omitted when None"
        );
    }

    // =========================================================================
    // Test helpers
    // =========================================================================

    fn create_test_status() -> StatusOutput {
        StatusOutput {
            workspace: WorkspaceStatus {
                status: WorkspaceState::Running,
                uptime_seconds: Some(9240),
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
            metrics: MetricsSnapshot {
                window_start: chrono::Utc::now(),
                requests_inspected: 142,
                blocked_credentials: 0,
                blocked_malware: 0,
            },
            events: SecurityEvents {
                count: 2,
                severity: EventSeverity::Warning,
            },
        }
    }
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[cfg(test)]
mod error_tests {
    use super::*;

    // =========================================================================
    // StatusError type tests
    // =========================================================================

    #[test]
    fn test_status_error_valkey_unreachable_display() {
        let err = StatusError::ValkeyUnreachable;
        let msg = err.to_string();
        assert!(
            msg.to_lowercase().contains("valkey") || msg.to_lowercase().contains("metrics"),
            "error message should mention valkey or metrics"
        );
    }

    #[test]
    fn test_status_error_workspace_unknown_display() {
        let err = StatusError::WorkspaceUnknown;
        let msg = err.to_string();
        assert!(
            msg.to_lowercase().contains("workspace") || msg.to_lowercase().contains("status"),
            "error message should mention workspace or status"
        );
    }

    // =========================================================================
    // Graceful degradation tests
    // =========================================================================

    #[test]
    fn test_metrics_unavailable_returns_default() {
        // When Valkey is unreachable, metrics should return a default/unavailable state
        let result = metrics_unavailable();
        assert_eq!(result.requests_inspected, 0);
        assert_eq!(result.blocked_credentials, 0);
        assert_eq!(result.blocked_malware, 0);
    }

    #[test]
    fn test_workspace_unknown_status() {
        // When workspace status check fails, should return Error state
        let result = workspace_unknown();
        assert_eq!(result.status, polis_common::types::WorkspaceState::Error);
        assert!(result.uptime_seconds.is_none());
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod proptests {
    use super::*;
    use polis_common::types::{AgentHealth, WorkspaceState};
    use proptest::prelude::*;

    fn arb_agent_health() -> impl Strategy<Value = AgentHealth> {
        prop_oneof![
            Just(AgentHealth::Healthy),
            Just(AgentHealth::Unhealthy),
            Just(AgentHealth::Starting),
            Just(AgentHealth::Stopped),
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

    proptest! {
        /// format_uptime always produces valid format
        #[test]
        fn prop_format_uptime_valid_format(seconds in 0u64..=604_800) {
            let result = format_uptime(seconds);
            prop_assert!(result.ends_with('m'), "should end with 'm'");
            prop_assert!(
                result.chars().all(|c| c.is_ascii_digit() || c == 'h' || c == 'm' || c == ' '),
                "should only contain digits, h, m, space"
            );
        }

        /// format_uptime with hours > 0 contains 'h'
        #[test]
        fn prop_format_uptime_hours_contains_h(hours in 1u64..=168, minutes in 0u64..60) {
            let seconds = hours * 3600 + minutes * 60;
            let result = format_uptime(seconds);
            prop_assert!(result.contains('h'), "hours > 0 should contain 'h'");
        }

        /// format_uptime with hours == 0 does not contain 'h'
        #[test]
        fn prop_format_uptime_no_hours_no_h(minutes in 0u64..60) {
            let seconds = minutes * 60;
            let result = format_uptime(seconds);
            prop_assert!(!result.contains('h'), "hours == 0 should not contain 'h'");
        }

        /// format_uptime extracts correct hours
        #[test]
        fn prop_format_uptime_correct_hours(hours in 1u64..=100, minutes in 0u64..60) {
            let seconds = hours * 3600 + minutes * 60;
            let result = format_uptime(seconds);
            let expected_prefix = format!("{hours}h");
            prop_assert!(result.starts_with(&expected_prefix), "should start with correct hours");
        }

        /// format_uptime extracts correct minutes
        #[test]
        fn prop_format_uptime_correct_minutes(minutes in 0u64..60) {
            let seconds = minutes * 60;
            let result = format_uptime(seconds);
            let expected = format!("{minutes}m");
            prop_assert_eq!(result, expected);
        }

        /// format_agent_line always contains agent name
        #[test]
        fn prop_format_agent_line_contains_name(
            name in "[a-z][a-z0-9-]{0,20}",
            health in arb_agent_health()
        ) {
            let result = format_agent_line(&name, health);
            prop_assert!(result.contains(&name), "should contain agent name");
        }

        /// format_agent_line always contains parentheses
        #[test]
        fn prop_format_agent_line_has_parens(
            name in "[a-z][a-z0-9-]{0,20}",
            health in arb_agent_health()
        ) {
            let result = format_agent_line(&name, health);
            prop_assert!(result.contains('(') && result.contains(')'), "should have parentheses");
        }

        /// format_events_warning always mentions polis logs
        #[test]
        fn prop_format_events_warning_has_hint(count in 0u32..1000) {
            let result = format_events_warning(count);
            prop_assert!(result.contains("polis logs"), "should mention polis logs");
        }

        /// format_events_warning uses singular for count == 1
        #[test]
        fn prop_format_events_warning_singular(_seed in 0u32..100) {
            let result = format_events_warning(1);
            prop_assert!(result.contains("1 security event"), "should use singular");
            prop_assert!(!result.contains("events"), "should not use plural");
        }

        /// format_events_warning uses plural for count != 1
        #[test]
        fn prop_format_events_warning_plural(count in 0u32..1000) {
            prop_assume!(count != 1);
            let result = format_events_warning(count);
            prop_assert!(result.contains("events"), "should use plural for count != 1");
        }

        /// workspace_state_display returns lowercase string
        #[test]
        fn prop_workspace_state_display_lowercase(state in arb_workspace_state()) {
            let result = workspace_state_display(state);
            prop_assert!(result.chars().all(|c| c.is_lowercase()), "should be lowercase");
        }

        /// workspace_state_display is non-empty
        #[test]
        fn prop_workspace_state_display_non_empty(state in arb_workspace_state()) {
            let result = workspace_state_display(state);
            prop_assert!(!result.is_empty(), "should not be empty");
        }

        /// agent_health_display returns lowercase string
        #[test]
        fn prop_agent_health_display_lowercase(health in arb_agent_health()) {
            let result = agent_health_display(health);
            prop_assert!(result.chars().all(|c| c.is_lowercase()), "should be lowercase");
        }

        /// agent_health_display is non-empty
        #[test]
        fn prop_agent_health_display_non_empty(health in arb_agent_health()) {
            let result = agent_health_display(health);
            prop_assert!(!result.is_empty(), "should not be empty");
        }

        /// metrics_unavailable returns zero counters
        #[test]
        fn prop_metrics_unavailable_zero_counters(_seed in 0u32..100) {
            let result = metrics_unavailable();
            prop_assert_eq!(result.requests_inspected, 0);
            prop_assert_eq!(result.blocked_credentials, 0);
            prop_assert_eq!(result.blocked_malware, 0);
        }

        /// workspace_unknown returns Error state
        #[test]
        fn prop_workspace_unknown_error_state(_seed in 0u32..100) {
            let result = workspace_unknown();
            prop_assert_eq!(result.status, WorkspaceState::Error);
            prop_assert!(result.uptime_seconds.is_none());
        }
    }
}
