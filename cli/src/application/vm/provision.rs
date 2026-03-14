//! VM provisioning operations: config transfer, env generation, cert/secret generation.
//!
//! Imports only from `crate::domain` and `crate::application::ports`.

use std::path::Path;

use anyhow::{Context, Result};

use crate::application::ports::{FileTransfer, InstanceInspector, ShellExecutor};
use crate::application::vm::lifecycle as vm;

/// Write the VM's external IP to `/opt/polis/.vm-ip` and append it to `.env`
/// so containers can reference it via `$POLIS_VM_IP`.
///
/// # Errors
///
/// Returns an error if the VM IP cannot be resolved or the write commands fail.
pub async fn persist_vm_ip(mp: &(impl InstanceInspector + ShellExecutor)) -> Result<()> {
    let ip = vm::resolve_vm_ip(mp).await?;
    mp.exec(&[
        "bash",
        "-c",
        &format!("printf '%s\\n' '{ip}' > /opt/polis/.vm-ip"),
    ])
    .await
    .context("writing .vm-ip")?;
    let script = format!(
        "sed -i '/^POLIS_VM_IP=/d' /opt/polis/.env 2>/dev/null; printf '%s\\n' 'POLIS_VM_IP={ip}' >> /opt/polis/.env"
    );
    mp.exec(&["bash", "-c", &script])
        .await
        .context("writing POLIS_VM_IP to .env")?;
    Ok(())
}

/// Transfer the embedded `polis-setup.config.tar` into the VM and extract it.
///
/// Steps:
/// 1. Validate tarball entries on the host for path traversal (V-013)
/// 2. Transfer the tarball into the VM via `multipass transfer`
/// 3. Extract to `/opt/polis` with `--no-same-owner` (V-013)
/// 4. Write `.env` with version values via `exec` + `printf '%s'` (V-004)
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

    // 4. Write .env with actual version values (V-004 — no shell interpolation).
    // Uses printf '%s' which treats the content as a literal string, avoiding
    // shell expansion. Does NOT use exec_with_stdin/tee because Multipass on
    // Windows fails to propagate stdin EOF, causing tee to hang indefinitely.
    let env_content = generate_env_content(version);
    mp.exec(&[
        "bash",
        "-c",
        &format!(
            "printf '%s' '{}' > /opt/polis/.env",
            env_content.replace('\'', "'\\''")
        ),
    ])
    .await
    .context("writing .env in VM")?;

    // 5+6. Fix execute permissions and strip Windows CRLF line endings in a
    // single exec to avoid the per-call SSH connection overhead on Windows
    // (no ControlMaster, Hyper-V latency per round-trip).
    // Windows tar preserves CRLF from the working tree; bash/systemd/docker
    // fail with cryptic errors if not stripped.
    mp.exec(&[
        "bash",
        "-c",
        "find /opt/polis -type f -name '*.sh' -exec chmod +x {} + && \
         find /opt/polis -type f \\( -name '*.sh' -o -name '*.yaml' -o -name '*.yml' \
           -o -name '*.env' -o -name '*.service' -o -name '*.toml' -o -name '*.conf' \\) \
           -exec sed -i 's/\\r$//' {} +",
    ])
    .await
    .context("fixing script permissions and stripping CRLF in VM")?;

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
         POLIS_TOOLBOX_VERSION={tag}\n\
         POLIS_CONTROL_PLANE_VERSION={tag}\n"
    )
}

/// Generate certificates and secrets inside the VM.
///
/// Batches all 5 scripts into a single `multipass exec` call to avoid the
/// per-invocation SSH connection overhead, which is significant on Windows
/// (no `ControlMaster`, Hyper-V latency). Scripts run in dependency order:
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

    // Batch all cert/secret generation into a single exec to avoid the
    // per-call SSH connection overhead on Windows (no ControlMaster).
    let script = format!(
        "set -e\n\
         {polis_root}/scripts/generate-ca.sh {polis_root}/certs/ca\n\
         {polis_root}/services/state/scripts/generate-certs.sh {polis_root}/certs/valkey\n\
         {polis_root}/services/state/scripts/generate-secrets.sh {polis_root}/secrets {polis_root}\n\
         {polis_root}/services/toolbox/scripts/generate-certs.sh {polis_root}/certs/toolbox {polis_root}/certs/ca\n\
         {polis_root}/scripts/fix-cert-ownership.sh {polis_root}\n\
         logger -t polis 'Certificate and secret generation completed' || true\n"
    );

    mp.exec(&["sudo", "bash", "-c", &script])
        .await
        .context("generating certificates and secrets")?;

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::process::Output;

    use anyhow::Result;

    use super::*;
    use crate::application::ports::{FileTransfer, ShellExecutor};
    use crate::application::vm::test_support::{impl_shell_executor_stubs, ok_output};

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
        /// # Errors
        ///
        /// This function will return an error if the underlying operations fail.
        async fn transfer(&self, src: &str, dst: &str) -> Result<Output> {
            self.transferred
                .borrow_mut()
                .push((src.to_string(), dst.to_string()));
            Ok(ok_output(b""))
        }
        /// # Errors
        ///
        /// This function will return an error if the underlying operations fail.
        async fn transfer_recursive(&self, _: &str, _: &str) -> Result<Output> {
            anyhow::bail!("not expected")
        }
        async fn transfer_from(&self, _: &str, _: &str) -> Result<Output> {
            Ok(ok_output(b""))
        }
    }
    impl ShellExecutor for TransferConfigSpy {
        /// # Errors
        ///
        /// This function will return an error if the underlying operations fail.
        async fn exec(&self, args: &[&str]) -> Result<Output> {
            self.exec_calls
                .borrow_mut()
                .push(args.iter().map(std::string::ToString::to_string).collect());
            Ok(ok_output(b""))
        }
        /// # Errors
        ///
        /// This function will return an error if the underlying operations fail.
        async fn exec_with_stdin(&self, args: &[&str], stdin: &[u8]) -> Result<Output> {
            self.exec_with_stdin_calls.borrow_mut().push((
                args.iter().map(std::string::ToString::to_string).collect(),
                stdin.to_vec(),
            ));
            Ok(ok_output(b""))
        }
        impl_shell_executor_stubs!(exec_spawn, exec_status);
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
            "POLIS_CONTROL_PLANE_VERSION",
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
            count, 10,
            "expected exactly 10 vars set to {tag}, got {count}"
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
    async fn transfer_config_writes_env_via_exec() {
        let (dir, _tar_path) = make_safe_tarball();
        let mp = TransferConfigSpy::new();
        transfer_config(&mp, dir.path(), "2.3.4")
            .await
            .expect("transfer_config");
        let calls = mp.exec_calls.borrow();
        let env_call = calls
            .iter()
            .find(|args| args.iter().any(|a| a.contains("POLIS_RESOLVER_VERSION")));
        assert!(
            env_call.is_some(),
            "expected an exec call writing .env with version vars"
        );
        let env_args = env_call.expect("env call");
        assert!(
            env_args.iter().any(|a| a.contains("v2.3.4")),
            "env content should contain versioned var: {env_args:?}"
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
        let combined_call = calls.iter().find(|args| {
            args.contains(&"bash".to_string()) && args.iter().any(|a| a.contains("chmod"))
        });
        assert!(
            combined_call.is_some(),
            "expected a bash -c call containing chmod +x for Windows tar fix"
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
