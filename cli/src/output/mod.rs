//! Output formatting module

#![allow(dead_code)] // Helper methods not yet adopted by all commands

pub mod json;
pub mod progress;
pub mod styles;

use console::Term;
use owo_colors::OwoColorize as _;
pub use styles::Styles;

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
}

#[cfg(test)]
mod tests;

