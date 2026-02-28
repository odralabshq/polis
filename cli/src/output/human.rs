//! Human-readable terminal renderer.

use owo_colors::OwoColorize as _;
use polis_common::types::StatusOutput;
use serde_json::Value as JsonValue;

use crate::domain::health::DoctorChecks;
use crate::output::OutputContext;

/// Renders domain types as human-readable terminal output using `OutputContext`.
pub struct HumanRenderer<'a> {
    ctx: &'a OutputContext,
}

impl<'a> HumanRenderer<'a> {
    /// Create a new `HumanRenderer` wrapping the given output context.
    #[must_use]
    pub fn new(ctx: &'a OutputContext) -> Self {
        Self { ctx }
    }

    /// Render workspace/agent/security status.
    pub fn render_status(&self, status: &StatusOutput) {
        use crate::commands::status::{agent_health_display, workspace_state_display};

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
            use crate::commands::status::format_uptime;
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
    ///
    /// `agents` is a JSON array of objects with `name`, `version`, `description`, `active` fields.
    pub fn render_agents(&self, agents: &[JsonValue]) {
        if agents.is_empty() {
            if !self.ctx.quiet {
                println!("No agents installed. Install one: polis agent add --path <folder>");
            }
            return;
        }

        println!("Available agents:\n");
        for agent in agents {
            let name = agent["name"].as_str().unwrap_or("(unknown)");
            let version = agent["version"].as_str().unwrap_or("");
            let desc = agent["description"].as_str().unwrap_or("");
            let marker = if agent["active"].as_bool().unwrap_or(false) {
                "  [active]"
            } else {
                ""
            };
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
            "Process isolation active",
        );
        self.print_check(
            checks.security.traffic_inspection,
            "Traffic inspection responding",
        );
        self.print_check(
            checks.security.malware_db_current,
            &format!(
                "Malware scanner database current (updated: {}h ago)",
                checks.security.malware_db_age_hours,
            ),
        );
        let expire_days = checks.security.certificates_expire_days;
        if expire_days > 30 {
            self.print_check(true, "Certificates valid (no immediate action required)");
        } else if expire_days > 0 {
            println!(
                "    {} Certificates expire soon",
                "\u{26a0}".style(self.ctx.styles.warning)
            );
        } else {
            self.print_check(false, "Certificates expired");
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
