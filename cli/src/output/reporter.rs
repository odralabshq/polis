//! `TerminalReporter` — Presentation-layer implementation of `ProgressReporter`.
//!
//! Wraps `&OutputContext` and implements the `application::ports::ProgressReporter`
//! trait so application services can emit progress events without depending on
//! any presentation type directly.

use std::cell::Cell;

use indicatif::{ProgressBar, ProgressStyle};
use owo_colors::OwoColorize as _;

use crate::application::ports::ProgressReporter;
use crate::output::OutputContext;

/// Terminal progress reporter that wraps an `OutputContext`.
///
/// - `step()` prints `"  → {message}"` (suppressed when `ctx.quiet`)
/// - `success()` prints `"  ✓ {message}"` (suppressed when `ctx.quiet`)
/// - `warn()` prints `"  ! {message}"` (suppressed when `ctx.quiet`)
/// - `start_waiting()` starts a live elapsed-time spinner on TTY
/// - `stop_waiting(success)` finishes the spinner with ✓ or ✗
pub struct TerminalReporter<'a> {
    ctx: &'a OutputContext,
    spinner: Cell<Option<ProgressBar>>,
}

impl<'a> TerminalReporter<'a> {
    /// Create a new `TerminalReporter` wrapping the given output context.
    #[must_use]
    pub fn new(ctx: &'a OutputContext) -> Self {
        Self {
            ctx,
            spinner: Cell::new(None),
        }
    }
}

impl ProgressReporter for TerminalReporter<'_> {
    fn step(&self, message: &str) {
        if !self.ctx.quiet {
            println!("  {} {message}", "→".cyan());
        }
    }

    fn success(&self, message: &str) {
        if !self.ctx.quiet {
            println!("  {} {message}", "✓".green());
        }
    }

    fn warn(&self, message: &str) {
        if !self.ctx.quiet {
            println!("  {} {message}", "!".yellow());
        }
    }

    fn start_waiting(&self) {
        if self.ctx.quiet || !self.ctx.is_tty {
            return;
        }
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&["⠁", "⠂", "⠄", "⡀", "⢀", "⠠", "⠐", "⠈"])
                .template("  {spinner:.cyan} starting up... {elapsed}")
                .expect("valid template"),
        );
        pb.enable_steady_tick(std::time::Duration::from_millis(100));
        self.spinner.set(Some(pb));
    }

    fn stop_waiting(&self, success: bool) {
        if let Some(pb) = self.spinner.take() {
            if success {
                pb.set_style(
                    ProgressStyle::default_spinner()
                        .template("  {prefix} {msg}")
                        .expect("valid template"),
                );
                pb.set_prefix("✓");
                pb.finish_with_message("workspace ready");
            } else {
                pb.abandon();
            }
        }
    }

    fn is_spinning(&self) -> bool {
        // SAFETY: we only peek, not take
        let pb = self.spinner.take();
        let result = pb.is_some();
        self.spinner.set(pb);
        result
    }
}
