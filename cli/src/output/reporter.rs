//! `TerminalReporter` — Presentation-layer implementation of `ProgressReporter`.
//!
//! Wraps `&OutputContext` and implements the `application::ports::ProgressReporter`
//! trait so application services can emit progress events without depending on
//! any presentation type directly.

use owo_colors::OwoColorize as _;

use crate::application::ports::ProgressReporter;
use crate::output::OutputContext;

/// Terminal progress reporter that wraps an `OutputContext`.
///
/// - `step()` prints `"  → {message}"` (suppressed when `ctx.quiet`)
/// - `success()` prints `"  ✓ {message}"` (suppressed when `ctx.quiet`)
/// - `warn()` prints `"  ⚠ {message}"` (suppressed when `ctx.quiet`)
pub struct TerminalReporter<'a> {
    ctx: &'a OutputContext,
}

impl<'a> TerminalReporter<'a> {
    /// Create a new `TerminalReporter` wrapping the given output context.
    #[must_use]
    pub fn new(ctx: &'a OutputContext) -> Self {
        Self { ctx }
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
}
