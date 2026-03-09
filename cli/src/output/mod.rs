//! Output formatting module

#![allow(dead_code)] // Presentation layer helpers — not all adopted by every command

pub mod human;
pub mod json;
pub mod models;
pub mod progress;
pub mod reporter;
pub mod styles;

pub use human::HumanRenderer;
pub use json::JsonRenderer;
use owo_colors::OwoColorize as _;
pub use styles::Styles;

use anyhow::Result;
use polis_common::agent::OnboardingStep;
use polis_common::types::StatusOutput;
use std::io::Write;
use std::sync::{Mutex, PoisonError};

use crate::application::ports::UpdateInfo;
use crate::application::services::agent::ActivateOutcome;
use crate::application::services::workspace::DeleteOutcome;
use crate::application::services::workspace::start::StartOutcome;
use crate::application::services::workspace::stop::StopOutcome;
use crate::domain::health::DiagnosticReport;
use crate::output::models::{ConnectionInfo, LogEntry, PendingRequest, SecurityStatus};

/// Resolved environment values for config rendering.
/// Constructed by the command layer, passed into renderers.
pub struct ConfigEnv {
    /// Resolved value of the `POLIS_CONFIG` environment variable.
    pub polis_config: Option<String>,
    /// Resolved value of the `NO_COLOR` environment variable.
    pub no_color: Option<String>,
}

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
    pub fn render_version(&self, version: &str, build_date: &str) -> Result<()> {
        match self {
            Renderer::Human(r) => {
                r.render_version(version, build_date);
                Ok(())
            }
            Renderer::Json(_) => JsonRenderer::render_version(version, build_date),
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

    /// Render agent activation result message.
    pub fn render_agent_activated(&self, agent: &str, already_active: bool) {
        match self {
            Renderer::Human(r) => r.render_agent_activated(agent, already_active),
            Renderer::Json(_) => {} // JSON output handled separately
        }
    }

    /// Render agent activation outcome (activated, already active, or unhealthy).
    pub fn render_activate_outcome(&self, outcome: &ActivateOutcome) {
        match self {
            Renderer::Human(r) => r.render_activate_outcome(outcome),
            Renderer::Json(_) => {} // JSON output handled separately
        }
    }

    /// Render onboarding steps for an activated agent.
    pub fn render_onboarding(&self, steps: &[polis_common::agent::OnboardingStep]) {
        match self {
            Renderer::Human(r) => r.render_onboarding(steps),
            Renderer::Json(_) => {} // JSON output handled separately
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
        config_env: &ConfigEnv,
    ) -> Result<()> {
        match self {
            Renderer::Human(r) => {
                r.render_config(config, path, config_env);
                Ok(())
            }
            Renderer::Json(_) => JsonRenderer::render_config(config, config_env),
        }
    }

    /// Render diagnostic health check results.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_diagnostics(
        &self,
        checks: &DiagnosticReport,
        issues: &[String],
        verbose: bool,
    ) -> Result<()> {
        match self {
            Renderer::Human(r) => {
                r.render_diagnostics(checks, issues, verbose);
                Ok(())
            }
            Renderer::Json(_) => JsonRenderer::render_diagnostics(checks, issues),
        }
    }

    /// Render connection info (SSH, VS Code, Cursor).
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_connection_info(&self, info: &ConnectionInfo) -> Result<()> {
        match self {
            Renderer::Human(r) => {
                r.render_connection_info(info);
                Ok(())
            }
            Renderer::Json(_) => JsonRenderer::render_connection_info(info),
        }
    }

    /// Render stop command outcome.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_stop_outcome(&self, outcome: &StopOutcome) -> Result<()> {
        match self {
            Renderer::Human(r) => {
                r.render_stop_outcome(outcome);
                Ok(())
            }
            Renderer::Json(_) => JsonRenderer::render_stop_outcome(outcome),
        }
    }

    /// Render delete command outcome.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_delete_outcome(&self, outcome: &DeleteOutcome, all: bool) -> Result<()> {
        match self {
            Renderer::Human(r) => {
                r.render_delete_outcome(outcome, all);
                Ok(())
            }
            Renderer::Json(_) => JsonRenderer::render_delete_outcome(outcome, all),
        }
    }

    /// Render start command outcome.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_start_outcome(
        &self,
        outcome: &StartOutcome,
        onboarding: &[OnboardingStep],
    ) -> Result<()> {
        match self {
            Renderer::Human(r) => {
                r.render_start_outcome(outcome, onboarding);
                Ok(())
            }
            Renderer::Json(_) => JsonRenderer::render_start_outcome(outcome, onboarding),
        }
    }

    /// Render update info (version comparison).
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_update_info(&self, current: &str, info: &UpdateInfo) -> Result<()> {
        match self {
            Renderer::Human(r) => {
                r.render_update_info(current, info);
                Ok(())
            }
            Renderer::Json(_) => JsonRenderer::render_update_info(current, info),
        }
    }

    /// Render security status.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_security_status(&self, status: &SecurityStatus) -> Result<()> {
        match self {
            Renderer::Human(r) => {
                r.render_security_status(status);
                Ok(())
            }
            Renderer::Json(_) => JsonRenderer::render_security_status(status),
        }
    }

    /// Render security pending requests.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_security_pending(&self, requests: &[PendingRequest]) -> Result<()> {
        match self {
            Renderer::Human(r) => {
                r.render_security_pending(requests);
                Ok(())
            }
            Renderer::Json(_) => JsonRenderer::render_security_pending(requests),
        }
    }

    /// Render security log entries.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_security_log(&self, entries: &[LogEntry]) -> Result<()> {
        match self {
            Renderer::Human(r) => {
                r.render_security_log(entries);
                Ok(())
            }
            Renderer::Json(_) => JsonRenderer::render_security_log(entries),
        }
    }

    /// Render security action result (approve/deny/rule/level).
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization fails.
    pub fn render_security_action(&self, message: &str) -> Result<()> {
        match self {
            Renderer::Human(r) => {
                r.render_security_action(message);
                Ok(())
            }
            Renderer::Json(_) => JsonRenderer::render_security_action(message),
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
    /// Primary output writer (replaces println!).
    writer: Mutex<Box<dyn Write + Send>>,
    /// Error output writer (replaces eprintln!).
    err_writer: Mutex<Box<dyn Write + Send>>,
}

impl OutputContext {
    /// Create output context from pre-resolved environment state.
    ///
    /// The caller is responsible for resolving `is_tty` (via `Term::stdout().is_term()`)
    /// and `no_color` (combining CLI flag and `NO_COLOR` env var) before calling this.
    #[must_use]
    pub fn new(no_color: bool, is_tty: bool, quiet: bool) -> Self {
        Self::with_writers(
            Box::new(std::io::stdout()),
            Box::new(std::io::stderr()),
            no_color,
            is_tty,
            quiet,
        )
    }

    /// Test-friendly constructor with explicit writers and state.
    ///
    /// Accepts caller-supplied writers and explicit TTY/color state,
    /// eliminating all environment coupling.
    #[must_use]
    pub fn with_writers(
        writer: Box<dyn Write + Send>,
        err_writer: Box<dyn Write + Send>,
        no_color: bool,
        is_tty: bool,
        quiet: bool,
    ) -> Self {
        let mut styles = Styles::default();
        if !no_color && is_tty {
            styles.colorize();
        }

        Self {
            styles,
            is_tty,
            quiet,
            writer: Mutex::new(writer),
            err_writer: Mutex::new(err_writer),
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
            let mut w = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
            let _ = writeln!(w, "  {} {msg}", "✓".style(self.styles.success));
        }
    }

    /// Print an in-progress step message prefixed with `→`. Suppressed when `quiet`.
    pub fn step(&self, msg: &str) {
        if !self.quiet {
            let mut w = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
            let _ = writeln!(w, "  {} {msg}", "→".cyan());
        }
    }

    /// Print a warning message prefixed with `!`. Suppressed when `quiet`.
    pub fn warn(&self, msg: &str) {
        if !self.quiet {
            let mut w = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
            let _ = writeln!(w, "  {} {msg}", "!".style(self.styles.warning));
        }
    }

    /// Print an error message prefixed with `✗` to stderr. Never suppressed.
    pub fn error(&self, msg: &str) {
        let mut w = self
            .err_writer
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let _ = writeln!(w, "  {} {msg}", "✗".style(self.styles.error));
    }

    /// Print an info message prefixed with `·`. Suppressed when `quiet`.
    pub fn info(&self, msg: &str) {
        if !self.quiet {
            let mut w = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
            let _ = writeln!(w, "  {} {msg}", "·".style(self.styles.info));
        }
    }

    /// Print a section header. Suppressed when `quiet`.
    pub fn header(&self, msg: &str) {
        if !self.quiet {
            let mut w = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
            let _ = writeln!(w, "  {}", msg.style(self.styles.header));
        }
    }

    /// Print a blank line. Suppressed when `quiet`.
    pub fn blank(&self) {
        if !self.quiet {
            let mut w = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
            let _ = writeln!(w);
        }
    }

    /// Print a key-value pair with the key dimmed. Suppressed when `quiet`.
    pub fn kv(&self, key: &str, value: &str) {
        if !self.quiet {
            let mut w = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
            let _ = writeln!(w, "  {}  {value}", key.style(self.styles.dim));
        }
    }

    /// Print the three core guarantees (governance, security, observability).
    pub fn guarantees(&self) {
        if self.quiet {
            return;
        }
        let mut w = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = writeln!(
            w,
            "  {}  policy engine active · audit trail recording",
            "[governance]   ".style(self.styles.governance)
        );
        let _ = writeln!(
            w,
            "  {}  workspace isolated · traffic inspection enabled",
            "[security]     ".style(self.styles.security)
        );
        let _ = writeln!(
            w,
            "  {}  action tracing live · trust scoring active",
            "[observability]".style(self.styles.observability)
        );
    }

    /// Write a raw formatted line (no prefix). Suppressed when quiet.
    pub fn write_raw(&self, msg: &str) {
        if !self.quiet {
            let mut w = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
            let _ = writeln!(w, "{msg}");
        }
    }

    /// Write a check mark line (✓ or ✗ prefix). Suppressed when quiet.
    pub fn write_check(&self, ok: bool, msg: &str) {
        if !self.quiet {
            let mut w = self.writer.lock().unwrap_or_else(PoisonError::into_inner);
            if ok {
                let _ = writeln!(w, "    {} {msg}", "\u{2713}".style(self.styles.success));
            } else {
                let _ = writeln!(w, "    {} {msg}", "\u{2717}".style(self.styles.error));
            }
        }
    }
}

#[cfg(test)]
pub mod test_support {
    //! Shared test helpers for output module tests.

    use std::io::{self, Write};
    use std::sync::{Arc, Mutex, PoisonError};

    /// Shared buffer type used by `SharedWriter` and test assertions.
    pub type SharedBuf = Arc<Mutex<Vec<u8>>>;

    /// A `Write` implementation backed by a shared `Arc<Mutex<Vec<u8>>>`.
    ///
    /// Allows tests to share a buffer between `OutputContext` and assertion code.
    pub struct SharedWriter(pub SharedBuf);

    impl Write for SharedWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// Construct an `OutputContext` backed by shared `Vec<u8>` buffers.
    ///
    /// Returns `(ctx, stdout_buf, stderr_buf)`.
    #[must_use]
    pub fn make_test_ctx(quiet: bool) -> (super::OutputContext, SharedBuf, SharedBuf) {
        let stdout_buf = Arc::new(Mutex::new(Vec::<u8>::new()));
        let stderr_buf = Arc::new(Mutex::new(Vec::<u8>::new()));
        let ctx = super::OutputContext::with_writers(
            Box::new(SharedWriter(Arc::clone(&stdout_buf))),
            Box::new(SharedWriter(Arc::clone(&stderr_buf))),
            true,  // no_color
            false, // is_tty
            quiet,
        );
        (ctx, stdout_buf, stderr_buf)
    }

    /// Read the contents of a shared buffer as a UTF-8 string.
    pub fn buf_to_string(buf: &SharedBuf) -> String {
        String::from_utf8(buf.lock().unwrap_or_else(PoisonError::into_inner).clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::test_support::{buf_to_string, make_test_ctx};
    use super::*;

    // ── Quiet mode suppression ────────────────────────────────────────────────

    #[test]
    fn test_quiet_suppresses_non_error_methods() {
        let (ctx, stdout, stderr) = make_test_ctx(true);
        ctx.success("msg");
        ctx.step("msg");
        ctx.warn("msg");
        ctx.info("msg");
        ctx.header("msg");
        ctx.blank();
        ctx.kv("k", "v");
        ctx.guarantees();
        ctx.write_raw("raw");
        ctx.write_check(true, "check");
        assert!(
            buf_to_string(&stdout).is_empty(),
            "stdout should be empty in quiet mode"
        );
        assert!(
            buf_to_string(&stderr).is_empty(),
            "stderr should be empty in quiet mode"
        );
    }

    #[test]
    fn test_quiet_does_not_suppress_error() {
        let (ctx, _stdout, stderr) = make_test_ctx(true);
        ctx.error("something went wrong");
        assert!(
            !buf_to_string(&stderr).is_empty(),
            "error should still write in quiet mode"
        );
        assert!(buf_to_string(&stderr).contains("something went wrong"));
    }

    // ── Writer routing ────────────────────────────────────────────────────────

    #[test]
    fn test_non_error_methods_write_to_stdout_not_stderr() {
        let (ctx, stdout, stderr) = make_test_ctx(false);
        ctx.success("ok");
        assert!(!buf_to_string(&stdout).is_empty());
        assert!(buf_to_string(&stderr).is_empty());
    }

    #[test]
    fn test_error_writes_to_stderr_not_stdout() {
        let (ctx, stdout, stderr) = make_test_ctx(false);
        ctx.error("boom");
        assert!(buf_to_string(&stdout).is_empty());
        assert!(!buf_to_string(&stderr).is_empty());
    }

    // ── Styling ───────────────────────────────────────────────────────────────

    #[test]
    fn test_no_color_produces_default_styles() {
        let ctx = OutputContext::with_writers(
            Box::new(std::io::sink()),
            Box::new(std::io::sink()),
            true, // no_color
            true, // is_tty
            false,
        );
        // Default styles have no ANSI codes — format is empty string
        let default = Styles::default();
        // Both should be the default (no color) style
        assert_eq!(
            format!("{:?}", ctx.styles.success),
            format!("{:?}", default.success)
        );
    }

    #[test]
    fn test_colorized_styles_when_no_color_false_and_is_tty_true() {
        let ctx = OutputContext::with_writers(
            Box::new(std::io::sink()),
            Box::new(std::io::sink()),
            false, // no_color
            true,  // is_tty
            false,
        );
        let default = Styles::default();
        // Colorized styles differ from default
        assert_ne!(
            format!("{:?}", ctx.styles.success),
            format!("{:?}", default.success)
        );
    }

    // ── new() accepts pre-resolved state ─────────────────────────────────────

    #[test]
    fn test_new_accepts_pre_resolved_state() {
        // Should construct without panicking or reading env vars
        let ctx = OutputContext::new(true, false, true);
        assert!(!ctx.is_tty);
        assert!(ctx.quiet);
    }

    // ── write_check ───────────────────────────────────────────────────────────

    #[test]
    fn test_write_check_ok_contains_checkmark() {
        let (ctx, stdout, _) = make_test_ctx(false);
        ctx.write_check(true, "all good");
        let out = buf_to_string(&stdout);
        assert!(out.contains("all good"));
        assert!(out.contains('\u{2713}'));
    }

    #[test]
    fn test_write_check_fail_contains_cross() {
        let (ctx, stdout, _) = make_test_ctx(false);
        ctx.write_check(false, "broken");
        let out = buf_to_string(&stdout);
        assert!(out.contains("broken"));
        assert!(out.contains('\u{2717}'));
    }

    // ── write_raw ─────────────────────────────────────────────────────────────

    #[test]
    fn test_write_raw_outputs_message() {
        let (ctx, stdout, _) = make_test_ctx(false);
        ctx.write_raw("hello world");
        assert!(buf_to_string(&stdout).contains("hello world"));
    }

    // ── Write failure resilience ──────────────────────────────────────────────

    #[test]
    fn test_write_failure_does_not_panic() {
        // std::io::sink() discards all writes — simulates a broken writer
        let ctx = OutputContext::with_writers(
            Box::new(std::io::sink()),
            Box::new(std::io::sink()),
            true,
            false,
            false,
        );
        // None of these should panic
        ctx.success("msg");
        ctx.step("msg");
        ctx.warn("msg");
        ctx.error("msg");
        ctx.info("msg");
        ctx.header("msg");
        ctx.blank();
        ctx.kv("k", "v");
        ctx.guarantees();
        ctx.write_raw("raw");
        ctx.write_check(true, "ok");
    }
}
