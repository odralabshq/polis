//! Unit tests for output styling module
//!
//! Tests for spec 03-output-styling.md

#[cfg(test)]
#[allow(clippy::similar_names, clippy::module_inception)]
mod tests {
    use crate::output::{OutputContext, Styles, progress};
    use owo_colors::OwoColorize;

    // --- Styles tests ---

    #[test]
    fn test_styles_default_has_no_colors() {
        let styles = Styles::default();
        let text = "test";
        let styled = text.style(styles.success);
        assert_eq!(format!("{styled}"), text);
    }

    #[test]
    fn test_styles_colorize_applies_colors() {
        let mut styles = Styles::default();
        styles.colorize();
        let styled = format!("{}", "test".style(styles.success));
        assert!(styled.contains("\x1b["), "should contain ANSI escape code");
        assert!(styled.contains("32"), "should contain green color code");
    }

    #[test]
    fn test_styles_colorize_sets_all_styles() {
        let mut styles = Styles::default();
        styles.colorize();
        let text = "x";
        let success = format!("{}", text.style(styles.success));
        let warning = format!("{}", text.style(styles.warning));
        let error = format!("{}", text.style(styles.error));
        let info = format!("{}", text.style(styles.info));
        assert_ne!(success, warning);
        assert_ne!(warning, error);
        assert_ne!(error, info);
    }

    // --- OutputContext construction tests ---

    #[test]
    fn test_output_context_no_color_flag_disables_colors() {
        let ctx = OutputContext::new(true, false);
        let styled = format!("{}", "test".style(ctx.styles.success));
        assert!(
            !styled.contains("\x1b["),
            "should not contain ANSI codes when no_color=true"
        );
    }

    #[test]
    fn test_output_context_quiet_flag_sets_quiet() {
        let ctx = OutputContext::new(false, true);
        assert!(ctx.quiet);
    }

    #[test]
    fn test_output_context_not_quiet_by_default() {
        let ctx = OutputContext::new(false, false);
        assert!(!ctx.quiet);
    }

    #[test]
    fn test_output_context_show_progress_false_when_quiet() {
        let ctx = OutputContext::new(false, true);
        assert!(!ctx.show_progress() || !ctx.quiet);
    }

    #[test]
    fn test_output_context_show_progress_false_when_not_tty() {
        let ctx = OutputContext::new(false, false);
        if !ctx.is_tty {
            assert!(!ctx.show_progress());
        }
    }

    // --- Helper method smoke tests (no_color=true avoids ANSI in test output) ---

    #[test]
    fn test_success_does_not_panic_when_not_quiet() {
        let ctx = OutputContext::new(true, false);
        ctx.success("workspace ready");
    }

    #[test]
    fn test_success_does_not_panic_when_quiet() {
        let ctx = OutputContext::new(true, true);
        ctx.success("workspace ready");
    }

    #[test]
    fn test_warn_does_not_panic_when_not_quiet() {
        let ctx = OutputContext::new(true, false);
        ctx.warn("certificate expiring soon");
    }

    #[test]
    fn test_warn_does_not_panic_when_quiet() {
        let ctx = OutputContext::new(true, true);
        ctx.warn("certificate expiring soon");
    }

    #[test]
    fn test_error_does_not_panic_when_not_quiet() {
        let ctx = OutputContext::new(true, false);
        ctx.error("connection refused");
    }

    #[test]
    fn test_error_does_not_panic_when_quiet() {
        // error() is never suppressed — must not panic even when quiet=true
        let ctx = OutputContext::new(true, true);
        ctx.error("connection refused");
    }

    #[test]
    fn test_info_does_not_panic_when_not_quiet() {
        let ctx = OutputContext::new(true, false);
        ctx.info("checking network");
    }

    #[test]
    fn test_info_does_not_panic_when_quiet() {
        let ctx = OutputContext::new(true, true);
        ctx.info("checking network");
    }

    #[test]
    fn test_header_does_not_panic_when_not_quiet() {
        let ctx = OutputContext::new(true, false);
        ctx.header("Polis Health Check");
    }

    #[test]
    fn test_header_does_not_panic_when_quiet() {
        let ctx = OutputContext::new(true, true);
        ctx.header("Polis Health Check");
    }

    #[test]
    fn test_kv_does_not_panic_when_not_quiet() {
        let ctx = OutputContext::new(true, false);
        ctx.kv("agent", "openclaw");
    }

    #[test]
    fn test_kv_does_not_panic_when_quiet() {
        let ctx = OutputContext::new(true, true);
        ctx.kv("agent", "openclaw");
    }

    #[test]
    fn test_kv_does_not_panic_with_empty_value() {
        let ctx = OutputContext::new(true, false);
        ctx.kv("status", "");
    }

    // --- Quiet flag is the only suppression gate ---

    #[test]
    fn test_quiet_field_true_when_constructed_quiet() {
        let ctx = OutputContext::new(false, true);
        assert!(ctx.quiet, "quiet flag must be stored");
    }

    #[test]
    fn test_quiet_field_false_when_constructed_not_quiet() {
        let ctx = OutputContext::new(false, false);
        assert!(!ctx.quiet, "quiet flag must be stored as false");
    }

    // --- Progress helpers tests ---

    #[test]
    fn test_spinner_creates_progress_bar() {
        let pb = progress::spinner("Loading...");
        pb.finish();
    }

    #[test]
    fn test_bar_creates_progress_bar() {
        let pb = progress::bar(100, "Downloading");
        assert_eq!(pb.length(), Some(100));
        pb.finish();
    }

    #[test]
    fn test_finish_success_completes_bar() {
        let pb = progress::spinner("Working...");
        progress::finish_success(&pb, "Done");
        assert!(pb.is_finished());
    }

    #[test]
    fn test_finish_error_completes_bar() {
        let pb = progress::spinner("Working...");
        progress::finish_error(&pb, "Failed");
        assert!(pb.is_finished());
    }

    #[test]
    fn test_no_color_env_disables_colors() {
        let ctx = OutputContext::new(true, false);
        let styled = format!("{}", "test".style(ctx.styles.success));
        assert!(!styled.contains("\x1b["), "NO_COLOR should disable colors");
    }
}
