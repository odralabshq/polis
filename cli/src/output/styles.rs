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
    /// Governance label (dark blue)
    pub governance: Style,
    /// Security label (medium blue)
    pub security: Style,
    /// Observability label (light blue)
    pub observability: Style,
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
        self.governance = Style::new().truecolor(37, 56, 144);
        self.security = Style::new().truecolor(26, 107, 160);
        self.observability = Style::new().truecolor(26, 151, 179);
    }
}
