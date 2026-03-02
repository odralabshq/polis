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

    fn start_waiting(&self, msg: &str) {
        if self.ctx.quiet || !self.ctx.is_tty {
            return;
        }
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&["⠁", "⠂", "⠄", "⡀", "⢀", "⠠", "⠐", "⠈"])
                .template("  {spinner:.cyan} {msg}")
                .expect("valid template"),
        );
        pb.set_message(format!("{msg} 0:00"));
        pb.enable_steady_tick(std::time::Duration::from_millis(100));
        // Spawn a task to update the elapsed time every second in mm:ss format.
        let pb_clone = pb.downgrade();
        let msg_owned = msg.to_owned();
        let start = std::time::Instant::now();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                let Some(pb) = pb_clone.upgrade() else { break };
                if pb.is_finished() { break }
                let secs = start.elapsed().as_secs();
                pb.set_message(format!("{msg_owned} {}:{:02}", secs / 60, secs % 60));
            }
        });
        self.spinner.set(Some(pb));
    }

    fn stop_waiting(&self, success: bool, msg: &str) {
        if let Some(pb) = self.spinner.take() {
            if success {
                pb.set_style(
                    ProgressStyle::default_spinner()
                        .template("  {prefix} {msg}")
                        .expect("valid template"),
                );
                pb.set_prefix("✓");
                pb.finish_with_message(msg.to_owned());
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
