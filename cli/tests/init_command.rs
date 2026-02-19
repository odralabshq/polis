//! Integration tests for `polis init` (issue 01 — init command skeleton).
//!
//! Tests exercise the public CLI surface via `assert_cmd`. Each test is
//! independent: filesystem side-effects are isolated with `tempfile::TempDir`
//! and `HOME` is overridden per-process via the `env()` builder.

#![allow(clippy::expect_used)]

use std::io::{Read, Write};
use std::net::TcpListener;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn polis() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("polis"))
}

/// Spin up a one-shot HTTP server that serves `response` to the first connection.
/// Returns the bound port.
fn serve_once(response: Vec<u8>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(&response);
        }
    });
    port
}

fn http_200(body: &[u8]) -> Vec<u8> {
    let mut r = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    r.extend_from_slice(body);
    r
}

fn http_status(code: u16, reason: &str) -> Vec<u8> {
    format!("HTTP/1.1 {code} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        .into_bytes()
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
    let port = serve_once(github_releases_json("v0.3.1", "amd64"));
    polis()
        .args(["init", "--check"])
        .env("HOME", dir.path())
        .env("POLIS_GITHUB_API_URL", format!("http://127.0.0.1:{port}"))
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

    let port = serve_once(github_releases_json("v0.3.0", "amd64"));
    polis()
        .args(["init", "--check"])
        .env("HOME", dir.path())
        .env("POLIS_GITHUB_API_URL", format!("http://127.0.0.1:{port}"))
        .assert()
        .success()
        .stdout(predicate::str::contains("up to date").or(predicate::str::contains("v0.3.0")));
}

// ── --check mutual exclusion with --image ─────────────────────────────────────

#[test]
fn test_init_check_and_image_together_exits_nonzero_with_message() {
    let dir = TempDir::new().expect("tempdir");
    polis()
        .args(["init", "--check", "--image", "https://example.com/img.qcow2"])
        .env("HOME", dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("--check cannot be used with --image"));
}

// ── --check with mock GitHub API ──────────────────────────────────────────────

/// Build a minimal GitHub releases JSON array with one release at `tag`.
fn github_releases_json(tag: &str, arch: &str) -> Vec<u8> {
    let body = serde_json::json!([{
        "tag_name": tag,
        "assets": [
            {
                "name": format!("polis-workspace-{arch}.qcow2"),
                "browser_download_url": format!("https://example.com/{tag}/polis-workspace-{arch}.qcow2")
            },
            {
                "name": format!("polis-workspace-{arch}.qcow2.sha256"),
                "browser_download_url": format!("https://example.com/{tag}/polis-workspace-{arch}.qcow2.sha256")
            }
        ]
    }]);
    http_200(body.to_string().as_bytes())
}

#[test]
fn test_init_check_up_to_date_prints_up_to_date() {
    let dir = TempDir::new().expect("tempdir");
    let images = dir.path().join(".polis").join("images");
    std::fs::create_dir_all(&images).expect("mkdir");

    let meta = serde_json::json!({
        "version": "v0.3.0",
        "sha256": "abcdef012345abcdef012345abcdef012345abcdef012345abcdef012345abcd",
        "arch": "amd64",
        "downloaded_at": "2026-01-01T00:00:00Z",
        "source": "https://example.com/image.qcow2"
    });
    std::fs::write(images.join("image.json"), meta.to_string()).expect("write metadata");

    let port = serve_once(github_releases_json("v0.3.0", "amd64"));
    polis()
        .args(["init", "--check"])
        .env("HOME", dir.path())
        .env("POLIS_GITHUB_API_URL", format!("http://127.0.0.1:{port}"))
        .assert()
        .success()
        .stdout(predicate::str::contains("Up to date."));
}

#[test]
fn test_init_check_update_available_prints_update_hint() {
    let dir = TempDir::new().expect("tempdir");
    let images = dir.path().join(".polis").join("images");
    std::fs::create_dir_all(&images).expect("mkdir");

    let meta = serde_json::json!({
        "version": "v0.3.0",
        "sha256": "abcdef012345abcdef012345abcdef012345abcdef012345abcdef012345abcd",
        "arch": "amd64",
        "downloaded_at": "2026-01-01T00:00:00Z",
        "source": "https://example.com/image.qcow2"
    });
    std::fs::write(images.join("image.json"), meta.to_string()).expect("write metadata");

    let port = serve_once(github_releases_json("v0.3.1", "amd64"));
    polis()
        .args(["init", "--check"])
        .env("HOME", dir.path())
        .env("POLIS_GITHUB_API_URL", format!("http://127.0.0.1:{port}"))
        .assert()
        .success()
        .stdout(predicate::str::contains("polis init --force"));
}

#[test]
fn test_init_check_not_cached_prints_download_hint() {
    let dir = TempDir::new().expect("tempdir");
    let port = serve_once(github_releases_json("v0.3.1", "amd64"));
    polis()
        .args(["init", "--check"])
        .env("HOME", dir.path())
        .env("POLIS_GITHUB_API_URL", format!("http://127.0.0.1:{port}"))
        .assert()
        .success()
        .stdout(predicate::str::contains("No image cached."))
        .stdout(predicate::str::contains("Download with: polis init"));
}

#[test]
fn test_init_check_malformed_image_json_treated_as_not_cached() {
    let dir = TempDir::new().expect("tempdir");
    let images = dir.path().join(".polis").join("images");
    std::fs::create_dir_all(&images).expect("mkdir");
    std::fs::write(images.join("image.json"), b"not valid json").expect("write");

    let port = serve_once(github_releases_json("v0.3.1", "amd64"));
    polis()
        .args(["init", "--check"])
        .env("HOME", dir.path())
        .env("POLIS_GITHUB_API_URL", format!("http://127.0.0.1:{port}"))
        .assert()
        .success()
        .stdout(predicate::str::contains("No image cached."));
}

#[test]
fn test_init_check_github_api_403_exits_nonzero_with_rate_limit_message() {
    let dir = TempDir::new().expect("tempdir");
    let port = serve_once(http_status(403, "Forbidden"));
    polis()
        .args(["init", "--check"])
        .env("HOME", dir.path())
        .env("POLIS_GITHUB_API_URL", format!("http://127.0.0.1:{port}"))
        .assert()
        .failure()
        .stderr(predicate::str::contains("GitHub API rate limit exceeded"));
}

#[test]
fn test_init_check_github_api_empty_releases_exits_nonzero() {
    let dir = TempDir::new().expect("tempdir");
    let port = serve_once(http_200(b"[]"));
    polis()
        .args(["init", "--check"])
        .env("HOME", dir.path())
        .env("POLIS_GITHUB_API_URL", format!("http://127.0.0.1:{port}"))
        .assert()
        .failure()
        .stderr(predicate::str::contains("No VM image found in recent GitHub releases."));
}

#[test]
fn test_init_check_does_not_create_image_file() {
    let dir = TempDir::new().expect("tempdir");
    let port = serve_once(github_releases_json("v0.3.1", "amd64"));
    polis()
        .args(["init", "--check"])
        .env("HOME", dir.path())
        .env("POLIS_GITHUB_API_URL", format!("http://127.0.0.1:{port}"))
        .assert()
        .success();
    assert!(
        !dir.path().join(".polis").join("images").join("polis-workspace.qcow2").exists(),
        "--check must not write any image file"
    );
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
        .stderr(predicate::str::contains("failed to read checksum file"));
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
        .stderr(
            predicate::str::contains("No VM image found in recent GitHub releases.")
                .or(predicate::str::contains("GitHub API rate limit exceeded"))
                .or(predicate::str::contains("GitHub API error"))
                .or(predicate::str::contains("GitHub repository not found"))
                .or(predicate::str::contains("failed to parse GitHub API response")),
        );
}

// ── GitHub API HTTP error paths ───────────────────────────────────────────────

#[test]
fn test_init_github_api_403_returns_rate_limit_error() {
    let dir = TempDir::new().expect("tempdir");
    let port = serve_once(http_status(403, "Forbidden"));
    polis()
        .arg("init")
        .env("HOME", dir.path())
        .env("POLIS_GITHUB_API_URL", format!("http://127.0.0.1:{port}"))
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "GitHub API rate limit exceeded (60 requests/hour unauthenticated).",
        ));
}

#[test]
fn test_init_github_api_404_returns_repo_not_found_error() {
    let dir = TempDir::new().expect("tempdir");
    let port = serve_once(http_status(404, "Not Found"));
    polis()
        .arg("init")
        .env("HOME", dir.path())
        .env("POLIS_GITHUB_API_URL", format!("http://127.0.0.1:{port}"))
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "GitHub repository not found: OdraLabsHQ/polis",
        ));
}

#[test]
fn test_init_github_api_500_returns_generic_http_error() {
    let dir = TempDir::new().expect("tempdir");
    let port = serve_once(http_status(500, "Internal Server Error"));
    polis()
        .arg("init")
        .env("HOME", dir.path())
        .env("POLIS_GITHUB_API_URL", format!("http://127.0.0.1:{port}"))
        .assert()
        .failure()
        .stderr(predicate::str::contains("GitHub API error: HTTP 500"));
}

#[test]
fn test_init_github_api_invalid_json_returns_parse_error() {
    let dir = TempDir::new().expect("tempdir");
    let port = serve_once(http_200(b"not valid json {{"));
    polis()
        .arg("init")
        .env("HOME", dir.path())
        .env("POLIS_GITHUB_API_URL", format!("http://127.0.0.1:{port}"))
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "failed to parse GitHub API response",
        ));
}

#[test]
fn test_init_github_api_empty_releases_returns_no_image_error() {
    let dir = TempDir::new().expect("tempdir");
    let port = serve_once(http_200(b"[]"));
    polis()
        .arg("init")
        .env("HOME", dir.path())
        .env("POLIS_GITHUB_API_URL", format!("http://127.0.0.1:{port}"))
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "No VM image found in recent GitHub releases.",
        ));
}

#[test]
fn test_init_http_url_no_cache_hits_download_stub() {
    let dir = TempDir::new().expect("tempdir");
    // Connection refused — real download attempt, not a stub bail.
    polis()
        .args(["init", "--image", "http://127.0.0.1:1/image.qcow2"])
        .env("HOME", dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("download interrupted").or(
            predicate::str::contains("download failed"),
        ));
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
        .stderr(predicate::str::contains("failed to read checksum file"));
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

// ── download_with_resume — integration ───────────────────────────────────────

#[test]
fn test_init_http_url_connection_refused_exits_nonzero() {
    let dir = TempDir::new().expect("tempdir");
    polis()
        .args(["init", "--image", "http://127.0.0.1:1/image.qcow2"])
        .env("HOME", dir.path())
        .assert()
        .failure();
}

#[test]
fn test_init_http_url_404_exits_nonzero_with_http_error_message() {
    let dir = TempDir::new().expect("tempdir");
    let port = serve_once(http_status(404, "Not Found"));
    polis()
        .args(["init", "--image", &format!("http://127.0.0.1:{port}/img")])
        .env("HOME", dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("download failed: HTTP 404"));
}

#[test]
fn test_init_http_url_200_download_succeeds_then_hits_verify_stub() {
    let dir = TempDir::new().expect("tempdir");
    let port = serve_once(http_200(b"fake qcow2 content"));
    polis()
        .args(["init", "--image", &format!("http://127.0.0.1:{port}/img")])
        .env("HOME", dir.path())
        .assert()
        .failure()
        // Download succeeded; verify fires next — sidecar absent → read error.
        .stderr(predicate::str::contains("failed to read checksum file"));

    // Dest file was written before verify was called.
    let dest = dir.path().join(".polis").join("images").join("polis-workspace.qcow2");
    assert!(dest.exists(), "dest file should exist after successful download");
}

// ── property tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Any http/https URL triggers a real download attempt (not a parse error).
        #[test]
        fn prop_init_http_url_reaches_download_stub(
            path in "[a-z0-9]{3,20}"
        ) {
            let dir = TempDir::new().expect("tempdir");
            // Port 1 is always refused — transport error, not an arg-parse error.
            let url = format!("http://127.0.0.1:1/{path}.qcow2");
            let output = polis()
                .args(["init", "--image", &url])
                .env("HOME", dir.path())
                .output()
                .expect("command ran");
            let stderr = String::from_utf8_lossy(&output.stderr);
            prop_assert!(!output.status.success(), "expected failure");
            prop_assert!(
                !stderr.contains("Image file not found") && !stderr.contains("Not a regular file"),
                "got arg-parse error instead of download error: {stderr}"
            );
        }

        /// --check with no cache always exits 0 and mentions `polis init`.
        /// Uses mock POLIS_GITHUB_API_URL — structurally required since run_check
        /// now always calls resolve_latest_image_url().
        #[test]
        fn prop_init_check_no_cache_always_succeeds(_seed in 0u32..10) {
            let dir = TempDir::new().expect("tempdir");
            let port = serve_once(github_releases_json("v0.3.1", "amd64"));
            polis()
                .args(["init", "--check"])
                .env("HOME", dir.path())
                .env("POLIS_GITHUB_API_URL", format!("http://127.0.0.1:{port}"))
                .assert()
                .success()
                .stdout(predicate::str::contains("polis init"));
        }

        /// --check --force always fails regardless of other state.
        #[test]
        fn prop_init_check_and_force_always_fails(_seed in 0u32..10) {
            let dir = TempDir::new().expect("tempdir");
            let output = polis()
                .args(["init", "--check", "--force"])
                .env("HOME", dir.path())
                .output()
                .expect("command ran");
            prop_assert!(!output.status.success());
            let stderr = String::from_utf8_lossy(&output.stderr);
            prop_assert!(stderr.contains("mutually exclusive"), "got: {stderr}");
        }

        /// --check --image <url> always fails regardless of URL value.
        #[test]
        fn prop_init_check_and_image_always_fails(url in "https://[a-z]{3,10}\\.example\\.com/[a-z]{3,10}\\.qcow2") {
            let dir = TempDir::new().expect("tempdir");
            let output = polis()
                .args(["init", "--check", "--image", &url])
                .env("HOME", dir.path())
                .output()
                .expect("command ran");
            prop_assert!(!output.status.success());
            let stderr = String::from_utf8_lossy(&output.stderr);
            prop_assert!(stderr.contains("--check cannot be used with --image"), "got: {stderr}");
        }
    }
}
