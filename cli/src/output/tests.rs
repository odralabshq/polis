//! Unit tests for output styling module
//!
//! Tests for spec 03-output-styling.md

#[cfg(test)]
mod tests {
    use crate::output::{progress, OutputContext, Styles};
    use owo_colors::OwoColorize;

    // --- Styles tests ---

    #[test]
    fn test_styles_default_has_no_colors() {
        let styles = Styles::default();
        // Default styles should not apply any formatting
        let text = "test";
        let styled = text.style(styles.success);
        // When no color is applied, styled output equals input
        assert_eq!(format!("{styled}"), text);
    }

    #[test]
    fn test_styles_colorize_applies_colors() {
        let mut styles = Styles::default();
        styles.colorize();
        // After colorize, styles should have color codes
        let text = "test";
        let styled = format!("{}", text.style(styles.success));
        // Green text should contain ANSI escape codes
        assert!(styled.contains("\x1b["), "should contain ANSI escape code");
        assert!(styled.contains("32"), "should contain green color code");
    }

    #[test]
    fn test_styles_colorize_sets_all_styles() {
        let mut styles = Styles::default();
        styles.colorize();

        // Verify each style produces different output
        let text = "x";
        let success = format!("{}", text.style(styles.success));
        let warning = format!("{}", text.style(styles.warning));
        let error = format!("{}", text.style(styles.error));
        let info = format!("{}", text.style(styles.info));

        assert_ne!(success, warning);
        assert_ne!(warning, error);
        assert_ne!(error, info);
    }

    // --- OutputContext tests ---

    #[test]
    fn test_output_context_no_color_flag_disables_colors() {
        let ctx = OutputContext::new(true, false);
        let text = "test";
        let styled = format!("{}", text.style(ctx.styles.success));
        // With no_color=true, should not contain escape codes
        assert!(!styled.contains("\x1b["), "should not contain ANSI codes when no_color=true");
    }

    #[test]
    fn test_output_context_quiet_flag_sets_quiet() {
        let ctx = OutputContext::new(false, true);
        assert!(ctx.quiet);
    }

    #[test]
    fn test_output_context_show_progress_false_when_quiet() {
        let ctx = OutputContext::new(false, true);
        // When quiet, show_progress should be false regardless of TTY
        assert!(!ctx.show_progress() || !ctx.quiet);
    }

    #[test]
    fn test_output_context_show_progress_false_when_not_tty() {
        // In test environment, stdout is typically not a TTY
        let ctx = OutputContext::new(false, false);
        // show_progress depends on is_tty
        if !ctx.is_tty {
            assert!(!ctx.show_progress());
        }
    }

    // --- Progress helpers tests ---

    #[test]
    fn test_spinner_creates_progress_bar() {
        let pb = progress::spinner("Loading...");
        // Spinner should be created without panic
        pb.finish();
    }

    #[test]
    fn test_bar_creates_progress_bar() {
        let pb = progress::bar(100, "Downloading");
        // Bar should be created without panic
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

    // --- NO_COLOR environment tests ---

    #[test]
    fn test_no_color_env_disables_colors() {
        // This test checks the logic - when NO_COLOR is set, colors should be disabled
        // We test the OutputContext behavior with no_color=true flag instead of env var
        // since env var manipulation is unsafe in Rust 2024
        let ctx = OutputContext::new(true, false); // no_color=true simulates NO_COLOR env
        let text = "test";
        let styled = format!("{}", text.style(ctx.styles.success));
        // Should not have ANSI codes
        assert!(
            !styled.contains("\x1b["),
            "NO_COLOR should disable colors"
        );
    }
}

// ============================================================================
// Property-Based Tests
// ============================================================================

mod proptests {
    use crate::output::{progress, OutputContext, Styles};
    use owo_colors::OwoColorize;
    use proptest::prelude::*;

    proptest! {
        /// OutputContext with no_color=true never produces ANSI codes
        #[test]
        fn prop_no_color_never_produces_ansi(text in "[a-zA-Z0-9 ]{1,50}") {
            let ctx = OutputContext::new(true, false);
            let styled = format!("{}", text.style(ctx.styles.success));
            prop_assert!(!styled.contains("\x1b["), "no_color should disable ANSI codes");
        }

        /// Styles::colorize produces different styles for each field
        #[test]
        fn prop_colorize_produces_distinct_styles(_seed in 0u32..100) {
            let mut styles = Styles::default();
            styles.colorize();
            
            let text = "x";
            let outputs: Vec<String> = vec![
                format!("{}", text.style(styles.success)),
                format!("{}", text.style(styles.warning)),
                format!("{}", text.style(styles.error)),
                format!("{}", text.style(styles.info)),
            ];
            
            // All colored outputs should be different
            for i in 0..outputs.len() {
                for j in (i+1)..outputs.len() {
                    prop_assert_ne!(&outputs[i], &outputs[j], "styles should be distinct");
                }
            }
        }

        /// show_progress is false when quiet is true
        #[test]
        fn prop_quiet_disables_progress(no_color in proptest::bool::ANY) {
            let ctx = OutputContext::new(no_color, true);
            prop_assert!(!ctx.show_progress(), "quiet should disable progress");
        }

        /// Progress bar length matches input
        #[test]
        fn prop_bar_length_matches_input(len in 1u64..10000) {
            let pb = progress::bar(len, "test");
            prop_assert_eq!(pb.length(), Some(len));
            pb.finish();
        }
    }
}
