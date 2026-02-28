//! VM provisioning operations: config transfer, env generation, cert/secret generation.
//!
//! Imports only from `crate::domain` and `crate::application::ports`.

use std::path::Path;

use anyhow::{Context, Result};

use crate::application::ports::{FileTransfer, ShellExecutor};

/// Transfer the embedded `polis-setup.config.tar` into the VM and extract it.
///
/// Steps:
/// 1. Validate tarball entries on the host for path traversal (V-013)
/// 2. Transfer the tarball into the VM via `multipass transfer`
/// 3. Extract to `/opt/polis` with `--no-same-owner` (V-013)
/// 4. Write `.env` with version values via `exec_with_stdin` (V-004)
/// 5. Fix execute permissions stripped by Windows tar
///
/// # Errors
///
/// Returns an error if the tarball contains path traversal entries, if any
/// multipass command fails, or if the `.env` file cannot be written.
pub async fn transfer_config(
    mp: &(impl ShellExecutor + FileTransfer),
    assets_dir: &Path,
    version: &str,
) -> Result<()> {
    let tar_path = assets_dir.join("polis-setup.config.tar");

    // 1. Validate tarball entries on the host before transferring (V-013).
    validate_tarball_paths(&tar_path).context("validating config tarball for path traversal")?;

    // 2. Transfer the single tarball into the VM.
    let tar_str = tar_path
        .to_str()
        .context("config tarball path is not valid UTF-8")?;
    let output = mp
        .transfer(tar_str, "/tmp/polis-setup.config.tar")
        .await
        .context("transferring config tarball to VM")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("multipass transfer failed: {stderr}");
    }

    // 3. Extract inside VM to /opt/polis (--no-same-owner prevents ownership manipulation).
    let output = mp
        .exec(&[
            "tar",
            "xf",
            "/tmp/polis-setup.config.tar",
            "-C",
            "/opt/polis",
            "--no-same-owner",
        ])
        .await
        .context("extracting config tarball in VM")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("tar extraction failed: {stderr}");
    }

    // Clean up the temp tarball inside the VM.
    let _ = mp.exec(&["rm", "-f", "/tmp/polis-setup.config.tar"]).await;

    // 4. Write .env with actual version values using stdin piping (V-004 — no shell interpolation).
    let env_content = generate_env_content(version);
    mp.exec_with_stdin(&["tee", "/opt/polis/.env"], env_content.as_bytes())
        .await
        .context("writing .env in VM")?;

    // 5. Fix execute permissions stripped by Windows tar (P5).
    mp.exec(&[
        "find",
        "/opt/polis",
        "-name",
        "*.sh",
        "-exec",
        "chmod",
        "+x",
        "{}",
        "+",
    ])
    .await
    .context("fixing script permissions in VM")?;

    // 6. Strip Windows CRLF line endings from shell scripts.
    // Windows tar preserves CRLF from the working tree; bash fails with
    // "\r': command not found" if not stripped.
    mp.exec(&[
        "find",
        "/opt/polis",
        "-name",
        "*.sh",
        "-exec",
        "sed",
        "-i",
        "s/\\r//",
        "{}",
        "+",
    ])
    .await
    .context("stripping CRLF from shell scripts in VM")?;

    Ok(())
}

/// Validate that a tarball contains no path traversal entries.
///
/// Checks every entry name for `../` components or absolute paths (starting
/// with `/`). Returns an error if any unsafe entry is found (V-013).
///
/// # Errors
///
/// Returns an error if the tarball cannot be read, parsed, or if any entry
/// contains a path traversal component or absolute path.
pub fn validate_tarball_paths(tar_path: &Path) -> Result<()> {
    let file =
        std::fs::File::open(tar_path).with_context(|| format!("opening {}", tar_path.display()))?;
    let mut archive = tar::Archive::new(file);
    for entry in archive.entries().context("reading tarball entries")? {
        let entry = entry.context("reading tarball entry")?;
        let path = entry.path().context("reading tarball entry path")?;
        let path_str = path.to_string_lossy();
        if path_str.starts_with('/') {
            anyhow::bail!(
                "FATAL: Config tarball contains absolute path entry: {path_str}\n\
                 This may indicate a compromised build artifact."
            );
        }
        // Check each component for `..`
        for component in path.components() {
            if component == std::path::Component::ParentDir {
                anyhow::bail!(
                    "FATAL: Config tarball contains path traversal entry: {path_str}\n\
                     This may indicate a compromised build artifact."
                );
            }
        }
    }
    Ok(())
}

/// Generate the `.env` file content from the CLI version string.
///
/// All 9 `POLIS_*_VERSION` variables are set to the same `v{version}` tag —
/// services are versioned in lockstep with the CLI.
#[must_use]
pub fn generate_env_content(version: &str) -> String {
    let tag = format!("v{version}");
    format!(
        "# Generated by polis CLI v{version}\n\
         POLIS_RESOLVER_VERSION={tag}\n\
         POLIS_CERTGEN_VERSION={tag}\n\
         POLIS_GATE_VERSION={tag}\n\
         POLIS_SENTINEL_VERSION={tag}\n\
         POLIS_SCANNER_VERSION={tag}\n\
         POLIS_WORKSPACE_VERSION={tag}\n\
         POLIS_HOST_INIT_VERSION={tag}\n\
         POLIS_STATE_VERSION={tag}\n\
         POLIS_TOOLBOX_VERSION={tag}\n"
    )
}

/// Generate certificates and secrets inside the VM.
///
/// Calls scripts in dependency order:
/// 1. scripts/generate-ca.sh — CA key + cert (idempotent skip)
/// 2. services/state/scripts/generate-certs.sh — Valkey certs (needs CA)
/// 3. services/state/scripts/generate-secrets.sh — Valkey secrets
/// 4. services/toolbox/scripts/generate-certs.sh — Toolbox certs (idempotent skip)
/// 5. scripts/fix-cert-ownership.sh — fix key ownership
///
/// All scripts are idempotent: they skip generation if files exist.
///
/// # Errors
/// Returns an error if any generation script fails.
pub async fn generate_certs_and_secrets(mp: &impl ShellExecutor) -> Result<()> {
    // SAFETY: polis_root is a compile-time constant.
    let polis_root = "/opt/polis";

    // Step 1: Generate CA (if not present)
    mp.exec(&[
        "sudo",
        "bash",
        "-c",
        &format!("{polis_root}/scripts/generate-ca.sh {polis_root}/certs/ca"),
    ])
    .await
    .context("generating CA certificate")?;

    // Step 2: Generate Valkey certs (needs CA)
    mp.exec(&[
        "sudo",
        "bash",
        "-c",
        &format!("{polis_root}/services/state/scripts/generate-certs.sh {polis_root}/certs/valkey"),
    ])
    .await
    .context("generating Valkey certificates")?;

    // Step 3: Generate Valkey secrets
    mp.exec(&[
        "sudo",
        "bash",
        "-c",
        &format!(
            "{polis_root}/services/state/scripts/generate-secrets.sh {polis_root}/secrets {polis_root}"
        ),
    ])
    .await
    .context("generating Valkey secrets")?;

    // Step 4: Generate Toolbox certs (needs CA, idempotent skip built-in)
    mp.exec(&[
        "sudo",
        "bash",
        "-c",
        &format!(
            "{polis_root}/services/toolbox/scripts/generate-certs.sh \
             {polis_root}/certs/toolbox {polis_root}/certs/ca"
        ),
    ])
    .await
    .context("generating Toolbox certificates")?;

    // Step 5: Fix ownership for container uid 65532
    mp.exec(&[
        "sudo",
        "bash",
        "-c",
        &format!("{polis_root}/scripts/fix-cert-ownership.sh {polis_root}"),
    ])
    .await
    .context("fixing certificate ownership")?;

    // Log for support diagnostics (not to stdout)
    mp.exec(&[
        "bash",
        "-c",
        "logger -t polis 'Certificate and secret generation completed'",
    ])
    .await
    .ok();

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::process::{ExitStatus, Output};

    use anyhow::Result;

    use super::*;
    use crate::application::ports::{FileTransfer, ShellExecutor};

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

    fn ok(stdout: &[u8]) -> Output {
        Output {
            status: exit_status(0),
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        }
    }

    struct TransferConfigSpy {
        transferred: std::cell::RefCell<Vec<(String, String)>>,
        exec_calls: std::cell::RefCell<Vec<Vec<String>>>,
        exec_with_stdin_calls: std::cell::RefCell<Vec<(Vec<String>, Vec<u8>)>>,
    }

    impl TransferConfigSpy {
        fn new() -> Self {
            Self {
                transferred: std::cell::RefCell::new(Vec::new()),
                exec_calls: std::cell::RefCell::new(Vec::new()),
                exec_with_stdin_calls: std::cell::RefCell::new(Vec::new()),
            }
        }
    }

    impl FileTransfer for TransferConfigSpy {
        async fn transfer(&self, src: &str, dst: &str) -> Result<Output> {
            self.transferred
                .borrow_mut()
                .push((src.to_string(), dst.to_string()));
            Ok(ok(b""))
        }
        async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
            anyhow::bail!("not expected")
        }
    }
    impl ShellExecutor for TransferConfigSpy {
        async fn exec(&self, args: &[&str]) -> Result<Output> {
            self.exec_calls
                .borrow_mut()
                .push(args.iter().map(std::string::ToString::to_string).collect());
            Ok(ok(b""))
        }
        async fn exec_with_stdin(&self, args: &[&str], stdin: &[u8]) -> Result<Output> {
            self.exec_with_stdin_calls.borrow_mut().push((
                args.iter().map(std::string::ToString::to_string).collect(),
                stdin.to_vec(),
            ));
            Ok(ok(b""))
        }
        fn exec_spawn(&self, _: &[&str]) -> Result<tokio::process::Child> {
            anyhow::bail!("not expected")
        }
        async fn exec_status(&self, _: &[&str]) -> Result<std::process::ExitStatus> {
            anyhow::bail!("not expected")
        }
    }

    fn make_safe_tarball() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let tar_path = dir.path().join("polis-setup.config.tar");
        let file = std::fs::File::create(&tar_path).expect("create tar");
        let mut builder = tar::Builder::new(file);
        let data = b"#!/bin/bash\necho hello\n";
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(&mut header, "scripts/setup.sh", data.as_ref())
            .expect("append");
        builder.finish().expect("finish");
        (dir, tar_path)
    }

    #[test]
    fn generate_env_content_contains_all_9_vars() {
        let content = generate_env_content("1.2.3");
        let expected_vars = [
            "POLIS_RESOLVER_VERSION",
            "POLIS_CERTGEN_VERSION",
            "POLIS_GATE_VERSION",
            "POLIS_SENTINEL_VERSION",
            "POLIS_SCANNER_VERSION",
            "POLIS_WORKSPACE_VERSION",
            "POLIS_HOST_INIT_VERSION",
            "POLIS_STATE_VERSION",
            "POLIS_TOOLBOX_VERSION",
        ];
        for var in &expected_vars {
            assert!(content.contains(var), "missing {var} in .env content");
        }
    }

    #[test]
    fn generate_env_content_uses_v_prefix() {
        let content = generate_env_content("1.2.3");
        assert!(
            content.contains("POLIS_RESOLVER_VERSION=v1.2.3"),
            "expected v-prefixed version tag"
        );
        assert!(
            content.contains("POLIS_TOOLBOX_VERSION=v1.2.3"),
            "expected v-prefixed version tag for TOOLBOX"
        );
    }

    #[test]
    fn generate_env_content_all_vars_same_version() {
        let content = generate_env_content("0.4.0");
        let tag = "v0.4.0";
        let count = content.matches(&format!("={tag}")).count();
        assert_eq!(
            count, 9,
            "expected exactly 9 vars set to {tag}, got {count}"
        );
    }

    #[test]
    fn generate_env_content_valid_env_syntax() {
        let content = generate_env_content("2.0.0");
        for line in content.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            assert!(
                line.contains('='),
                "line is not valid KEY=VALUE syntax: {line}"
            );
            let (key, _) = line.split_once('=').expect("split on =");
            assert!(!key.is_empty(), "key must not be empty");
        }
    }

    #[test]
    fn validate_tarball_paths_accepts_safe_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tar_path = dir.path().join("safe.tar");
        let file = std::fs::File::create(&tar_path).expect("create tar");
        let mut builder = tar::Builder::new(file);
        let data = b"hello";
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "scripts/setup.sh", data.as_ref())
            .expect("append");
        builder.finish().expect("finish");
        assert!(
            validate_tarball_paths(&tar_path).is_ok(),
            "safe tarball should pass validation"
        );
    }

    #[test]
    fn validate_tarball_paths_rejects_path_traversal() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tar_path = dir.path().join("traversal.tar");
        {
            use std::io::Write;
            let mut file = std::fs::File::create(&tar_path).expect("create tar");
            let mut header = [0u8; 512];
            let name = b"../etc/passwd";
            header[..name.len()].copy_from_slice(name);
            header[156] = b'0';
            header[124..135].copy_from_slice(b"00000000000");
            header[100..107].copy_from_slice(b"0000644");
            let sum: u32 = header.iter().map(|&b| u32::from(b)).sum::<u32>() + 8 * u32::from(b' ')
                - header[148..156].iter().map(|&b| u32::from(b)).sum::<u32>();
            let cksum = format!("{sum:06o}\0 ");
            header[148..156].copy_from_slice(cksum.as_bytes());
            file.write_all(&header).expect("write header");
            file.write_all(&[0u8; 1024]).expect("write EOF");
        }
        let result = validate_tarball_paths(&tar_path);
        assert!(result.is_err(), "path traversal tarball should be rejected");
        let msg = result.expect_err("expected Err").to_string();
        assert!(msg.contains("FATAL"), "error should contain FATAL: {msg}");
    }

    #[test]
    fn validate_tarball_paths_rejects_absolute_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tar_path = dir.path().join("absolute.tar");
        {
            use std::io::Write;
            let mut file = std::fs::File::create(&tar_path).expect("create tar");
            let mut header = [0u8; 512];
            let name = b"/etc/passwd";
            header[..name.len()].copy_from_slice(name);
            header[156] = b'0';
            header[124..135].copy_from_slice(b"00000000000");
            header[100..107].copy_from_slice(b"0000644");
            let sum: u32 = header.iter().map(|&b| u32::from(b)).sum::<u32>() + 8 * u32::from(b' ')
                - header[148..156].iter().map(|&b| u32::from(b)).sum::<u32>();
            let cksum = format!("{sum:06o}\0 ");
            header[148..156].copy_from_slice(cksum.as_bytes());
            file.write_all(&header).expect("write header");
            file.write_all(&[0u8; 1024]).expect("write EOF");
        }
        let result = validate_tarball_paths(&tar_path);
        assert!(result.is_err(), "absolute path tarball should be rejected");
        let msg = result.expect_err("expected Err").to_string();
        assert!(msg.contains("FATAL"), "error should contain FATAL: {msg}");
    }

    #[tokio::test]
    async fn transfer_config_transfers_tarball_to_vm() {
        let (dir, _tar_path) = make_safe_tarball();
        let mp = TransferConfigSpy::new();
        transfer_config(&mp, dir.path(), "1.0.0")
            .await
            .expect("transfer_config");
        let transfers = mp.transferred.borrow();
        assert_eq!(transfers.len(), 1, "expected exactly 1 transfer call");
        assert!(
            transfers[0].1.contains("/tmp/polis-setup.config.tar"),
            "expected transfer to /tmp/polis-setup.config.tar, got: {}",
            transfers[0].1
        );
    }

    #[tokio::test]
    async fn transfer_config_extracts_with_no_same_owner() {
        let (dir, _tar_path) = make_safe_tarball();
        let mp = TransferConfigSpy::new();
        transfer_config(&mp, dir.path(), "1.0.0")
            .await
            .expect("transfer_config");
        let calls = mp.exec_calls.borrow();
        let extract_call = calls
            .iter()
            .find(|args| args.contains(&"tar".to_string()) && args.contains(&"xf".to_string()));
        assert!(extract_call.is_some(), "expected a tar xf exec call");
        let extract_args = extract_call.expect("extract call");
        assert!(
            extract_args.contains(&"--no-same-owner".to_string()),
            "tar extraction must use --no-same-owner: {extract_args:?}"
        );
        assert!(
            extract_args.contains(&"/opt/polis".to_string()),
            "tar extraction must target /opt/polis: {extract_args:?}"
        );
    }

    #[tokio::test]
    async fn transfer_config_writes_env_via_exec_with_stdin() {
        let (dir, _tar_path) = make_safe_tarball();
        let mp = TransferConfigSpy::new();
        transfer_config(&mp, dir.path(), "2.3.4")
            .await
            .expect("transfer_config");
        let calls = mp.exec_with_stdin_calls.borrow();
        assert_eq!(calls.len(), 1, "expected exactly 1 exec_with_stdin call");
        let (args, stdin) = &calls[0];
        assert!(
            args.contains(&"/opt/polis/.env".to_string()),
            "exec_with_stdin should target /opt/polis/.env: {args:?}"
        );
        let content = String::from_utf8_lossy(stdin);
        assert!(
            content.contains("POLIS_RESOLVER_VERSION=v2.3.4"),
            "env content should contain versioned var: {content}"
        );
    }

    #[tokio::test]
    async fn transfer_config_fixes_sh_permissions() {
        let (dir, _tar_path) = make_safe_tarball();
        let mp = TransferConfigSpy::new();
        transfer_config(&mp, dir.path(), "1.0.0")
            .await
            .expect("transfer_config");
        let calls = mp.exec_calls.borrow();
        let chmod_call = calls
            .iter()
            .find(|args| args.contains(&"find".to_string()) && args.contains(&"chmod".to_string()));
        assert!(
            chmod_call.is_some(),
            "expected a find ... chmod +x exec call for Windows tar fix"
        );
    }

    #[tokio::test]
    async fn transfer_config_rejects_path_traversal_tarball() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tar_path = dir.path().join("polis-setup.config.tar");
        {
            use std::io::Write;
            let mut file = std::fs::File::create(&tar_path).expect("create tar");
            let mut header = [0u8; 512];
            let name = b"../etc/passwd";
            header[..name.len()].copy_from_slice(name);
            header[156] = b'0';
            header[124..135].copy_from_slice(b"00000000000");
            header[100..107].copy_from_slice(b"0000644");
            let sum: u32 = header.iter().map(|&b| u32::from(b)).sum::<u32>() + 8 * u32::from(b' ')
                - header[148..156].iter().map(|&b| u32::from(b)).sum::<u32>();
            let cksum = format!("{sum:06o}\0 ");
            header[148..156].copy_from_slice(cksum.as_bytes());
            file.write_all(&header).expect("write header");
            file.write_all(&[0u8; 1024]).expect("write EOF");
        }
        let mp = TransferConfigSpy::new();
        let result = transfer_config(&mp, dir.path(), "1.0.0").await;
        assert!(result.is_err(), "should reject path traversal tarball");
        assert!(
            mp.transferred.borrow().is_empty(),
            "no transfer should occur for unsafe tarball"
        );
    }
}
