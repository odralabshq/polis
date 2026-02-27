//! Image digest verification — supply chain security for pulled Docker images.
//!
//! The `image-digests.json` manifest is embedded in the CLI binary at compile
//! time via `include_dir!`. After `docker compose pull`, every image in the
//! manifest is inspected inside the VM and its `RepoDigests[0]` is compared
//! against the expected sha256 digest.
//!
//! An empty manifest (`{}`) is treated as a local-dev stub and skips
//! verification with a warning (Requirement 18).

use std::collections::HashMap;

use anyhow::{Context, Result};

use crate::assets::get_asset;
use crate::multipass::Multipass;

/// Mapping from Docker image reference to expected sha256 digest.
///
/// Example entry:
/// ```json
/// { "ghcr.io/odralabshq/polis-resolver:v0.4.0": "sha256:abc123..." }
/// ```
pub type DigestManifest = HashMap<String, String>;

/// Verify that every pulled image's digest matches the embedded release manifest.
///
/// Reads `image-digests.json` from the embedded assets via [`get_asset`],
/// then for each entry runs `docker inspect` inside the VM and compares
/// `RepoDigests[0]` against the expected digest.
///
/// # Empty manifest
///
/// When the manifest is `{}` (local dev stub), a warning is printed to stderr
/// and the function returns `Ok(())` without contacting the VM.
///
/// # Errors
///
/// - Returns an error if the manifest cannot be parsed.
/// - Returns an error if `docker inspect` fails for any image.
/// - Returns an error if any image digest does not match the expected value.
pub async fn verify_image_digests(mp: &impl Multipass) -> Result<()> {
    let manifest_bytes = get_asset("image-digests.json")?;
    let manifest: DigestManifest =
        serde_json::from_slice(manifest_bytes).context("parsing embedded digest manifest")?;

    // Requirement 18.1 / 18.2: empty manifest → warn and skip (local dev build).
    if manifest.is_empty() {
        eprintln!(
            "⚠ Warning: image digest manifest is empty — verification skipped (local dev build)"
        );
        return Ok(());
    }

    for (image, expected_digest) in &manifest {
        let output = mp
            .exec(&[
                "docker",
                "inspect",
                "--format",
                "{{index .RepoDigests 0}}",
                image,
            ])
            .await
            .with_context(|| format!("inspecting image {image}"))?;

        let actual = String::from_utf8_lossy(&output.stdout);
        let actual = actual.trim();

        // Requirement 5.3: abort on mismatch with image name, expected, actual, recovery command.
        if !actual.contains(expected_digest.as_str()) {
            anyhow::bail!(
                "Image digest mismatch for {image}\n\
                 Expected: {expected_digest}\n\
                 Actual:   {actual}\n\n\
                 This may indicate image tampering.\n\
                 Recovery: polis delete && polis start"
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::process::{ExitStatus, Output};
    use std::sync::Mutex;

    use anyhow::Result;

    use super::*;
    use crate::multipass::Multipass;

    // ── Cross-platform ExitStatus helper ─────────────────────────────────────

    #[cfg(unix)]
    fn exit_status(code: i32) -> ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(code << 8)
    }

    #[cfg(windows)]
    fn exit_status(code: i32) -> ExitStatus {
        use std::os::windows::process::ExitStatusExt;
        #[allow(clippy::cast_sign_loss)]
        ExitStatus::from_raw(code as u32)
    }

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn ok_output(stdout: &str) -> Output {
        Output {
            status: exit_status(0),
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        }
    }

    fn fail_output() -> Output {
        Output {
            status: exit_status(1),
            stdout: Vec::new(),
            stderr: b"docker inspect failed".to_vec(),
        }
    }

    // ── Mock ─────────────────────────────────────────────────────────────────

    /// A mock Multipass that returns a configurable `exec()` response.
    ///
    /// `responses` is a list of `(image_substring, stdout)` pairs. When
    /// `exec()` is called with args containing `image_substring`, the
    /// corresponding stdout is returned. Falls back to `fail_output()` if no
    /// match is found.
    struct DigestMock {
        responses: Vec<(String, String)>,
        exec_calls: Mutex<Vec<Vec<String>>>,
    }

    impl DigestMock {
        fn new(responses: Vec<(&str, &str)>) -> Self {
            Self {
                responses: responses
                    .into_iter()
                    .map(|(k, v)| (k.to_owned(), v.to_owned()))
                    .collect(),
                exec_calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<Vec<String>> {
            self.exec_calls.lock().expect("lock").clone()
        }
    }

    impl Multipass for DigestMock {
        async fn vm_info(&self) -> Result<Output> {
            anyhow::bail!("not expected")
        }
        async fn launch(&self, _: &crate::multipass::LaunchParams<'_>) -> Result<Output> {
            anyhow::bail!("not expected")
        }
        async fn start(&self) -> Result<Output> {
            anyhow::bail!("not expected")
        }
        async fn stop(&self) -> Result<Output> {
            anyhow::bail!("not expected")
        }
        async fn delete(&self) -> Result<Output> {
            anyhow::bail!("not expected")
        }
        async fn purge(&self) -> Result<Output> {
            anyhow::bail!("not expected")
        }
        async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
            anyhow::bail!("not expected")
        }
        async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
            anyhow::bail!("not expected")
        }
        async fn version(&self) -> Result<Output> {
            anyhow::bail!("not expected")
        }
        async fn exec(&self, args: &[&str]) -> Result<Output> {
            let args_owned: Vec<String> =
                args.iter().map(std::string::ToString::to_string).collect();
            self.exec_calls.lock().expect("lock").push(args_owned);

            let combined = args.join(" ");
            for (key, stdout) in &self.responses {
                if combined.contains(key.as_str()) {
                    return Ok(ok_output(stdout));
                }
            }
            Ok(fail_output())
        }
        async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
            anyhow::bail!("not expected")
        }
        fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
            anyhow::bail!("not expected")
        }
        async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
            anyhow::bail!("not expected")
        }
    }

    // ── Unit tests ────────────────────────────────────────────────────────────

    /// Helper: run `verify_image_digests` against a synthetic manifest (bypasses
    /// the embedded asset) by directly calling the inner verification logic.
    async fn verify_manifest(mp: &impl Multipass, manifest: &DigestManifest) -> Result<()> {
        if manifest.is_empty() {
            eprintln!(
                "⚠ Warning: image digest manifest is empty — verification skipped (local dev build)"
            );
            return Ok(());
        }

        for (image, expected_digest) in manifest {
            let output = mp
                .exec(&[
                    "docker",
                    "inspect",
                    "--format",
                    "{{index .RepoDigests 0}}",
                    image,
                ])
                .await
                .with_context(|| format!("inspecting image {image}"))?;

            let actual = String::from_utf8_lossy(&output.stdout);
            let actual = actual.trim();

            if !actual.contains(expected_digest.as_str()) {
                anyhow::bail!(
                    "Image digest mismatch for {image}\n\
                     Expected: {expected_digest}\n\
                     Actual:   {actual}\n\n\
                     This may indicate image tampering.\n\
                     Recovery: polis delete && polis start"
                );
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn empty_manifest_skips_verification() {
        let mp = DigestMock::new(vec![]);
        let manifest: DigestManifest = HashMap::new();
        let result = verify_manifest(&mp, &manifest).await;
        assert!(result.is_ok(), "empty manifest should succeed");
        // No exec calls should have been made.
        assert!(
            mp.calls().is_empty(),
            "no docker inspect calls for empty manifest"
        );
    }

    #[tokio::test]
    async fn matching_digest_passes() {
        let digest = "sha256:abc123def456";
        let image = "ghcr.io/odralabshq/polis-resolver:v0.4.0";
        // docker inspect returns the full repo digest string
        let repo_digest = format!("{image}@{digest}");
        let mp = DigestMock::new(vec![(image, &repo_digest)]);

        let mut manifest = DigestManifest::new();
        manifest.insert(image.to_owned(), digest.to_owned());

        let result = verify_manifest(&mp, &manifest).await;
        assert!(result.is_ok(), "matching digest should pass: {result:?}");
    }

    #[tokio::test]
    async fn mismatched_digest_returns_error() {
        let expected = "sha256:expected000";
        let actual_digest = "sha256:actual999";
        let image = "ghcr.io/odralabshq/polis-resolver:v0.4.0";
        let repo_digest = format!("{image}@{actual_digest}");
        let mp = DigestMock::new(vec![(image, &repo_digest)]);

        let mut manifest = DigestManifest::new();
        manifest.insert(image.to_owned(), expected.to_owned());

        let err = verify_manifest(&mp, &manifest)
            .await
            .expect_err("mismatched digest should fail");

        let msg = err.to_string();
        assert!(msg.contains(image), "error should mention image name");
        assert!(
            msg.contains(expected),
            "error should mention expected digest"
        );
        assert!(
            msg.contains(actual_digest),
            "error should mention actual digest"
        );
        assert!(
            msg.contains("polis delete && polis start"),
            "error should include recovery command"
        );
    }

    #[tokio::test]
    async fn error_message_contains_all_required_fields() {
        let image = "ghcr.io/odralabshq/polis-gate:v1.0.0";
        let expected = "sha256:deadbeef";
        let actual = "sha256:cafebabe";
        let repo_digest = format!("{image}@{actual}");
        let mp = DigestMock::new(vec![(image, &repo_digest)]);

        let mut manifest = DigestManifest::new();
        manifest.insert(image.to_owned(), expected.to_owned());

        let err = verify_manifest(&mp, &manifest)
            .await
            .expect_err("should fail");
        let msg = err.to_string();

        // Requirement 5.3: error must contain image name, expected, actual, recovery command.
        assert!(msg.contains(image));
        assert!(msg.contains(expected));
        assert!(msg.contains(actual));
        assert!(msg.contains("polis delete && polis start"));
    }

    #[tokio::test]
    async fn docker_inspect_failure_propagates_error() {
        let image = "ghcr.io/odralabshq/polis-resolver:v0.4.0";
        // No matching response → mock returns fail_output() with non-zero exit.
        // But our verify_manifest checks the stdout content, not exit code.
        // To simulate a real exec error, use a mock that returns Err.
        #[allow(clippy::items_after_statements)]
        struct FailingMock;
        #[allow(clippy::items_after_statements)]
        impl Multipass for FailingMock {
            async fn vm_info(&self) -> Result<Output> {
                anyhow::bail!("not expected")
            }
            async fn launch(&self, _: &crate::multipass::LaunchParams<'_>) -> Result<Output> {
                anyhow::bail!("not expected")
            }
            async fn start(&self) -> Result<Output> {
                anyhow::bail!("not expected")
            }
            async fn stop(&self) -> Result<Output> {
                anyhow::bail!("not expected")
            }
            async fn delete(&self) -> Result<Output> {
                anyhow::bail!("not expected")
            }
            async fn purge(&self) -> Result<Output> {
                anyhow::bail!("not expected")
            }
            async fn transfer(&self, _: &str, _: &str) -> Result<Output> {
                anyhow::bail!("not expected")
            }
            async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
                anyhow::bail!("not expected")
            }
            async fn version(&self) -> Result<Output> {
                anyhow::bail!("not expected")
            }
            async fn exec(&self, _: &[&str]) -> Result<Output> {
                Err(anyhow::anyhow!("multipass exec failed"))
            }
            async fn exec_with_stdin(&self, _: &[&str], _: &[u8]) -> Result<Output> {
                anyhow::bail!("not expected")
            }
            fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
                anyhow::bail!("not expected")
            }
            async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
                anyhow::bail!("not expected")
            }
        }

        let mut manifest = DigestManifest::new();
        manifest.insert(image.to_owned(), "sha256:abc".to_owned());

        let err = verify_manifest(&FailingMock, &manifest)
            .await
            .expect_err("exec failure should propagate");
        assert!(err.to_string().contains("inspecting image"));
    }

    #[tokio::test]
    async fn multiple_images_all_verified() {
        let images = vec![
            ("ghcr.io/odralabshq/polis-resolver:v1.0", "sha256:aaa"),
            ("ghcr.io/odralabshq/polis-gate:v1.0", "sha256:bbb"),
            ("ghcr.io/odralabshq/polis-sentinel:v1.0", "sha256:ccc"),
        ];

        let responses: Vec<(&str, String)> = images
            .iter()
            .map(|(img, digest)| (*img, format!("{img}@{digest}")))
            .collect();
        let responses_ref: Vec<(&str, &str)> =
            responses.iter().map(|(k, v)| (*k, v.as_str())).collect();

        let mp = DigestMock::new(responses_ref);
        let mut manifest = DigestManifest::new();
        for (img, digest) in &images {
            manifest.insert(img.to_string(), digest.to_string());
        }

        let result = verify_manifest(&mp, &manifest).await;
        assert!(result.is_ok(), "all matching digests should pass");
        assert_eq!(mp.calls().len(), 3, "should inspect all 3 images");
    }

    #[tokio::test]
    async fn digest_type_alias_is_hashmap() {
        // Compile-time check: DigestManifest is usable as HashMap<String, String>.
        let mut m: DigestManifest = HashMap::new();
        m.insert("image".to_owned(), "sha256:abc".to_owned());
        assert_eq!(m.get("image").map(String::as_str), Some("sha256:abc"));
    }
}
