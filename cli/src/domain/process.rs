//! Process utilities — pure functions for exit code handling.
//!
//! This module has zero imports from `crate::infra`, `crate::commands`,
//! `crate::application`, `tokio`, `std::fs`, or `std::net`.

use std::process::{ExitCode, ExitStatus};

/// Convert a process `ExitStatus` to an `ExitCode`.
///
/// - If the process exited normally, returns the exit code (clamped to 0-255).
/// - If the process was terminated by a signal (Unix) or has no code, returns 255.
///
/// This function centralizes the exit code conversion logic used by `exec`,
/// `connect`, and `internal` commands.
#[must_use]
pub fn exit_code_from_status(status: ExitStatus) -> ExitCode {
    let code = status.code().unwrap_or(255);
    ExitCode::from(u8::try_from(code).unwrap_or(255))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::os::unix::process::ExitStatusExt;

    // Helper to extract the numeric value from ExitCode debug output
    fn extract_exit_code_value(code: ExitCode) -> u8 {
        // ExitCode debug format is "ExitCode(unix_exit_status(N))" where N is the raw value
        // For normal exits, raw = exit_code << 8, so we need to extract and shift
        // For our function output, we create ExitCode::from(u8), so the value is direct
        let debug = format!("{code:?}");
        // Parse the inner number from "ExitCode(unix_exit_status(N))"
        let start = debug.find('(').map_or(0, |i| i + 1);
        let inner = &debug[start..debug.len() - 1];
        let start2 = inner.find('(').map_or(0, |i| i + 1);
        let num_str = &inner[start2..inner.len() - 1];
        num_str.parse::<u8>().unwrap_or(0)
    }

    #[test]
    fn exit_code_zero_returns_zero() {
        // Exit code 0 is stored as 0 in raw status on Unix (0 << 8 = 0)
        let status = ExitStatus::from_raw(0);
        assert_eq!(status.code(), Some(0));
        let code = exit_code_from_status(status);
        assert_eq!(extract_exit_code_value(code), 0);
    }

    #[test]
    fn exit_code_one_returns_one() {
        // Exit code 1 is stored as 1 << 8 = 256 in raw status on Unix
        let status = ExitStatus::from_raw(1 << 8);
        assert_eq!(status.code(), Some(1));
        let code = exit_code_from_status(status);
        assert_eq!(extract_exit_code_value(code), 1);
    }

    #[test]
    fn exit_code_255_returns_255() {
        // Exit code 255 is stored as 255 << 8 in raw status on Unix
        let status = ExitStatus::from_raw(255 << 8);
        assert_eq!(status.code(), Some(255));
        let code = exit_code_from_status(status);
        assert_eq!(extract_exit_code_value(code), 255);
    }

    #[test]
    fn signal_termination_returns_255() {
        // Signal 9 (SIGKILL) - raw value is just the signal number (no exit code)
        let status = ExitStatus::from_raw(9);
        assert_eq!(status.code(), None); // Signal termination has no exit code
        let code = exit_code_from_status(status);
        assert_eq!(extract_exit_code_value(code), 255);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// **Validates: Requirements 7.2, 7.3**
        ///
        /// Property 2: Exit Code Conversion
        ///
        /// For any `ExitStatus` with a code in range 0-255, `exit_code_from_status`
        /// returns an `ExitCode` with the same numeric value. For any `ExitStatus`
        /// with a code outside 0-255 or with no code (signal termination), the
        /// function returns `ExitCode::from(255)`.
        #[test]
        fn prop_exit_code_conversion_in_range(exit_code in 0u8..=255u8) {
            // Create an ExitStatus with a normal exit code (code << 8 on Unix)
            let raw = i32::from(exit_code) << 8;
            let status = ExitStatus::from_raw(raw);

            // Verify the status has the expected code
            prop_assert_eq!(status.code(), Some(i32::from(exit_code)));

            // Convert and verify
            let result = exit_code_from_status(status);
            prop_assert_eq!(extract_exit_code_value(result), exit_code);
        }

        /// **Validates: Requirements 7.2, 7.3**
        ///
        /// Property 2: Exit Code Conversion (signal termination case)
        ///
        /// For signal termination (no exit code), the function returns 255.
        #[test]
        fn prop_exit_code_signal_termination_returns_255(signal in 1u8..=31u8) {
            // Create an ExitStatus for signal termination (raw value is just the signal number)
            let status = ExitStatus::from_raw(i32::from(signal));

            // Signal termination has no exit code
            prop_assert!(status.code().is_none());

            // Convert and verify it returns 255
            let result = exit_code_from_status(status);
            prop_assert_eq!(extract_exit_code_value(result), 255);
        }
    }
}
