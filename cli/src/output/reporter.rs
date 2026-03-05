//! `TerminalReporter` — Presentation-layer implementation of `ProgressReporter`.
//!
//! Wraps `&OutputContext` and implements the `application::ports::ProgressReporter`
//! trait so application services can emit progress events without depending on
//! any presentation type directly.

use std::sync::Mutex;
use std::time::Instant;

use indicatif::{ProgressBar, ProgressStyle};
use owo_colors::OwoColorize as _;

use crate::application::ports::ProgressReporter;
use crate::output::OutputContext;

/// Terminal progress reporter that wraps an `OutputContext`.
///
/// - `step()` prints `"  → {message}"` (suppressed when `ctx.quiet`)
/// - `success()` prints `"  ✓ {message}"` (suppressed when `ctx.quiet`)
/// - `warn()` prints `"  ! {message}"` (suppressed when `ctx.quiet`)
/// - `begin_stage()` starts a timed spinner on TTY, auto-completing any prior stage
/// - `complete_stage()` finishes the spinner with ✓ and elapsed time
/// - `fail_stage()` finishes the spinner with ✗ and elapsed time
pub struct TerminalReporter<'a> {
    ctx: &'a OutputContext,
    stage: Mutex<Option<ActiveStage>>,
}

/// A currently-running timed stage.
struct ActiveStage {
    message: String,
    start: Instant,
    /// `None` when not on a TTY (non-TTY still tracks timing for the log line).
    spinner: Option<ProgressBar>,
}

impl<'a> TerminalReporter<'a> {
    /// Create a new `TerminalReporter` wrapping the given output context.
    #[must_use]
    pub fn new(ctx: &'a OutputContext) -> Self {
        Self {
            ctx,
            stage: Mutex::new(None),
        }
    }

    /// Finish the active stage, printing a final status line.
    fn finish_active_stage(&self, success: bool) {
        #[allow(clippy::expect_used)] // Mutex poison is unrecoverable — panic is correct.
        let Some(stage) = self.stage.lock().expect("stage lock").take() else {
            return;
        };
        let elapsed = stage.start.elapsed().as_secs();
        let time = format!("{}:{:02}", elapsed / 60, elapsed % 60);

        if let Some(pb) = &stage.spinner {
            pb.finish_and_clear();
        }

        if !self.ctx.quiet {
            if success {
                println!("  {} {} {time}", "✓".green(), stage.message);
            } else {
                println!("  {} {} {time}", "✗".red(), stage.message);
            }
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

    fn begin_stage(&self, message: &str) {
        if self.ctx.quiet {
            return;
        }

        // Auto-complete any active stage with success.
        self.finish_active_stage(true);

        let spinner = if self.ctx.is_tty {
            let pb = ProgressBar::new_spinner();
            #[allow(clippy::expect_used)] // Template is a compile-time constant.
            pb.set_style(
                ProgressStyle::default_spinner()
                    .tick_strings(&["⠁", "⠂", "⠄", "⡀", "⢀", "⠠", "⠐", "⠈"])
                    .template("  {spinner:.cyan} {msg}")
                    .expect("valid template"),
            );
            pb.set_message(format!("{message} 0:00"));
            pb.enable_steady_tick(std::time::Duration::from_millis(100));

            // Spawn a task to update elapsed time every second.
            let weak = pb.downgrade();
            let msg_owned = message.to_owned();
            let start = Instant::now();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    let Some(pb) = weak.upgrade() else { break };
                    if pb.is_finished() {
                        break;
                    }
                    let secs = start.elapsed().as_secs();
                    pb.set_message(format!("{msg_owned} {}:{:02}", secs / 60, secs % 60));
                }
            });
            Some(pb)
        } else {
            // Non-TTY: print a plain step line as a breadcrumb.
            println!("  {} {message}", "→".cyan());
            None
        };

        #[allow(clippy::expect_used)] // Mutex poison is unrecoverable — panic is correct.
        let mut guard = self.stage.lock().expect("stage lock");
        *guard = Some(ActiveStage {
            message: message.to_owned(),
            start: Instant::now(),
            spinner,
        });
    }

    fn complete_stage(&self) {
        self.finish_active_stage(true);
    }

    fn fail_stage(&self) {
        self.finish_active_stage(false);
    }
}
