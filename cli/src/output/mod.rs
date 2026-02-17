//! Output formatting module

#![allow(dead_code)] // Module not yet integrated into CLI commands

pub mod json;
pub mod progress;
pub mod styles;

use console::Term;
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
}

#[cfg(test)]
mod tests;
