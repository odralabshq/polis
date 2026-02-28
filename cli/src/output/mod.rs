//! Output formatting module

#![allow(dead_code)] // Presentation layer helpers — not all adopted by every command

pub mod human;
pub mod json;
pub mod progress;
pub mod reporter;
pub mod styles;

use console::Term;
pub use human::HumanRenderer;
pub use json::JsonRenderer;
use owo_colors::OwoColorize as _;
pub use styles::Styles;

use anyhow::Result;
use polis_common::types::StatusOutput;


use crate::domain::health::DoctorChecks;

/// Enum-dispatched output renderer.
///
/// Use `AppContext::renderer()` to obtain the appropriate variant based on
/// the active `OutputMode`. Enum dispatch (not trait objects) gives
/// zero-overhead rendering.
pub enum Renderer<'a> {
    /// Human-readable terminal output.
    Human(HumanRenderer<'a>),
    /// Machine-readable JSON output.
    Json(JsonRenderer),
}

impl Renderer<'_> {

    /// Render the CLI version information.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_version(&self, version: &str, commit: &str, build_date: &str) -> Result<()> {
        match self {
            Renderer::Human(r) => {
                r.render_version(version, commit, build_date);
                Ok(())
            }
            Renderer::Json(_) => JsonRenderer::render_version(version, commit, build_date),
        }
    }
    /// Render workspace/agent/security status.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_status(&self, status: &StatusOutput) -> Result<()> {
        match self {
            Renderer::Human(r) => {
                r.render_status(status);
                Ok(())
            }
            Renderer::Json(_) => JsonRenderer::render_status(status),
        }
    }

    /// Render the list of installed agents.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_agent_list(&self, agents: &[crate::domain::agent::AgentInfo]) -> Result<()> {
        match self {
            Renderer::Human(r) => {
                r.render_agent_list(agents);
                Ok(())
            }
            Renderer::Json(_) => JsonRenderer::render_agent_list(agents),
        }
    }

    /// Render the current polis configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_config(
        &self,
        config: &crate::domain::config::PolisConfig,
        path: &std::path::Path,
    ) -> Result<()> {
        match self {
            Renderer::Human(r) => {
                r.render_config(config, path);
                Ok(())
            }
            Renderer::Json(_) => JsonRenderer::render_config(config),
        }
    }

    /// Render doctor health check results.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_doctor(
        &self,
        checks: &DoctorChecks,
        issues: &[String],
        verbose: bool,
    ) -> Result<()> {
        match self {
            Renderer::Human(r) => {
                r.render_doctor(checks, issues, verbose);
                Ok(())
            }
            Renderer::Json(_) => JsonRenderer::render_doctor(checks, issues),
        }
    }
}

/// Output context carrying styling and terminal state.
pub struct OutputContext {
    /// Stylesheet for colored output.
    pub styles: Styles,
    /// Whether stdout is a TTY.
    pub is_tty: bool,
    /// Whether to suppress non-error output.
    pub quiet: bool,
}

impl OutputContext {
    /// Create output context based on CLI flags and environment.
    #[must_use]
    pub fn new(no_color: bool, quiet: bool) -> Self {
        let is_tty = Term::stdout().is_term();
        let use_colors = !no_color && is_tty && std::env::var("NO_COLOR").is_err();

        let mut styles = Styles::default();
        if use_colors {
            styles.colorize();
        }

        Self {
            styles,
            is_tty,
            quiet,
        }
    }

    /// Check if progress indicators should be shown.
    #[must_use]
    pub fn show_progress(&self) -> bool {
        self.is_tty && !self.quiet
    }

    /// Print a success message prefixed with `✓`. Suppressed when `quiet`.
    pub fn success(&self, msg: &str) {
        if !self.quiet {
            println!("  {} {msg}", "✓".style(self.styles.success));
        }
    }

    /// Print a warning message prefixed with `⚠`. Suppressed when `quiet`.
    pub fn warn(&self, msg: &str) {
        if !self.quiet {
            println!("  {} {msg}", "⚠".style(self.styles.warning));
        }
    }

    /// Print an error message prefixed with `✗` to stderr. Never suppressed.
    pub fn error(&self, msg: &str) {
        eprintln!("  {} {msg}", "✗".style(self.styles.error));
    }

    /// Print an info message prefixed with `ℹ`. Suppressed when `quiet`.
    pub fn info(&self, msg: &str) {
        if !self.quiet {
            println!("  {} {msg}", "ℹ".style(self.styles.info));
        }
    }

    /// Print a section header. Suppressed when `quiet`.
    pub fn header(&self, msg: &str) {
        if !self.quiet {
            println!("  {}", msg.style(self.styles.header));
        }
    }

    /// Print a key-value pair with the key dimmed. Suppressed when `quiet`.
    pub fn kv(&self, key: &str, value: &str) {
        if !self.quiet {
            println!("  {}  {value}", key.style(self.styles.dim));
        }
    }

    /// Print the three core guarantees (governance, security, observability).
    pub fn guarantees(&self) {
        if self.quiet {
            return;
        }
        println!(
            "  {}  policy engine active · audit trail recording",
            "[governance]   ".style(self.styles.governance)
        );
        println!(
            "  {}  workspace isolated · traffic inspection enabled",
            "[security]     ".style(self.styles.security)
        );
        println!(
            "  {}  action tracing live · trust scoring active",
            "[observability]".style(self.styles.observability)
        );
    }
}
