//! Human-readable terminal renderer.

use polis_common::types::{AgentHealth, WorkspaceState};
use owo_colors::OwoColorize as _;
use polis_common::types::StatusOutput;

use crate::domain::health::DoctorChecks;
use crate::output::OutputContext;

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

        println!();
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
            println!();
            self.ctx
                .warn(&format!("{} security events", status.events.count));
            self.ctx.info("Run: polis logs --security");
        }
    }

    /// Render the list of installed agents.
    pub fn render_agent_list(&self, agents: &[crate::domain::agent::AgentInfo]) {
        if agents.is_empty() {
            if !self.ctx.quiet {
                println!("No agents installed. Install one: polis agent add --path <folder>");
            }
            return;
        }

        println!("Available agents:\n");
        for agent in agents {
            let name = &agent.name;
            let version = agent.version.as_deref().unwrap_or("");
            let desc = agent.description.as_deref().unwrap_or("");
            let marker = if agent.active { "  [active]" } else { "" };
            println!("  {name:<16} {version:<10} {desc}{marker}");
        }
        println!("\nStart an agent: polis start --agent <name>");
    }

    /// Render the current polis configuration.
    pub fn render_config(
        &self,
        config: &crate::domain::config::PolisConfig,
        path: &std::path::Path,
    ) {
        println!();
        println!(
            "  {}",
            format!("Configuration ({})", path.display()).style(self.ctx.styles.header)
        );
        println!();
        println!("  {:<20} {}", "security.level:", config.security.level);
        println!();
        println!("  {}", "Environment:".style(self.ctx.styles.bold));
        println!(
            "    {:<18} {}",
            "POLIS_CONFIG:",
            std::env::var("POLIS_CONFIG").unwrap_or_else(|_| "(not set)".to_string())
        );
        println!(
            "    {:<18} {}",
            "NO_COLOR:",
            std::env::var("NO_COLOR").unwrap_or_else(|_| "(not set)".to_string())
        );
        println!();
    }

    /// Render doctor health check results.
    pub fn render_doctor(&self, checks: &DoctorChecks, issues: &[String], verbose: bool) {
        use owo_colors::OwoColorize;

        println!();
        println!("  {}", "Polis Health Check".style(self.ctx.styles.header));
        println!();

        // Prerequisites
        println!("  Prerequisites:");
        if checks.prerequisites.multipass_found {
            let ver = checks
                .prerequisites
                .multipass_version
                .as_deref()
                .unwrap_or("unknown");
            self.print_check(
                checks.prerequisites.multipass_version_ok,
                &format!("Multipass {ver} (need \u{2265} 1.16.0)"),
            );
            if !checks.prerequisites.multipass_version_ok {
                #[cfg(target_os = "linux")]
                println!("      Update: sudo snap refresh multipass");
                #[cfg(not(target_os = "linux"))]
                println!("      Update: https://multipass.run/install");
            }
            println!();
        } else {
            self.print_check(false, "multipass not found");
            #[cfg(target_os = "linux")]
            println!("      Install: sudo snap install multipass");
            #[cfg(not(target_os = "linux"))]
            println!("      Install: https://multipass.run/install");
            println!();
        }

        // Workspace
        println!("  Workspace:");
        self.print_check(checks.workspace.ready, "Ready to start");
        if checks.workspace.disk_space_ok {
            self.print_check(
                true,
                &format!("{} GB disk space available", checks.workspace.disk_space_gb),
            );
        } else {
            self.print_check(
                false,
                &format!(
                    "Low disk space ({} GB available, need 10 GB)",
                    checks.workspace.disk_space_gb
                ),
            );
        }
        println!();

        // Network
        println!("  Network:");
        self.print_check(checks.network.internet, "Internet connectivity");
        self.print_check(checks.network.dns, "DNS resolution working");
        println!();

        // Security
        self.render_doctor_security(checks);

        // Summary
        println!();
        if issues.is_empty() {
            println!(
                "  {} Everything looks good!",
                "\u{2713}".style(self.ctx.styles.success)
            );
        } else {
            let hint = if verbose {
                ""
            } else {
                " Run with --verbose for details."
            };
            println!(
                "  {} Found {} issues.{hint}",
                "\u{2717}".style(self.ctx.styles.error),
                issues.len(),
            );
            if verbose {
                println!();
                for issue in issues {
                    println!("    {} {issue}", "\u{2717}".style(self.ctx.styles.error));
                }
            }
        }

        println!();
    }

    fn render_doctor_security(&self, checks: &DoctorChecks) {
        use owo_colors::OwoColorize;
        println!("  Security:");
        self.print_check(
            checks.security.process_isolation,
            "process isolation active",
        );
        self.print_check(
            checks.security.traffic_inspection,
            "traffic inspection responding",
        );
        self.print_check(
            checks.security.malware_db_current,
            &format!(
                "malware scanner database current (updated: {}h ago)",
                checks.security.malware_db_age_hours,
            ),
        );
        let expire_days = checks.security.certificates_expire_days;
        if expire_days > 30 {
            self.print_check(true, "certificates valid (no immediate action required)");
        } else if expire_days > 0 {
            println!(
                "    {} certificates expire soon",
                "\u{26a0}".style(self.ctx.styles.warning)
            );
        } else {
            self.print_check(false, "certificates expired");
        }
    }

    fn print_check(&self, ok: bool, msg: &str) {
        use owo_colors::OwoColorize;
        if ok {
            println!("    {} {msg}", "\u{2713}".style(self.ctx.styles.success));
        } else {
            println!("    {} {msg}", "\u{2717}".style(self.ctx.styles.error));
        }
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
    use crate::application::services::workspace_status::workspace_unknown;
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
        assert_eq!(workspace_state_display(WorkspaceState::NotFound), "not found");
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
}
