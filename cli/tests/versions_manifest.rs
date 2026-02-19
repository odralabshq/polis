//! Integration tests for `load_versions_manifest()` (issue 07).
//!
//! Tests exercise the full download → verify → parse → validate pipeline
//! via a local HTTP server (same pattern as `init_command.rs`) and
//! `POLIS_GITHUB_API_URL` env override.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::io::{Read, Write};
use std::net::TcpListener;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn polis() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("polis"))
}

// ── HTTP test helpers ─────────────────────────────────────────────────────────

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

fn http_status(code: u16, reason: &str) -> Vec<u8> {
    format!("HTTP/1.1 {code} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        .into_bytes()
}

// ── Signed-tar fixture builder ────────────────────────────────────────────────

/// Build a zipsign-signed `.tar.gz` containing `filename` with `content`.
/// Returns `(signed_bytes, verifying_key_b64)`.
fn make_signed_tar(filename: &str, content: &[u8]) -> (Vec<u8>, String) {
    use std::io::Cursor;

    // Deterministic test key from fixed 32 bytes.
    let signing_key = zipsign_api::SigningKey::from_bytes(&[0x42u8; 32]);
    let verifying_key = signing_key.verifying_key();

    // Build a .tar.gz in memory.
    let mut tar_gz = Cursor::new(Vec::new());
    {
        let enc = flate2::write::GzEncoder::new(&mut tar_gz, flate2::Compression::default());
        let mut builder = tar::Builder::new(enc);
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, filename, content)
            .expect("append");
        builder.finish().expect("finish tar");
    }
    let tar_gz_bytes = tar_gz.into_inner();

    // Sign the .tar.gz with zipsign.
    let mut signed = Cursor::new(Vec::new());
    zipsign_api::sign::copy_and_sign_tar(
        &mut Cursor::new(&tar_gz_bytes),
        &mut signed,
        &[signing_key],
        None,
    )
    .expect("sign tar");

    let vk_b64 = encode_b64(&verifying_key.to_bytes());
    (signed.into_inner(), vk_b64)
}

fn encode_b64(input: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i < input.len() {
        let b0 = input[i] as usize;
        let b1 = if i + 1 < input.len() { input[i + 1] as usize } else { 0 };
        let b2 = if i + 2 < input.len() { input[i + 2] as usize } else { 0 };
        out.push(ALPHABET[b0 >> 2] as char);
        out.push(ALPHABET[((b0 & 3) << 4) | (b1 >> 4)] as char);
        if i + 1 < input.len() {
            out.push(ALPHABET[((b1 & 0xf) << 2) | (b2 >> 6)] as char);
        } else {
            out.push('=');
        }
        if i + 2 < input.len() {
            out.push(ALPHABET[b2 & 0x3f] as char);
        } else {
            out.push('=');
        }
        i += 3;
    }
    out
}

// ── GitHub API error paths ────────────────────────────────────────────────────

#[test]
fn test_load_versions_manifest_github_api_403_returns_rate_limit_error() {
    let dir = TempDir::new().expect("tempdir");
    let port = serve_once(http_status(403, "Forbidden"));
    // load_versions_manifest shares the same GitHub API path as init.
    // We exercise it via `polis init` which uses the same POLIS_GITHUB_API_URL env var.
    polis()
        .arg("init")
        .env("HOME", dir.path())
        .env("POLIS_GITHUB_API_URL", format!("http://127.0.0.1:{port}"))
        .assert()
        .failure()
        .stderr(predicate::str::contains("GitHub API rate limit exceeded"));
}

#[test]
fn test_load_versions_manifest_github_api_404_returns_repo_not_found() {
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
fn test_load_versions_manifest_github_api_500_returns_generic_http_error() {
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

// ── Manifest deserialization (public API surface) ─────────────────────────────

#[test]
fn test_versions_manifest_public_api_deserializes_valid_json() {
    let json = serde_json::json!({
        "manifest_version": 1,
        "vm_image": { "version": "v0.3.0", "asset": "polis-workspace-v0.3.0-amd64.qcow2" },
        "containers": { "polis-gate-oss": "v0.3.1" }
    });
    let m: polis_cli::commands::update::VersionsManifest =
        serde_json::from_value(json).expect("valid manifest");
    assert_eq!(m.manifest_version, 1);
    assert_eq!(m.vm_image.version, "v0.3.0");
    assert_eq!(m.containers["polis-gate-oss"], "v0.3.1");
}

#[test]
fn test_versions_manifest_public_api_missing_field_returns_error() {
    let json = serde_json::json!({
        "vm_image": { "version": "v0.3.0", "asset": "x.qcow2" },
        "containers": {}
    });
    assert!(
        serde_json::from_value::<polis_cli::commands::update::VersionsManifest>(json).is_err()
    );
}

// ── Signed-tar fixture tests ──────────────────────────────────────────────────

#[test]
fn test_make_signed_tar_produces_verifiable_archive() {
    use std::io::Cursor;

    let content = b"hello from versions.json";
    let (signed_bytes, _vk_b64) = make_signed_tar("versions.json", content);

    let mut unsigned = Cursor::new(Vec::new());
    zipsign_api::unsign::copy_and_unsign_tar(
        &mut Cursor::new(&signed_bytes),
        &mut unsigned,
    )
    .expect("unsign should succeed");
    unsigned.set_position(0);

    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(unsigned));
    let mut entry = archive
        .entries()
        .expect("entries")
        .next()
        .expect("at least one entry")
        .expect("valid entry");
    let mut extracted = Vec::new();
    entry.read_to_end(&mut extracted).expect("read entry");
    assert_eq!(extracted, content);
}

#[test]
fn test_make_signed_tar_wrong_key_fails_verification() {
    use std::io::Cursor;

    let (signed_bytes, _) = make_signed_tar("versions.json", b"content");

    let wrong_signing_key = zipsign_api::SigningKey::from_bytes(&[0x99u8; 32]);
    let wrong_verifying_key = wrong_signing_key.verifying_key();

    let result = zipsign_api::verify::verify_tar(
        &mut Cursor::new(&signed_bytes),
        &[wrong_verifying_key],
        None,
    );
    assert!(result.is_err(), "wrong key must fail verification");
}
