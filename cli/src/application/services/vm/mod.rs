//! Application services for VM lifecycle, provisioning, and integrity.
//!
//! These modules decompose the original `workspace/vm.rs` into focused
//! application services. Each module imports only from `crate::domain` and
//! `crate::application::ports`.

pub mod health;
pub mod integrity;
pub mod lifecycle;
pub mod provision;
pub mod services;

#[cfg(test)]
pub(crate) mod test_support;

pub(crate) fn inception_line(level: &str, msg: &str) -> String {
    use owo_colors::{OwoColorize, Stream::Stdout, Style};
    let tag_style = match level {
        "L0" => Style::new().truecolor(107, 33, 168),
        "L1" => Style::new().truecolor(93, 37, 163),
        "L2" => Style::new().truecolor(64, 47, 153),
        _ => Style::new().truecolor(46, 53, 147),
    };
    format!(
        "{}  {}",
        "[inception]".if_supports_color(Stdout, |t| t.style(tag_style)),
        msg
    )
}
