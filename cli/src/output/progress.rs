//! Progress indicators using indicatif

#![allow(clippy::expect_used)] // Templates are compile-time constants

use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};

/// Create a spinner for indeterminate progress.
///
/// # Panics
///
/// Panics if the spinner template string is invalid (it is a compile-time constant and will not panic).
#[must_use]
pub fn spinner(msg: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&[
                "⠁", "⠂", "⠄", "⡀", "⡈", "⡐", "⡠", "⣀", "⣁", "⣂", "⣄", "⣌", "⣔", "⣤", "⣥", "⣦",
                "⣮", "⣶", "⣷", "⣿", "⡿", "⠿", "⢟", "⠟", "⡛", "⠛", "⠫", "⢋", "⠋", "⠍", "⡉", "⠉",
                "⠑", "⠡", "⢁",
            ])
            .template("{spinner:.cyan} {msg}")
            .expect("valid template"),
    );
    pb.set_message(msg.to_string());
    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}

/// Create a progress bar for determinate progress.
///
/// # Panics
///
/// Panics if the progress bar template string is invalid (it is a compile-time constant and will not panic).
#[must_use]
pub fn bar(len: u64, msg: &str) -> ProgressBar {
    let pb = ProgressBar::new(len);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  {msg}\n    {bar:40.cyan/dim} {percent}%  {bytes}/{total_bytes}")
            .expect("valid template")
            .progress_chars("━━─"),
    );
    pb.set_message(msg.to_string());
    pb
}

/// Finish a spinner with a checkmark on the left.
pub fn finish_ok(pb: &ProgressBar, msg: &str) {
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{prefix} {msg}")
            .expect("valid template"),
    );
    pb.set_prefix("✓");
    pb.finish_with_message(msg.to_string());
}

/// Finish a progress bar with a success message.
pub fn finish_success(pb: &ProgressBar, msg: &str) {
    pb.finish_with_message(format!("✓ {msg}"));
}

/// Finish a progress bar with an error message.
pub fn finish_error(pb: &ProgressBar, msg: &str) {
    pb.finish_with_message(format!("✗ {msg}"));
}
