//! Output styles using owo-colors stylesheet pattern

use owo_colors::Style;

/// Centralized stylesheet for CLI output colors.
#[derive(Default, Clone)]
pub struct Styles {
    /// Success messages (green)
    pub success: Style,
    /// Warning messages (yellow)
    pub warning: Style,
    /// Error messages (red)
    pub error: Style,
    /// Info messages (blue)
    pub info: Style,
    /// Dimmed/secondary text
    pub dim: Style,
    /// Bold text
    pub bold: Style,
    /// Headers/section titles
    pub header: Style,
}

impl Styles {
    /// Apply colors to the stylesheet.
    pub fn colorize(&mut self) {
        self.success = Style::new().green();
        self.warning = Style::new().yellow();
        self.error = Style::new().red();
        self.info = Style::new().blue();
        self.dim = Style::new().dimmed();
        self.bold = Style::new().bold();
        self.header = Style::new().bold().cyan();
    }
}
