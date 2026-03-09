//! Human-readable terminal renderer.

use owo_colors::OwoColorize as _;
use polis_common::agent::OnboardingStep;
use polis_common::types::StatusOutput;
use polis_common::types::{AgentHealth, WorkspaceState};

use crate::application::ports::UpdateInfo;
use crate::application::services::agent::{ActivateOutcome, AgentOutcome};
use crate::application::services::workspace::DeleteOutcome;
use crate::application::services::workspace::start::StartOutcome;
use crate::application::services::workspace::stop::StopOutcome;
use crate::domain::health::DiagnosticReport;
use crate::output::OutputContext;
use crate::output::models::{ConnectionInfo, LogEntry, PendingRequest, SecurityStatus};

/// Renders domain types as human-readable terminal output using `OutputContext`.
pub struct HumanRenderer<'a> {
    ctx: &'a OutputContext,
}

impl<'a> HumanRenderer<'a> {
    /// Render the CLI version information.
    pub fn render_version(&self, version: &str, build_date: &str) {
        if self.ctx.quiet {
            return;
        }
        self.ctx.info(&format!("polis v{version} ({build_date})"));
    }
    /// Create a new `HumanRenderer` wrapping the given output context.
    #[must_use]
    pub fn new(ctx: &'a OutputContext) -> Self {
        Self { ctx }
    }

    /// Render workspace/agent/security status.
    pub fn render_status(&self, status: &StatusOutput) {
        self.ctx.kv(
            "Workspace:",
            workspace_state_display(status.workspace.status),
        );

        if let Some(agent) = &status.agent {
            self.ctx.kv(
                "Agent:",
                &format!("{} ({})", agent.name, agent_health_display(agent.status)),
            );
        }

        if let Some(uptime) = status.workspace.uptime_seconds {
            self.ctx.kv("Uptime:", &format_uptime(uptime));
        }

        self.ctx.blank();
        self.ctx.header("Security:");

        if status.security.traffic_inspection {
            self.ctx.success("Traffic inspection active");
        } else {
            self.ctx.warn("Traffic inspection inactive");
        }
        if status.security.credential_protection {
            self.ctx.success("Credential protection enabled");
        } else {
            self.ctx.warn("Credential protection disabled");
        }
        if status.security.malware_scanning {
            self.ctx.success("Malware scanning enabled");
        } else {
            self.ctx.warn("Malware scanning disabled");
        }

        if status.events.count > 0 {
            self.ctx.blank();
            self.ctx
                .warn(&format!("{} security events", status.events.count));
            self.ctx.info("Run: polis logs --security");
        }
    }

    /// Render the list of installed agents.
    pub fn render_agent_list(&self, agents: &[crate::domain::agent::AgentInfo]) {
        use owo_colors::OwoColorize;

        if agents.is_empty() {
            self.ctx
                .write_raw("No agents installed. Install one: polis agent install --path <folder>");
            return;
        }

        self.ctx.write_raw("Installed agents:\n");
        for agent in agents {
            let name = &agent.name;
            let version = agent.version.as_deref().unwrap_or("");
            let desc = agent.description.as_deref().unwrap_or("");
            let marker = if agent.active { "  [active]" } else { "" };
            self.ctx
                .write_raw(&format!("  {name:<16} {version:<10} {desc}{marker}"));

            // Display warning if present (e.g., malformed manifest)
            if let Some(warning) = &agent.warning {
                self.ctx.write_raw(&format!(
                    "    {} {warning}",
                    "!".style(self.ctx.styles.warning)
                ));
            }
        }
        self.ctx
            .write_raw("\nActivate an agent: polis agent activate <name>");
    }

    /// Render agent activation outcome with optional onboarding steps.
    pub fn render_agent_activated(&self, agent: &str, already_active: bool) {
        if already_active {
            self.ctx.info(&format!("Agent '{agent}' is already active"));
        } else {
            self.ctx.success(&format!("Agent '{agent}' activated"));
        }
    }

    /// Render onboarding steps for an activated agent.
    pub fn render_onboarding(&self, steps: &[polis_common::agent::OnboardingStep]) {
        if steps.is_empty() || self.ctx.quiet {
            return;
        }
        self.ctx.blank();
        self.ctx.header("Getting started");
        for (i, step) in steps.iter().enumerate() {
            self.ctx.info(&format!(
                "{}. {}  {}",
                i + 1,
                step.title,
                step.command.style(self.ctx.styles.command)
            ));
        }
    }

    /// Render agent activation outcome with health warning and onboarding.
    pub fn render_activate_outcome(&self, outcome: &ActivateOutcome) {
        let (o, unhealthy) = match outcome {
            ActivateOutcome::Activated(o) | ActivateOutcome::AlreadyActive(o) => (o, false),
            ActivateOutcome::ActivatedUnhealthy(o) => (o, true),
            ActivateOutcome::SwapRequired { .. } => return,
        };
        if unhealthy {
            self.ctx
                .warn("Agent activated but health check timed out — it may not be ready yet.");
        }
        match o {
            AgentOutcome::Activated { agent, onboarding } => {
                self.render_agent_activated(agent, false);
                self.render_onboarding(onboarding);
            }
            AgentOutcome::AlreadyActive { agent, onboarding } => {
                self.render_agent_activated(agent, true);
                self.render_onboarding(onboarding);
            }
        }
    }

    /// Render the current polis configuration.
    pub fn render_config(
        &self,
        config: &crate::domain::config::PolisConfig,
        path: &std::path::Path,
        config_env: &crate::output::ConfigEnv,
    ) {
        self.ctx.blank();
        self.ctx.header(&format!("Configuration ({})", path.display()));
        self.ctx.blank();
        self.ctx
            .write_raw(&format!("  {:<20} {}", "security.level:", config.security.level));
        self.ctx.blank();
        self.ctx
            .write_raw(&format!("  {}", "Environment:".style(self.ctx.styles.bold)));
        self.ctx.write_raw(&format!(
            "    {:<18} {}",
            "POLIS_CONFIG:",
            config_env
                .polis_config
                .as_deref()
                .unwrap_or("(not set)")
        ));
        self.ctx.write_raw(&format!(
            "    {:<18} {}",
            "NO_COLOR:",
            config_env.no_color.as_deref().unwrap_or("(not set)")
        ));
        self.ctx.blank();
    }

    /// Render diagnostic health check results.
    pub fn render_diagnostics(&self, checks: &DiagnosticReport, issues: &[String], verbose: bool) {
        self.ctx.blank();
        self.ctx.header("Polis Health Check");
        self.ctx.blank();

        // Prerequisites
        self.render_diagnostics_prerequisites(checks);

        // Workspace
        self.ctx.write_raw("  Workspace:");
        self.ctx.write_check(checks.workspace.ready, "Ready to start");
        if checks.workspace.disk_space_ok {
            self.ctx.write_check(
                true,
                &format!("{} GB disk space available", checks.workspace.disk_space_gb),
            );
        } else {
            self.ctx.write_check(
                false,
                &format!(
                    "Low disk space ({} GB available, need 10 GB)",
                    checks.workspace.disk_space_gb
                ),
            );
        }
        // Image cache status
        if let Some(ref override_val) = checks.workspace.image.polis_image_override {
            self.ctx
                .write_check(true, &format!("Image override: {override_val}"));
        } else {
            self.ctx.write_check(checks.workspace.image.cached, "Image cached");
        }
        self.ctx.blank();

        // Network
        self.ctx.write_raw("  Network:");
        self.ctx
            .write_check(checks.network.internet, "Internet connectivity");
        self.ctx
            .write_check(checks.network.dns, "DNS resolution working");
        self.ctx.blank();

        // Security
        self.render_diagnostics_security(checks);

        // Summary
        self.ctx.blank();
        if issues.is_empty() {
            self.ctx.write_raw(&format!(
                "  {} Everything looks good!",
                "\u{2713}".style(self.ctx.styles.success)
            ));
        } else {
            let hint = if verbose {
                ""
            } else {
                " Run with --verbose for details."
            };
            self.ctx.write_raw(&format!(
                "  {} Found {} issues.{hint}",
                "\u{2717}".style(self.ctx.styles.error),
                issues.len(),
            ));
            if verbose {
                self.ctx.blank();
                for issue in issues {
                    self.ctx.write_raw(&format!(
                        "    {} {issue}",
                        "\u{2717}".style(self.ctx.styles.error)
                    ));
                }
            }
        }

        self.ctx.blank();
    }

    fn render_diagnostics_prerequisites(&self, checks: &DiagnosticReport) {
        self.ctx.write_raw("  Prerequisites:");
        if checks.prerequisites.found {
            let ver = checks.prerequisites.version.as_deref().unwrap_or("unknown");
            self.ctx.write_check(
                checks.prerequisites.version_ok,
                &format!("Multipass {ver} (need \u{2265} 1.16.0)"),
            );
            if !checks.prerequisites.version_ok {
                #[cfg(target_os = "linux")]
                self.ctx
                    .write_raw("      Update: sudo snap refresh multipass");
                #[cfg(not(target_os = "linux"))]
                self.ctx
                    .write_raw("      Update: https://multipass.run/install");
            }
        } else {
            self.ctx.write_check(false, "multipass not found");
            #[cfg(target_os = "linux")]
            self.ctx
                .write_raw("      Install: sudo snap install multipass");
            #[cfg(not(target_os = "linux"))]
            self.ctx
                .write_raw("      Install: https://multipass.run/install");
        }
        self.ctx.blank();
    }

    fn render_diagnostics_security(&self, checks: &DiagnosticReport) {
        self.ctx.write_raw("  Security:");
        self.ctx.write_check(
            checks.security.process_isolation,
            "process isolation active",
        );
        self.ctx.write_check(
            checks.security.traffic_inspection,
            "traffic inspection responding",
        );
        self.ctx.write_check(
            checks.security.malware_db.is_current,
            &format!(
                "malware scanner database current (updated: {}h ago)",
                checks.security.malware_db.age_hours,
            ),
        );
        let expire_days = checks.security.certificates.expire_days;
        if expire_days > 30 {
            self.ctx
                .write_check(true, "certificates valid (no immediate action required)");
        } else if expire_days > 0 {
            self.ctx.write_raw(&format!(
                "    {} certificates expire soon",
                "!".style(self.ctx.styles.warning)
            ));
        } else {
            self.ctx.write_check(false, "certificates expired");
        }
    }

    fn print_check(&self, ok: bool, msg: &str) {
        self.ctx.write_check(ok, msg);
    }

    /// Render connection info (SSH, VS Code, Cursor).
    pub fn render_connection_info(&self, info: &ConnectionInfo) {
        self.ctx.blank();
        self.ctx.kv("SSH     ", &info.ssh);
        self.ctx.kv("VS Code ", &info.vscode);
        self.ctx.kv("Cursor  ", &info.cursor);
    }

    /// Render stop command outcome.
    pub fn render_stop_outcome(&self, outcome: &StopOutcome) {
        match outcome {
            StopOutcome::NotFound => {
                self.ctx.info("No workspace to stop.");
                self.ctx.info("Create one: polis start");
            }
            StopOutcome::AlreadyStopped => {
                self.ctx.info("Workspace is already stopped.");
                self.ctx.info("Resume: polis start");
            }
            StopOutcome::Stopped => {
                self.ctx.info("Your data is preserved.");
                self.ctx.info("Resume: polis start");
            }
        }
    }

    /// Render delete command outcome.
    pub fn render_delete_outcome(&self, outcome: &DeleteOutcome, all: bool) {
        match outcome {
            DeleteOutcome::NotFound => {
                self.ctx.success("no workspace to delete");
            }
            DeleteOutcome::Deleted => {
                if all {
                    self.ctx.success("all data removed");
                } else {
                    self.ctx.success("workspace removed");
                }
            }
        }
    }

    /// Render start command outcome.
    pub fn render_start_outcome(&self, outcome: &StartOutcome, onboarding: &[OnboardingStep]) {
        match outcome {
            StartOutcome::AlreadyRunning { active_agent } => {
                let label = active_agent.as_ref().map_or_else(
                    || "workspace running".to_string(),
                    |n| format!("workspace running with agent: {n}"),
                );
                self.ctx.success(&label);
                self.ctx.blank();
                self.ctx.kv("Connect", "polis connect");
                self.ctx.kv("Status", "polis status");
            }
            StartOutcome::Created { .. } | StartOutcome::Restarted { .. } => {
                self.ctx.blank();
                self.ctx.header("Getting started");
                let default_steps = [
                    OnboardingStep {
                        title: "Connect to workspace:".into(),
                        command: "polis connect or ssh workspace".into(),
                    },
                    OnboardingStep {
                        title: "Manage agents:".into(),
                        command: "polis agent".into(),
                    },
                ];
                for (i, step) in default_steps.iter().chain(onboarding.iter()).enumerate() {
                    self.ctx.info(&format!(
                        "{}. {}  {}",
                        i + 1,
                        step.title,
                        step.command.style(self.ctx.styles.command)
                    ));
                }
            }
        }
    }

    /// Render update info (version comparison).
    pub fn render_update_info(&self, current: &str, info: &UpdateInfo) {
        match info {
            UpdateInfo::UpToDate => {
                self.ctx.success(&format!("CLI v{current} (up to date)"));
            }
            UpdateInfo::Available {
                version,
                release_notes,
                ..
            } => {
                self.ctx
                    .info(&format!("CLI v{current} → v{version} available"));
                if !release_notes.is_empty() && !self.ctx.quiet {
                    self.ctx.info(&format!("  Changes in v{version}:"));
                    for note in release_notes {
                        self.ctx.info(&format!("    • {note}"));
                    }
                }
            }
        }
    }

    /// Render security status.
    pub fn render_security_status(&self, status: &SecurityStatus) {
        self.ctx.info(&format!("Security level: {}", status.level));
        if let Some(err) = &status.pending_error {
            self.ctx
                .warn(&format!("Could not query pending requests: {err}"));
        } else if status.pending_count == 0 {
            self.ctx.success("No pending blocked requests");
        } else {
            self.ctx.warn(&format!(
                "{} pending blocked request(s)",
                status.pending_count
            ));
        }
    }

    /// Render security pending requests.
    pub fn render_security_pending(&self, requests: &[PendingRequest]) {
        if requests.is_empty() {
            self.ctx.info("No pending requests.");
            return;
        }
        self.ctx.header("Pending Requests:");
        for req in requests {
            self.ctx.info(&format!(
                "  {} - {} ({})",
                req.id, req.domain, req.timestamp
            ));
        }
    }

    /// Render security log entries.
    pub fn render_security_log(&self, entries: &[LogEntry]) {
        if entries.is_empty() {
            self.ctx.info("No log entries.");
            return;
        }
        self.ctx.header("Security Log:");
        for entry in entries {
            self.ctx.info(&format!(
                "  [{}] {} - {}",
                entry.timestamp, entry.action, entry.details
            ));
        }
    }

    /// Render security action result (approve/deny/rule/level).
    pub fn render_security_action(&self, message: &str) {
        self.ctx.success(message);
    }
}

// ── Display helpers (used by tests and output layer) ─────────────────────────

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

#[must_use]
pub fn workspace_state_display(state: WorkspaceState) -> &'static str {
    match state {
        WorkspaceState::Running => "running",
        WorkspaceState::Stopped => "stopped",
        WorkspaceState::Starting => "starting",
        WorkspaceState::Stopping => "stopping",
        WorkspaceState::NotFound => "not found",
        WorkspaceState::Error => "error",
    }
}

#[must_use]
pub fn agent_health_display(health: AgentHealth) -> &'static str {
    match health {
        AgentHealth::Healthy => "healthy",
        AgentHealth::Unhealthy => "unhealthy",
        AgentHealth::Starting => "starting",
        AgentHealth::Stopped => "stopped",
    }
}

#[cfg(test)]
#[must_use]
pub fn format_agent_line(name: &str, health: AgentHealth) -> String {
    format!("{name} ({})", agent_health_display(health))
}

#[cfg(test)]
#[must_use]
pub fn format_events_warning(count: u32) -> String {
    let noun = if count == 1 { "event" } else { "events" };
    format!("{count} security {noun}\nRun: polis logs --security")
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::application::services::workspace::workspace_unknown;
    use polis_common::types::{
        AgentHealth, AgentStatus, EventSeverity, SecurityEvents, SecurityStatus, StatusOutput,
        WorkspaceState, WorkspaceStatus,
    };

    #[test]
    fn test_format_uptime_hours_and_minutes() {
        assert_eq!(format_uptime(9240), "2h 34m");
    }

    #[test]
    fn test_format_uptime_minutes_only() {
        assert_eq!(format_uptime(300), "5m");
    }

    #[test]
    fn test_format_uptime_zero() {
        assert_eq!(format_uptime(0), "0m");
    }

    #[test]
    fn test_workspace_state_display_all() {
        assert_eq!(workspace_state_display(WorkspaceState::Running), "running");
        assert_eq!(workspace_state_display(WorkspaceState::Stopped), "stopped");
        assert_eq!(
            workspace_state_display(WorkspaceState::Starting),
            "starting"
        );
        assert_eq!(
            workspace_state_display(WorkspaceState::Stopping),
            "stopping"
        );
        assert_eq!(
            workspace_state_display(WorkspaceState::NotFound),
            "not found"
        );
        assert_eq!(workspace_state_display(WorkspaceState::Error), "error");
    }

    #[test]
    fn test_agent_health_display_all() {
        assert_eq!(agent_health_display(AgentHealth::Healthy), "healthy");
        assert_eq!(agent_health_display(AgentHealth::Unhealthy), "unhealthy");
        assert_eq!(agent_health_display(AgentHealth::Starting), "starting");
        assert_eq!(agent_health_display(AgentHealth::Stopped), "stopped");
    }

    #[test]
    fn test_format_agent_line() {
        assert_eq!(
            format_agent_line("claude-dev", AgentHealth::Healthy),
            "claude-dev (healthy)"
        );
    }

    #[test]
    fn test_format_events_warning_singular() {
        assert!(format_events_warning(1).contains("1 security event\n"));
    }

    #[test]
    fn test_format_events_warning_plural() {
        assert!(format_events_warning(2).contains("2 security events"));
    }

    #[test]
    fn test_workspace_unknown() {
        let ws = workspace_unknown();
        assert_eq!(ws.status, WorkspaceState::Error);
        assert!(ws.uptime_seconds.is_none());
    }

    fn test_status() -> StatusOutput {
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
            events: SecurityEvents {
                count: 2,
                severity: EventSeverity::Warning,
            },
        }
    }

    #[test]
    fn test_status_json_roundtrip() {
        let status = test_status();
        let json = serde_json::to_string(&status).expect("serialize");
        let back: StatusOutput = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.workspace.status, WorkspaceState::Running);
        assert_eq!(back.events.count, 2);
    }

    #[test]
    fn test_status_json_omits_none_fields() {
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
            events: SecurityEvents {
                count: 0,
                severity: EventSeverity::None,
            },
        };
        let json = serde_json::to_string(&status).expect("serialize");
        assert!(!json.contains("uptime_seconds"));
        assert!(!json.contains(r#""agent""#));
    }

    // ── HumanRenderer edge case tests ─────────────────────────────────────────

    #[test]
    fn test_render_agent_list_empty_produces_no_agents_message() {
        use crate::output::test_support::make_test_ctx;
        let (ctx, stdout, _) = make_test_ctx(false);
        let renderer = HumanRenderer::new(&ctx);
        renderer.render_agent_list(&[]);
        let out = crate::output::test_support::buf_to_string(&stdout);
        assert!(out.contains("No agents installed"), "got: {out}");
    }

    #[test]
    fn test_render_version_contains_version_and_date() {
        use crate::output::test_support::make_test_ctx;
        let (ctx, stdout, _) = make_test_ctx(false);
        let renderer = HumanRenderer::new(&ctx);
        renderer.render_version("1.2.3", "2024-06-01");
        let out = crate::output::test_support::buf_to_string(&stdout);
        assert!(out.contains("1.2.3"), "got: {out}");
        assert!(out.contains("2024-06-01"), "got: {out}");
    }

    #[test]
    fn test_render_config_none_polis_config_shows_not_set() {
        use crate::output::test_support::make_test_ctx;
        use crate::output::ConfigEnv;
        use crate::domain::config::PolisConfig;
        use std::path::Path;

        let (ctx, stdout, _) = make_test_ctx(false);
        let renderer = HumanRenderer::new(&ctx);
        let config = PolisConfig::default();
        let config_env = ConfigEnv {
            polis_config: None,
            no_color: None,
        };
        renderer.render_config(&config, Path::new("/etc/polis.yaml"), &config_env);
        let out = crate::output::test_support::buf_to_string(&stdout);
        assert!(out.contains("(not set)"), "got: {out}");
    }

    #[test]
    fn test_render_config_with_values_displays_them() {
        use crate::output::test_support::make_test_ctx;
        use crate::output::ConfigEnv;
        use crate::domain::config::PolisConfig;
        use std::path::Path;

        let (ctx, stdout, _) = make_test_ctx(false);
        let renderer = HumanRenderer::new(&ctx);
        let config = PolisConfig::default();
        let config_env = ConfigEnv {
            polis_config: Some("/custom/path.yaml".to_string()),
            no_color: Some("1".to_string()),
        };
        renderer.render_config(&config, Path::new("/etc/polis.yaml"), &config_env);
        let out = crate::output::test_support::buf_to_string(&stdout);
        assert!(out.contains("/custom/path.yaml"), "got: {out}");
        assert!(out.contains('1'), "got: {out}");
    }
}
