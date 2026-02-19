//! Integration tests for `polis init` (issue 01 — init command skeleton).
//!
//! Tests exercise the public CLI surface via `assert_cmd`. Each test is
//! independent: filesystem side-effects are isolated with `tempfile::TempDir`
//! and `HOME` is overridden per-process via the `env()` builder.

#![allow(clippy::expect_used)]

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn polis() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("polis"))
}

// ── help / registration ──────────────────────────────────────────────────────

#[test]
fn test_init_help_shows_in_top_level_help() {
    polis()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("init"));
}

#[test]
fn test_init_help_flag_succeeds() {
    polis()
        .args(["init", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--image"))
        .stdout(predicate::str::contains("--force"))
        .stdout(predicate::str::contains("--check"));
}

// ── argument validation ───────────────────────────────────────────────────────

#[test]
fn test_init_check_and_force_together_exits_nonzero_with_message() {
    let dir = TempDir::new().expect("tempdir");
    polis()
        .args(["init", "--check", "--force"])
        .env("HOME", dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("mutually exclusive"));
}

#[test]
fn test_init_image_nonexistent_path_exits_nonzero_with_message() {
    let dir = TempDir::new().expect("tempdir");
    polis()
        .args(["init", "--image", "/nonexistent/path/image.qcow2"])
        .env("HOME", dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Image file not found"));
}

#[test]
fn test_init_image_directory_path_exits_nonzero_with_message() {
    let dir = TempDir::new().expect("tempdir");
    let src_dir = dir.path().join("notafile");
    std::fs::create_dir_all(&src_dir).expect("mkdir");
    polis()
        .args(["init", "--image", src_dir.to_str().expect("utf8")])
        .env("HOME", dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not a regular file"));
}

// ── --check with no cached image ─────────────────────────────────────────────

#[test]
fn test_init_check_no_cache_exits_zero_with_message() {
    let dir = TempDir::new().expect("tempdir");
    polis()
        .args(["init", "--check"])
        .env("HOME", dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("polis init"));
}

// ── --check with cached image + metadata ─────────────────────────────────────

#[test]
fn test_init_check_with_cached_image_and_metadata_reports_up_to_date() {
    let dir = TempDir::new().expect("tempdir");
    let images = dir.path().join(".polis").join("images");
    std::fs::create_dir_all(&images).expect("mkdir");

    // Write a fake cached image
    std::fs::write(images.join("polis-workspace.qcow2"), b"fake").expect("write image");

    // Write valid image.json
    let meta = serde_json::json!({
        "version": "v0.3.0",
        "sha256": "abcdef012345abcdef012345abcdef012345abcdef012345abcdef012345abcd",
        "arch": "amd64",
        "downloaded_at": "2026-01-01T00:00:00Z",
        "source": "https://example.com/image.qcow2"
    });
    std::fs::write(images.join("image.json"), meta.to_string()).expect("write metadata");

    polis()
        .args(["init", "--check"])
        .env("HOME", dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("up to date").or(predicate::str::contains("v0.3.0")));
}

// ── --force skips cache ───────────────────────────────────────────────────────

#[test]
fn test_init_force_with_local_image_skips_cache_and_attempts_acquire() {
    let dir = TempDir::new().expect("tempdir");
    let images = dir.path().join(".polis").join("images");
    std::fs::create_dir_all(&images).expect("mkdir");

    // Pre-populate cache with metadata so without --force it would short-circuit
    let meta = serde_json::json!({
        "version": "v0.2.0",
        "sha256": "abcdef012345abcdef012345abcdef012345abcdef012345abcdef012345abcd",
        "arch": "amd64",
        "downloaded_at": "2026-01-01T00:00:00Z",
        "source": "https://example.com/image.qcow2"
    });
    std::fs::write(images.join("polis-workspace.qcow2"), b"old").expect("write old image");
    std::fs::write(images.join("image.json"), meta.to_string()).expect("write metadata");

    // Provide a real local file as --image source
    let src = dir.path().join("new.qcow2");
    std::fs::write(&src, b"new image content").expect("write src");

    // With --force it proceeds past cache check and hits the verify stub
    polis()
        .args(["init", "--force", "--image", src.to_str().expect("utf8")])
        .env("HOME", dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("issue 03"));
}

// ── no --force, no cache → hits acquire stubs ────────────────────────────────

#[test]
fn test_init_no_flags_no_cache_hits_github_resolver_stub() {
    let dir = TempDir::new().expect("tempdir");
    polis()
        .arg("init")
        .env("HOME", dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("issue 04"));
}

#[test]
fn test_init_http_url_no_cache_hits_download_stub() {
    let dir = TempDir::new().expect("tempdir");
    polis()
        .args(["init", "--image", "https://example.com/image.qcow2"])
        .env("HOME", dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("issue 02"));
}

#[test]
fn test_init_local_file_no_cache_hits_verify_stub() {
    let dir = TempDir::new().expect("tempdir");
    let src = dir.path().join("image.qcow2");
    std::fs::write(&src, b"fake image").expect("write");

    polis()
        .args(["init", "--image", src.to_str().expect("utf8")])
        .env("HOME", dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("issue 03"));
}

// ── images dir is created ─────────────────────────────────────────────────────

#[test]
fn test_init_creates_images_directory_when_missing() {
    let dir = TempDir::new().expect("tempdir");
    // Confirm it doesn't exist yet
    assert!(!dir.path().join(".polis").join("images").exists());

    // Run init (will fail at stub, but dir creation happens first)
    polis()
        .arg("init")
        .env("HOME", dir.path())
        .assert()
        .failure();

    assert!(dir.path().join(".polis").join("images").is_dir());
}

// ── property tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Any http/https URL triggers the download stub (issue 02), not a parse error.
        #[test]
        fn prop_init_http_url_reaches_download_stub(
            path in "[a-z0-9]{3,20}"
        ) {
            let dir = TempDir::new().expect("tempdir");
            let url = format!("https://example.com/{path}.qcow2");
            let output = polis()
                .args(["init", "--image", &url])
                .env("HOME", dir.path())
                .output()
                .expect("command ran");
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Must fail at stub 02, not at argument parsing
            prop_assert!(
                stderr.contains("issue 02"),
                "expected issue 02 stub error, got: {stderr}"
            );
        }

        /// --check with no cache always exits 0 and mentions `polis init`.
        #[test]
        fn prop_init_check_no_cache_always_succeeds(_seed in 0u32..100) {
            let dir = TempDir::new().expect("tempdir");
            polis()
                .args(["init", "--check"])
                .env("HOME", dir.path())
                .assert()
                .success()
                .stdout(predicate::str::contains("polis init"));
        }
    }
}
