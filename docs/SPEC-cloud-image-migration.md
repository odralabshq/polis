# Polis VM: Multipass + Cloud-Init Migration Specification

## Goal

Eliminate the Packer build pipeline entirely. Replace baked VM images (qcow2/VHDX) with Multipass + cloud-init provisioning at first launch. This removes ~7 GB release artifacts, the S3/CloudFront CDN, and all Packer/goss infrastructure.

## Current State

| Component | Detail |
|---|---|
| Build tool | Packer (QEMU + Hyper-V builders) |
| Base image | Ubuntu 24.04 live-server ISO (3.2 GB) |
| Docker images | Baked into VM as overlay2 layers (~2-3 GB) |
| Release artifacts | qcow2 (~2-7 GB) + VHDX (~2-7 GB) per release |
| Distribution | S3 + CloudFront CDN + GitHub Release attachments |
| CI jobs | `vm` (ubuntu-24.04), `vm-hyperv` (windows-2022), `cdn` |
| CLI flow | Download qcow2 → verify signature → `multipass launch file://image.qcow2` |
| Provisioning | 10+ shell scripts run by Packer provisioners |
| Validation | 5 goss test suites run inside Packer build |

## Target State

| Component | Detail |
|---|---|
| Build tool | None (cloud-init at launch time) |
| Base image | Canonical Ubuntu 24.04 cloud image (managed by Multipass) |
| Docker images | Pulled from GHCR at launch time via `docker compose pull` |
| Release artifacts | CLI binary only (~5 MB) |
| Distribution | GitHub Release (CLI tarball only) |
| CI jobs | `docker` (build + push to GHCR), `cli` (build binary) |
| CLI flow | `multipass launch 24.04 --cloud-init <yaml> --timeout 900` |
| Provisioning | cloud-init (OS setup) + CLI (polis config transfer + image pull) |
| Validation | Post-launch health check via `multipass exec` |

## Architecture

### Two-Phase Provisioning

The provisioning splits into two phases:

**Phase 1 — cloud-init (runs during `multipass launch`)**
- Install Docker CE from the official Docker apt repo (GPG fingerprint verified)
- Install Sysbox (version-pinned + SHA256 verified)
- Configure Docker daemon with Sysbox runtime
- Apply VM hardening (sysctl, auditd, AppArmor, password lockdown)
- Install utilities (netcat-openbsd, jq)
- Configure ubuntu user with docker group membership
- Install `polis.service` systemd unit (auto-starts services on VM boot)

**Phase 2 — CLI orchestration (runs after `multipass launch` completes)**
- Transfer polis config bundle (docker-compose.yml, scripts, certs, pre-generated agent artifacts) via `multipass transfer`
- Pull Docker images via `multipass exec -- docker compose pull`
- Verify image digests against release manifest
- Start services via `multipass exec -- docker compose up -d`
- Run health check

This split keeps cloud-init static (no version templating) while the CLI handles all version-specific orchestration.

### Unified Init Image (`polis-init`)

The current compose file uses three init containers:

| Container | Image | Purpose |
|---|---|---|
| `host-init` | `dhi.io/alpine-base:3.23-dev` (built) | Host iptables + bridge setup |
| `scanner-init` | `dhi.io/alpine-base:3.23-dev` (pulled) | `chown` ClamAV volume |
| `state-init` | `dhi.io/alpine-base:3.23-dev` (pulled) | `chown` Valkey volume |

`scanner-init` and `state-init` pull directly from `dhi.io` at runtime, which requires DHI authentication. This breaks the cloud-init migration where users pull all images from public GHCR.

The fix: consolidate all three into a single `polis-init` image built in CI and pushed to GHCR. Rename `services/host-init` to `services/init`.

**Naming convention**: The env var remains `POLIS_HOST_INIT_VERSION` (not renamed to `POLIS_INIT_VERSION`) to minimize churn across compose files, POC scripts, and the release workflow. The image name on GHCR changes to `polis-init-oss` but the compose variable stays the same. The release workflow's version loop (`for svc in RESOLVER CERTGEN GATE SENTINEL SCANNER WORKSPACE HOST_INIT STATE TOOLBOX`) requires no changes.

```dockerfile
# services/init/Dockerfile
FROM dhi.io/alpine-base:3.23-dev@sha256:2b318097...
RUN apk add --no-cache docker-cli iptables
```

The Dockerfile is identical to the current `host-init` — DHI alpine base + `docker-cli` + `iptables`. The `docker-cli` and `iptables` packages add ~15 MB, negligible for an init container.

In `docker-compose.yml`, all three init containers use the same built image with different commands:

```yaml
  host-init:
    image: ghcr.io/odralabshq/polis-init-oss:${POLIS_HOST_INIT_VERSION:-latest}
    build:
      context: .
      dockerfile: services/init/Dockerfile
    command: ["sh", "-c", "INT_BR=\"br-$$(docker network inspect ... )\" ..."]
    # ... existing host-init config (NET_ADMIN, host network, etc.)

  scanner-init:
    image: ghcr.io/odralabshq/polis-init-oss:${POLIS_HOST_INIT_VERSION:-latest}
    command: chown -R 65532:65532 /var/lib/clamav
    # ... existing scanner-init config (CHOWN cap, read_only, etc.)

  state-init:
    image: ghcr.io/odralabshq/polis-init-oss:${POLIS_HOST_INIT_VERSION:-latest}
    command: chown -R 65532:65532 /data
    # ... existing state-init config (CHOWN cap, read_only, etc.)
```

This eliminates all runtime `dhi.io` pulls. DHI auth is only needed at CI build time (already configured). Users pull everything from public GHCR.

### Why Two Phases?

- Cloud-init YAML is a static file — no templating engine, no variable substitution for image tags
- Docker image versions change per release; the CLI already knows the version
- `multipass transfer` is the idiomatic way to move files into an instance
- Keeps cloud-init focused on OS-level setup (cacheable, testable independently)

## Cloud-Init Design

The production `cloud-init.yaml` replaces all Packer provisioner scripts. It ships alongside the CLI binary (or is embedded in it).

```yaml
#cloud-config
hostname: polis-vm
manage_etc_hosts: true

users:
  - name: ubuntu
    sudo: ALL=(ALL) NOPASSWD:ALL
    shell: /bin/bash
    lock_passwd: true
    groups: [docker]

package_update: true
package_upgrade: false

# Base packages — Docker CE installed via runcmd (official apt repo)
packages:
  - netcat-openbsd
  - jq
  - auditd
  - ca-certificates
  - curl
  - gnupg
  - openssl

write_files:
  - path: /etc/docker/daemon.json
    content: |
      {
        "runtimes": {
          "sysbox-runc": { "path": "/usr/bin/sysbox-runc" }
        },
        "no-new-privileges": true,
        "userland-proxy": false
      }
    permissions: '0644'

  - path: /etc/sysctl.d/99-polis-hardening.conf
    content: |
      kernel.randomize_va_space = 2
      kernel.dmesg_restrict = 1
      kernel.kptr_restrict = 2
      fs.suid_dumpable = 0
      kernel.yama.ptrace_scope = 2
    permissions: '0644'

  - path: /etc/audit/rules.d/docker.rules
    content: |
      -w /usr/bin/docker -p rwxa -k docker
      -w /var/lib/docker -p rwxa -k docker
      -w /etc/docker -p rwxa -k docker
      -w /usr/lib/systemd/system/docker.service -p rwxa -k docker
      -w /etc/default/docker -p rwxa -k docker
      -w /etc/docker/daemon.json -p rwxa -k docker
      -w /usr/bin/containerd -p rwxa -k docker
    permissions: '0644'

  - path: /etc/systemd/system/polis.service
    content: |
      [Unit]
      Description=Polis Secure Workspace
      After=docker.service sysbox.service
      Requires=docker.service

      [Service]
      Type=oneshot
      RemainAfterExit=yes
      WorkingDirectory=/opt/polis
      ExecStartPre=/opt/polis/scripts/setup-certs.sh
      ExecStart=/usr/bin/docker compose up -d
      ExecStop=/usr/bin/docker compose down
      TimeoutStartSec=120
      User=root

      [Install]
      WantedBy=multi-user.target
    permissions: '0644'

runcmd:
  # Install Docker CE from official apt repo (GPG fingerprint verified, with retry)
  - |
    set -euo pipefail
    DOCKER_GPG_FINGERPRINT="9DC858229FC7DD38854AE2D88D81803C0EBFCD88"
    install -m 0755 -d /etc/apt/keyrings
    curl -fsSL https://download.docker.com/linux/ubuntu/gpg | tee /etc/apt/keyrings/docker.asc > /dev/null
    chmod a+r /etc/apt/keyrings/docker.asc
    ACTUAL=$(gpg --show-keys --with-colons --with-fingerprint /etc/apt/keyrings/docker.asc 2>/dev/null | awk -F: '/^fpr/{print $10; exit}')
    if [ "${ACTUAL}" != "${DOCKER_GPG_FINGERPRINT}" ]; then
      echo "FATAL: Docker GPG fingerprint mismatch (got ${ACTUAL})" >&2
      exit 1
    fi
    echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/ubuntu $(. /etc/os-release && echo "${VERSION_CODENAME}") stable" | tee /etc/apt/sources.list.d/docker.list > /dev/null
    for i in 1 2 3; do
      apt-get update && break
      echo "apt-get update failed (attempt $i/3), retrying in 10s..." >&2
      sleep 10
    done
    apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
    usermod -aG docker ubuntu

  # Install Sysbox (SHA256 verified, with retry)
  # NOTE: amd64 only — Sysbox does not support arm64 as of v0.6.7
  - |
    set -euo pipefail
    SYSBOX_VERSION="0.6.7"
    SYSBOX_SHA256="b7ac389e5a19592cadf16e0ca30e40919516128f6e1b7f99e1cb4ff64554172e"
    DEB="sysbox-ce_${SYSBOX_VERSION}.linux_amd64.deb"
    for i in 1 2 3; do
      curl -fsSL -o "/tmp/${DEB}" "https://github.com/nestybox/sysbox/releases/download/v${SYSBOX_VERSION}/${DEB}" && break
      echo "Sysbox download failed (attempt $i/3), retrying in 10s..." >&2
      sleep 10
    done
    ACTUAL=$(sha256sum "/tmp/${DEB}" | awk '{print $1}')
    if [ "${ACTUAL}" != "${SYSBOX_SHA256}" ]; then
      echo "FATAL: Sysbox SHA256 mismatch (expected ${SYSBOX_SHA256}, got ${ACTUAL})" >&2
      exit 1
    fi
    apt-get install -y "/tmp/${DEB}"
    rm -f "/tmp/${DEB}"

  # Restart Docker cleanly — Sysbox postinst stops Docker; wipe stale netns state
  - systemctl stop docker.socket docker || true
  - systemctl stop sysbox sysbox-mgr sysbox-fs || true
  - rm -f /var/run/docker/netns/*
  - systemctl start sysbox-mgr sysbox-fs sysbox || true
  - systemctl reset-failed docker || true
  - systemctl start docker
  - sleep 5

  # Apply hardening
  - sysctl --system
  - systemctl enable apparmor
  - systemctl start apparmor || true
  - systemctl enable auditd
  - systemctl restart auditd || true

  # Create polis directory and enable polis.service
  - mkdir -p /opt/polis
  - chown ubuntu:ubuntu /opt/polis
  - systemctl daemon-reload
  - systemctl enable polis.service

  # Lock password accounts (CLI uses multipass exec, not SSH password auth)
  - passwd -l ubuntu
  - passwd -l root

final_message: "Polis VM provisioned in $UPTIME seconds"
```

### Key Design Decisions

1. **Docker CE via `runcmd` (not `apt:` module)** — Docker CE is installed via a shell block in `runcmd` rather than cloud-init's `apt:` module. This mirrors the working `install-docker.sh` from the Packer build exactly. The GPG fingerprint `9DC858229FC7DD38854AE2D88D81803C0EBFCD88` is verified before the repo is added.

2. **Docker CE, not `docker.io`** — Ubuntu's `docker.io` package ships runc 1.3.x which conflicts with Sysbox's hardened `/proc` (`unsafe procfs detected: openat2 fsmount`). Docker CE ships its own runc that works correctly with Sysbox. This is the same package used in the Packer build.

3. **Sysbox via `runcmd`** — No apt repo exists for Sysbox; it's distributed as a .deb on GitHub. The SHA256 verification matches the current `install-sysbox.sh` approach.

4. **Clean Docker restart after Sysbox install** — Sysbox's postinst script stops Docker. The restart sequence explicitly stops `docker.socket` (not just `docker.service`), stops all Sysbox services, wipes stale `/var/run/docker/netns/*` files, then starts Sysbox and Docker in the correct order. Skipping any of these steps causes bind-mount failures when containers start.

5. **`live-restore: false` (omitted from daemon.json)** — `live-restore: true` is intentionally omitted. When Docker restarts with `live-restore: true`, it attempts to re-attach to existing network namespaces managed by Sysbox, which fails because Sysbox manages container networking independently from the Docker daemon. This causes DNS resolution failures and network breakage inside Sysbox containers after a daemon restart. This is a [confirmed Sysbox bug](https://github.com/nestybox/sysbox/issues/270) (open since 2021, unresolved). The Packer build's `harden-vm.sh` sets `live-restore: true` and the goss tests assert it — this was incorrect but never triggered because the Packer-built VM never restarts Docker after containers are running. The goss tests should be considered stale validation. The cloud-init config correctly omits `live-restore` to avoid this issue. **Action**: Update `packer/goss/goss-docker.yaml` to remove the `live-restore: true` assertion before deleting the Packer directory, so the git history reflects the corrected understanding.

6. **`package_upgrade: false`** — Skipping full upgrade saves 2-3 minutes. Security updates come from the base image (Multipass downloads the latest 24.04.x point release).

7. **Hardening via `write_files:`** — Sysctl, audit rules, Docker daemon config, and the `polis.service` systemd unit are written as static files before `runcmd:` executes. This is more reliable than heredocs in shell scripts.

8. **`set -euo pipefail` in all runcmd blocks** — Each multi-line `runcmd` block begins with `set -euo pipefail` to ensure any failure within the block causes cloud-init to report `error` status (exit code 1) rather than silently continuing. This is critical for the CLI's `verify_cloud_init()` check — cloud-init only reports errors for runcmd failures if the generated script actually exits non-zero. Without `set -e`, a failed `apt-get install` would be swallowed and cloud-init would report `status: done`.

9. **`lock_passwd: true` + explicit password lock** — The `ubuntu` user is created with `lock_passwd: true` (cloud-init locks the password at user creation time). Additionally, `passwd -l ubuntu` and `passwd -l root` are run at the end of `runcmd` as defense-in-depth. The CLI uses `multipass exec` which bypasses password authentication entirely — no password is ever needed. This matches the Packer build's cleanup step which locked both accounts.

10. **`polis.service` systemd unit in cloud-init** — The Packer build created this unit in `install-polis.sh`. The cloud-init migration writes it via `write_files:` and enables it in `runcmd`. This ensures `polis start` → `polis stop` → `multipass restart polis` → services auto-start correctly, because `vm::restart()` calls `systemctl start polis`.

11. **No yq in the VM** — yq is not installed in the VM. Agent artifacts (compose overlays, systemd units, metadata) are pre-generated at CI/release time using `generate-agent.sh` (which requires yq) and bundled into the config tarball. The CLI reads pre-generated flat files (e.g., `.generated/metadata.json`, `.generated/runtime-user`) instead of shelling out to yq inside the VM. This eliminates a supply chain risk (downloading an unverified binary at launch time) and reduces the VM's attack surface.

## CLI Changes

### `cli/src/multipass.rs` — Update `launch` Signature

Replace the positional parameter list with a `LaunchParams` struct to avoid breaking test doubles on future changes:

```rust
/// Parameters for `multipass launch`.
pub struct LaunchParams<'a> {
    pub image: &'a str,
    pub cpus: &'a str,
    pub memory: &'a str,
    pub disk: &'a str,
    pub cloud_init: Option<&'a str>,
}
```

Update the trait method:

```rust
// Before:
async fn launch(&self, image_url: &str, cpus: &str, memory: &str, disk: &str) -> Result<Output>;

// After:
async fn launch(&self, params: &LaunchParams<'_>) -> Result<Output>;
```

Production implementation:

```rust
async fn launch(&self, params: &LaunchParams<'_>) -> Result<Output> {
    let mut args = vec![
        "launch", params.image,
        "--name", VM_NAME,
        "--cpus", params.cpus,
        "--memory", params.memory,
        "--disk", params.disk,
        "--timeout", "900",
    ];
    if let Some(ci) = params.cloud_init {
        args.push("--cloud-init");
        args.push(ci);
    }
    tokio::process::Command::new("multipass")
        .args(&args)
        .output()
        .await
        .context("failed to run multipass launch")
}
```

Test doubles only need to implement `async fn launch(&self, _: &LaunchParams<'_>) -> Result<Output>` — future parameter additions to `LaunchParams` won't break existing mocks (new fields default via `..Default::default()` or are `Option`).

Key changes:
- `image_url` becomes `image` — now `"24.04"` instead of `"file:///path/to/image.qcow2"`
- `--timeout 900` — 15 minutes for cloud-init to complete (Docker + Sysbox install)
- `--cloud-init` — path to the cloud-init YAML file

### `cli/src/workspace/vm.rs` — Update `create`

```rust
pub async fn create(mp: &impl Multipass, cloud_init_path: &Path, quiet: bool) -> Result<()> {
    check_prerequisites(mp).await?;
    // ... progress messages ...

    let ci_str = cloud_init_path.to_string_lossy();
    let params = LaunchParams {
        image: "24.04",
        cpus: VM_CPUS,
        memory: VM_MEMORY,
        disk: VM_DISK,
        cloud_init: Some(&ci_str),
    };
    let output = mp
        .launch(&params)
        .await
        .context("launching workspace")?;

    // Verify cloud-init completed successfully before proceeding to Phase 2
    verify_cloud_init(mp).await?;

    // ... error handling, credential transfer, service start ...
}

/// Verify cloud-init completed without errors.
///
/// Uses `cloud-init status` exit codes (cloud-init 24.1+):
///   0 = success
///   1 = unrecoverable error (critical failure)
///   2 = recoverable error (degraded — something went wrong but cloud-init completed)
///
/// Both exit code 1 and 2 are treated as failures because our runcmd blocks
/// use `set -euo pipefail`, so any provisioning failure (Docker, Sysbox, etc.)
/// will cause cloud-init to report an error status.
async fn verify_cloud_init(mp: &impl Multipass) -> Result<()> {
    let output = mp.exec(&["cloud-init", "status", "--wait"]).await
        .context("checking cloud-init status")?;
    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let exit_code = output.status.code().unwrap_or(-1);
        let hint = if exit_code == 1 {
            "Critical failure — cloud-init could not complete."
        } else {
            "Degraded — cloud-init completed with errors."
        };
        anyhow::bail!(
            "Cloud-init provisioning failed ({hint})\n\
             Status: {stdout}\n\n\
             View logs: polis exec -- cat /var/log/cloud-init-output.log\n\
             Fix: polis delete && polis start"
        );
    }
    Ok(())
}
```

### `cli/src/workspace/image.rs` — Simplify or Remove

The entire image download/verification/caching module becomes unnecessary:
- No more qcow2 download from GitHub releases
- No more SHA256 sidecar verification
- No more `images_dir()` cache management
- No more `resolve_latest_image_url()` GitHub API calls

Replace with a simple function that writes the embedded cloud-init YAML to a temp file:

```rust
/// Write the embedded cloud-init YAML to a temporary file.
/// Returns the path and the TempDir guard — the guard must be held by the
/// caller until `multipass launch` completes, then dropped for auto-cleanup.
pub fn write_cloud_init() -> Result<(PathBuf, tempfile::TempDir)> {
    let yaml = include_str!("../../cloud-init.yaml");
    let dir = tempfile::tempdir().context("creating temp dir")?;
    let path = dir.path().join("cloud-init.yaml");
    std::fs::write(&path, yaml).context("writing cloud-init.yaml")?;
    Ok((path, dir))
}
```

The cloud-init.yaml is embedded in the CLI binary at compile time via `include_str!`. This means:
- No separate file to distribute
- Version is locked to the CLI release
- Temp directory is automatically cleaned up when the `TempDir` guard is dropped
- Users can override with `--cloud-init <path>` flag (future enhancement)

**Note**: `write_cloud_init()` is intentionally synchronous. It writes ~3 KB to a temp file, which completes in microseconds. The caller (`create_and_start_vm`) should call it before entering the async launch flow, avoiding the need for `spawn_blocking`.

### `cli/src/commands/start.rs` — Simplified Flow

The `create_and_start_vm` function changes from:

```
1. resolve_image_source() → download qcow2 → verify signature
2. vm::create(mp, &image_path) → multipass launch file://image.qcow2
3. setup_agent → start_compose
```

To:

```
1. image::write_cloud_init() → write embedded YAML to temp file (returns TempDir guard)
2. vm::create(mp, &cloud_init_path) → multipass launch 24.04 --cloud-init <path>
3. _guard dropped → temp file cleaned up
4. transfer_config(mp) → multipass transfer config bundle (includes pre-generated agent artifacts)
5. pull_images(mp) → multipass exec -- docker compose pull
6. verify_image_digests(mp) → verify pulled images match digest manifest
7. setup_agent → start_compose
```

New helper functions needed:

```rust
/// Transfer polis config bundle into the VM.
async fn transfer_config(mp: &impl Multipass) -> Result<()> {
    // Bundle docker-compose.yml, scripts, certs, pre-generated agent artifacts into tar
    // Transfer via multipass transfer
    // Extract inside VM via multipass exec
    // chmod +x all .sh files (Windows tar strips execute bits)
}

/// Pull Docker images inside the VM.
async fn pull_images(mp: &impl Multipass, quiet: bool) -> Result<()> {
    let output = mp.exec(&[
        "bash", "-c",
        "cd /opt/polis && docker compose pull"
    ]).await.context("pulling Docker images")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to pull Docker images: {stderr}");
    }
    Ok(())
}

/// Verify pulled image digests match the release manifest.
///
/// The digest manifest (`image-digests.json`) is embedded in the CLI binary
/// at compile time alongside the cloud-init YAML. It maps image references
/// to their expected `sha256:` digests, as recorded by the release workflow
/// after pushing to GHCR.
async fn verify_image_digests(mp: &impl Multipass) -> Result<()> {
    let manifest: HashMap<String, String> =
        serde_json::from_str(include_str!("../../image-digests.json"))
            .context("parsing embedded digest manifest")?;
    for (image, expected_digest) in &manifest {
        let output = mp.exec(&[
            "docker", "inspect", "--format",
            "{{index .RepoDigests 0}}", image,
        ]).await.context(format!("inspecting {image}"))?;
        let actual = String::from_utf8_lossy(&output.stdout);
        if !actual.contains(expected_digest) {
            anyhow::bail!(
                "Image digest mismatch for {image}\n\
                 Expected: {expected_digest}\n\
                 Actual: {actual}\n\n\
                 This may indicate image tampering. Retry with: polis delete && polis start"
            );
        }
    }
    Ok(())
}
```

### `StartArgs` — Remove `--image` Flag

The `--image` flag is no longer needed. Remove it:

```rust
// Remove:
#[arg(long)]
pub image: Option<String>,
```

Consider adding `--cloud-init` as a future override for advanced users.

## Release Workflow Changes

### Jobs to Remove

| Job | Reason |
|---|---|
| `vm` | No more QEMU image build |
| `vm-hyperv` | No more Hyper-V image build |
| `cdn` | No more S3/CloudFront upload |

### Jobs to Modify

**`docker`** — Keep build, keep GHCR push, remove tarball export:
- Remove: `docker save -o .build/polis-images.tar` step
- Remove: `Upload images tarball` artifact step
- Keep: Build all images, push to GHCR with version tags
- Images are now pulled at launch time, not baked in

**`cli`** — No changes needed (already builds standalone binary)

**`release`** — Simplify:
- Remove: Download VM artifacts (qcow2, VHDX)
- Remove: VM artifact attachments to GitHub Release
- Keep: CLI binary + checksum + signed tarball
- Update: `needs:` to `[validate, docker, cli]` (remove vm, vm-hyperv, cdn)
- Add: cloud-init.yaml as a release attachment (for reference/advanced users)

### Simplified Workflow

```yaml
jobs:
  validate:    # unchanged
  docker:      # build + push to GHCR + cosign sign + generate digest manifest + SBOM
  cli:         # build CLI binary (embeds cloud-init.yaml + image-digests.json)
    needs: [docker]  # needs digest manifest from docker job
  release:     # create GitHub Release with CLI binary + digest manifest + SBOMs
    needs: [validate, docker, cli]
```

This reduces CI time from ~30 min (docker + 2 VM builds + CDN upload) to ~10 min (docker + CLI build).

### GHCR Image Tagging Strategy

Images are tagged with both version and `latest`:
```
ghcr.io/odralabshq/polis-resolver:v0.4.0
ghcr.io/odralabshq/polis-resolver:latest
ghcr.io/odralabshq/polis-gate:v0.4.0
ghcr.io/odralabshq/polis-gate:latest
...
```

The CLI's `docker compose pull` uses the version tag from docker-compose.yml. The compose file already references images with `${POLIS_RESOLVER_VERSION}` etc., which the CLI sets before pulling.

### Image Signing and Digest Verification

All GHCR images are signed using [Sigstore cosign](https://github.com/sigstore/cosign) with keyless signing (GitHub OIDC). This provides cryptographic proof that images were built by the official release workflow and have not been tampered with.

**Release workflow additions:**

```yaml
  # In the `docker` job, after pushing images:
  - name: Install cosign
    uses: sigstore/cosign-installer@v3.8.2

  - name: Sign images and generate digest manifest
    env:
      DIGEST_MANIFEST: .build/image-digests.json
    run: |
      VERSION="${{ needs.validate.outputs.version }}"
      echo "{}" > "${DIGEST_MANIFEST}"
      for img in $(docker images --format '{{.Repository}}:{{.Tag}}' | grep "polis-.*-oss:${VERSION}"); do
        DIGEST=$(docker inspect --format='{{index .RepoDigests 0}}' "${img}" | cut -d@ -f2)
        cosign sign --yes "${img}@${DIGEST}"
        # Append to digest manifest
        jq --arg img "${img}" --arg digest "${DIGEST}" \
          '. + {($img): $digest}' "${DIGEST_MANIFEST}" > tmp.json && mv tmp.json "${DIGEST_MANIFEST}"
        echo "✓ Signed ${img}@${DIGEST}"
      done

  - name: Upload digest manifest
    uses: actions/upload-artifact@v4
    with:
      name: image-digests
      path: .build/image-digests.json
      retention-days: 1
```

The digest manifest (`image-digests.json`) is:
1. Generated during the `docker` job after pushing and signing all images
2. Uploaded as a build artifact and attached to the GitHub Release
3. Embedded in the CLI binary at compile time via `include_str!("../../image-digests.json")`
4. Used by `verify_image_digests()` after `docker compose pull` to verify every pulled image matches its expected digest

**CLI verification flow:**
```
docker compose pull → for each image: docker inspect → compare RepoDigests[0] against manifest → fail if mismatch
```

This provides defense-in-depth: even if an attacker overwrites a mutable tag on GHCR, the CLI will detect the digest mismatch and abort. Users can also independently verify signatures:
```bash
cosign verify --certificate-oidc-issuer=https://token.actions.githubusercontent.com \
  --certificate-identity-regexp='github.com/OdraLabsHQ/polis' \
  ghcr.io/odralabshq/polis-workspace-oss:v0.4.0
```

## Files to Delete

| Path | Reason |
|---|---|
| `packer/polis-vm.pkr.hcl` | Packer template — replaced by cloud-init |
| `packer/scripts/build-vm-hyperv.ps1` | Hyper-V build script |
| `packer/scripts/bundle-config-windows.ps1` | Config bundling for Packer |
| `packer/scripts/bundle-polis-config.sh` | Config bundling for Packer |
| `packer/scripts/export-images-windows.ps1` | Docker image export for Packer |
| `packer/scripts/harden-vm.sh` | Hardening — moved to cloud-init |
| `packer/scripts/install-docker.sh` | Docker install — moved to cloud-init |
| `packer/scripts/install-sysbox.sh` | Sysbox install — moved to cloud-init |
| `packer/scripts/install-polis.sh` | Polis setup — moved to CLI transfer |
| `packer/scripts/install-agents.sh` | Agent setup — moved to CLI transfer |
| `packer/scripts/load-images.sh` | Image loading — replaced by GHCR pull |
| `packer/scripts/setup-certs.sh` | Cert setup — moved to CLI transfer |
| `packer/scripts/sign-vm-hyperv.ps1` | VM signing — no VM to sign |
| `packer/goss/*.yaml` | Goss tests — replaced by health check |
| `packer/debug-goss-spec.yaml` | Goss debug spec |
| `packer/goss-spec.yaml` | Goss spec |
| `packer/packer_cache/` | Packer cache directory |
| `packer/output/` | Packer output directory |
| `services/host-init/` | Replaced by `services/init/` (unified init image) |

The entire `packer/` directory can be deleted.

## Files to Modify

| Path | Change |
|---|---|
| `cloud-init.yaml` | Production config (done — see Cloud-Init Design above) |
| `docker-compose.yml` | Replace `dhi.io/alpine-base` in `scanner-init` and `state-init` with `ghcr.io/odralabshq/polis-init-oss`; update `host-init` image reference |
| `cli/src/multipass.rs` | Add `cloud_init` param to `launch()` |
| `cli/src/workspace/vm.rs` | Use cloud-init path instead of image path; add `verify_cloud_init()` using exit codes |
| `cli/src/workspace/image.rs` | Replace with cloud-init embed + write helper |
| `cli/src/commands/start.rs` | Remove image download, add config transfer + image pull + digest verification |
| `cli/src/commands/agent.rs` | Replace yq shell-outs with reads from pre-generated flat files (`.generated/metadata.json`, `.generated/runtime-user`) |
| `scripts/generate-agent.sh` | Add generation of `metadata.json` and `runtime-user` flat files to `.generated/` |
| `.github/workflows/release.yml` | Remove vm/vm-hyperv/cdn jobs; add cosign signing + digest manifest generation; add SBOM generation |
| `Justfile` | Remove `build-vm`, `build-vm-hyperv`, related internal targets |
| `scripts/install-tools-windows.ps1` | Remove Packer, QEMU dependencies |
| `scripts/poc-cloud-init.sh` | Add `VM_NAME` input validation; remove yq dependency |

## New Files

| Path | Purpose |
|---|---|
| `services/init/Dockerfile` | Unified init image: DHI alpine-base + docker-cli + iptables |
| `image-digests.json` | Digest manifest generated by release workflow, embedded in CLI binary |

## Reproducibility Strategy

| Concern | Mitigation |
|---|---|
| Docker CE version drift | GPG fingerprint verified; latest stable from official repo |
| Sysbox version drift | Pin version + SHA256 checksum in cloud-init |
| Ubuntu base image drift | `multipass launch 24.04` tracks 24.04.x point releases (not rolling) |
| Docker GPG key rotation | Verify by fingerprint `9DC858229FC7DD38854AE2D88D81803C0EBFCD88` |
| Docker image version drift | Compose file pins images by version tag; CLI sets version env vars |
| Docker image tag mutation | CLI verifies pulled image digests against embedded manifest (`image-digests.json`); images signed with cosign keyless (GitHub OIDC) |
| Network unavailable at launch | Fail fast with clear error message — internet is required |
| Partial provisioning | cloud-init runcmd blocks use `set -euo pipefail`; CLI verifies `cloud-init status` exit code (0=ok, 1=critical, 2=degraded) before Phase 2 |

### What's NOT Reproducible (and why it's acceptable)

- **apt package minor versions**: `containerd.io` and `docker-compose-plugin` are not version-pinned (they track Docker CE compatibility). This matches how most Docker installations work.
- **Ubuntu kernel version**: Varies by 24.04.x point release. Multipass downloads the latest available. This is acceptable because kernel updates are security-critical.
- **cloud-init version**: Bundled with the Ubuntu image. Varies by point release. Cloud-init YAML is forward-compatible.

## Known Issues and Mitigations

### P1: First launch is slower than baked image (5-10 min vs 30s)

Cloud-init must download and install Docker CE (~200 MB), Sysbox (~70 MB), and pull Docker images from GHCR (~500 MB compressed). Total: ~5-10 minutes on a typical connection.

**Mitigation**:
- `--timeout 900` prevents premature timeout
- Progress is visible via `multipass launch` output
- Subsequent `polis start` (after `polis stop`) is instant — no re-provisioning
- Only `polis delete && polis start` triggers full re-provisioning

### P2: Multipass + Hyper-V timeout with numeric hostnames

[Confirmed bug](https://superuser.com/questions/1925755): Multipass on Windows/Hyper-V times out when the VM hostname contains numbers.

**Mitigation**: VM name is `polis` (no numbers). The cloud-init sets `hostname: polis-vm` (hyphen, not a number suffix). This avoids the bug.

### P3: `docker.io` vs `docker-ce` — runc incompatibility with Sysbox

Ubuntu's `docker.io` package ships runc 1.3.x which produces `unsafe procfs detected: openat2 fsmount` errors when Sysbox intercepts `/proc` access inside containers. This causes containers to fail with exit code 255.

**Mitigation**: cloud-init installs `docker-ce` from Docker's official apt repo, which ships its own runc binary compatible with Sysbox. This is the same package used in the Packer build.

### P4: Stale netns files after Sysbox install

Sysbox's postinst script stops `docker.service` but not `docker.socket`. When Docker restarts, the socket re-activates Docker before Sysbox is fully ready, leaving stale zero-byte files in `/var/run/docker/netns/`. Subsequent `docker compose up` fails with `bind-mount /proc/<pid>/ns/net -> /var/run/docker/netns/<hash>: no such file or directory`.

**Mitigation**: The restart sequence in `runcmd` explicitly stops both `docker.socket` and `docker.service`, stops all Sysbox services, wipes `/var/run/docker/netns/*`, then starts Sysbox and Docker in the correct order.

### P5: Windows tar strips execute bits from shell scripts

When the CLI bundles config files on Windows and transfers them via `multipass transfer`, tar does not preserve Unix execute permissions. Shell scripts mounted into containers (e.g. health check scripts) have mode `0666` instead of `0755`, causing `permission denied` in container health checks.

**Mitigation**: After extracting the config bundle inside the VM, run `find /opt/polis -name '*.sh' -exec chmod +x {} \;`.

### P6: Sysbox .deb download fails (GitHub rate limiting)

GitHub may rate-limit unauthenticated downloads from `github.com/nestybox/sysbox/releases`.

**Mitigation**:
- Sysbox .deb is ~70 MB, well within GitHub's anonymous download limits
- If rate-limited, `curl -fsSL` in runcmd will fail with a clear error
- Future: Mirror Sysbox .deb to GHCR or a project-controlled URL

### P7: Docker image pull fails (GHCR unavailable)

If GHCR is down during `docker compose pull`, the launch will partially succeed (VM is up, Docker is installed) but services won't start.

**Mitigation**:
- `docker compose pull` is run by the CLI (Phase 2), not cloud-init
- CLI can retry the pull on failure
- Health check detects missing images and reports actionable error

### P8: cloud-init runs again on reboot

Cloud-init may re-run modules on subsequent boots.

**Mitigation**: cloud-init's default behavior on Multipass instances is to run once per instance. The `runcmd` module only runs on first boot by default. No explicit cleanup needed.

### P9: Multipass not installed

Users need Multipass installed before running `polis start`.

**Mitigation**: Already handled — `polis doctor` checks for Multipass and provides installation instructions. The `check_prerequisites` function in `vm.rs` validates Multipass version ≥ 1.16.0.

### P10: Partial cloud-init provisioning not detected by CLI

If cloud-init fails mid-runcmd (e.g., Docker installed but Sysbox not), `multipass launch` may still return success on some backends because cloud-init runs asynchronously after the VM boots.

**Mitigation**: All multi-line runcmd blocks use `set -euo pipefail`, ensuring any command failure causes the block to exit non-zero, which cloud-init records as an error. After `multipass launch` completes, the CLI runs `multipass exec -- cloud-init status --wait` and checks the exit code: 0 = success, 1 = critical failure, 2 = recoverable error (degraded). Both non-zero codes cause the CLI to abort with an actionable error message including log location and recovery command. This is more reliable than string-matching on status output.

### P11: Health check only validates workspace container

The current `health::check()` only queries the `workspace` container. A failure in `gate`, `sentinel`, or `state` goes undetected.

**Mitigation**: This is pre-existing behavior, not introduced by this migration. A follow-up PR should expand the health check to validate all critical services. Not a blocker for this migration.

### P12: Hardcoded amd64 architecture in cloud-init

The Sysbox download URL hardcodes `amd64`. Sysbox only supports amd64 as of v0.6.7.

**Mitigation**: Acceptable for now. A comment is added to the cloud-init YAML noting the amd64 constraint. If arm64 support is added to Sysbox, the cloud-init will need architecture detection (the CLI already has `current_arch()` for this).

## POC Status

Both Windows (Hyper-V) and Linux (QEMU) POC scripts are implemented and verified:

| Script | Platform | Status |
|---|---|---|
| `scripts/poc-cloud-init.ps1` | Windows / PowerShell | ✅ Verified — completes in ~2 min |
| `scripts/poc-cloud-init.sh` | Linux / bash | ✅ Verified |

The POC scripts simulate the full 6-phase user journey:
1. Launch VM with cloud-init
2. Bundle and transfer config
3. Generate certificates
4. Load Docker images (from `.build/polis-images.tar`)
5. Start services (`docker compose up -d`)
6. Health check

All 11 containers start healthy. The POC uses locally-built images from `just build` rather than GHCR pull — production will use GHCR.

**Note**: The POC validates `docker load` from a tarball (Phase 4), not `docker compose pull` from GHCR. A separate integration test should be added to CI that validates the GHCR pull path with version-pinned tags. This can be a simple script that runs `docker compose pull` with `POLIS_*_VERSION` env vars set to a known release tag.

## Rollback Plan

If the Multipass + cloud-init approach has critical issues in production:

1. **Short-term**: The Packer pipeline exists in git history. Revert the deletion commit to restore `packer/` directory.
2. **Medium-term**: Re-add `--image` flag to `polis start` if needed to support pre-built images alongside cloud-init.
3. **Long-term**: If cloud-init proves unreliable, consider Multipass custom images built with Packer (Multipass supports `file://` URLs to local images).

The Docker images on GHCR are useful regardless of approach — they can be pulled at launch time OR baked into a VM image.

## Migration Steps (Implementation Order)

1. ✅ **Rewrite `cloud-init.yaml`** — Production config with Docker CE, Sysbox, hardening, `set -euo pipefail`, retry loops, `polis.service` unit, password lockdown
2. ✅ **POC scripts** — `scripts/poc-cloud-init.ps1` and `scripts/poc-cloud-init.sh` verified on Windows and Linux
3. **Fix goss test for `live-restore`** — Update `packer/goss/goss-docker.yaml` to remove the `live-restore: true` assertion (corrects stale validation before deletion)
4. **Consolidate init image (single atomic commit)** — Create `services/init/Dockerfile`, update `docker-compose.yml` to use `ghcr.io/odralabshq/polis-init-oss` for `host-init`, `scanner-init`, and `state-init` (keeping `POLIS_HOST_INIT_VERSION` env var); delete `services/host-init/`
5. **Remove yq from VM, pre-bundle agents** — Update `generate-agent.sh` to emit `metadata.json` and `runtime-user` flat files; update `cli/src/commands/agent.rs` to read flat files instead of yq; remove yq from `cloud-init.yaml`
6. **Update `cli/src/multipass.rs`** — Add `LaunchParams` struct, update `launch()` trait and implementation
7. **Update `cli/src/workspace/vm.rs`** — Use cloud-init path, update `create()` signature, add `verify_cloud_init()` using exit codes
8. **Rewrite `cli/src/workspace/image.rs`** — Replace download logic with `include_str!` embed for cloud-init YAML and digest manifest, return `(PathBuf, TempDir)` tuple
9. **Update `cli/src/commands/start.rs`** — New flow: cloud-init → transfer config (with pre-generated agents) → pull images → verify digests → start; remove `--image` flag
10. **Add cosign signing to release workflow** — Install cosign, sign all GHCR images with keyless (GitHub OIDC), generate `image-digests.json` manifest, attach to release
11. **Update `.github/workflows/release.yml`** — Remove vm/vm-hyperv/cdn jobs; add SBOM generation via `anchore/sbom-action`
12. **Update `Justfile`** — Remove build-vm targets, update install-tools
13. **Delete `packer/` directory** — All contents
14. **Test on Windows** — `polis start` with Hyper-V backend
15. **Test on Linux** — `polis start` with QEMU backend
16. **Test on macOS** — `polis start` with QEMU backend (if applicable)
17. **Add GHCR pull integration test** — Validate `docker compose pull` + digest verification with version-pinned tags in CI

## Tradeoffs Summary

| Aspect | Baked Image (Before) | Cloud-Init (After) |
|---|---|---|
| First launch time | ~30s | ~5-10 min |
| Subsequent start | ~5s | ~5s |
| Release artifact size | ~7 GB (qcow2 + VHDX) | ~5 MB (CLI only) |
| CI build time | ~30 min | ~10 min |
| Infrastructure cost | S3 + CloudFront CDN | None (GHCR is free for public repos) |
| Reproducibility | High (baked) | High (version-pinned, digest-verified, cosign-signed) |
| Internet required | At download time | At first launch |
| Cross-platform | Separate builds per hypervisor | Single cloud-init, Multipass handles hypervisor |
| Maintenance | 10+ shell scripts + Packer HCL + goss tests | 1 cloud-init YAML + CLI code |
| Supply chain verification | Signed qcow2 checksum sidecar | Cosign keyless signing + digest manifest + SBOM |

## Architecture Review Responses

This section documents resolutions to findings from the architecture review (`odralabs-docs/docs/review/cloud-image-migration/architecture-review.md`).

| Finding | Severity | Resolution | Status |
|---|---|---|---|
| F-001: `live-restore` parity divergence | Critical | Omitting `live-restore` is correct. Sysbox [issue #270](https://github.com/nestybox/sysbox/issues/270) confirms it breaks Sysbox's network namespace management on daemon restart, causing DNS failures. Packer goss tests were wrong. Added step 3 to fix goss before deletion. | ✅ Resolved |
| F-002: Blocking I/O in `write_cloud_init()` | Critical | Function is sync (writes ~3 KB, completes in µs). Called before async launch flow. No `spawn_blocking` needed. Documented rationale in spec. | ✅ Resolved |
| F-003: Temp directory leak via `into_path()` | Critical | Changed to return `(PathBuf, TempDir)` tuple. Guard auto-cleans on drop. | ✅ Resolved |
| F-004: yq download has no SHA256 verification | High | yq removed from VM entirely. Agents pre-bundled at CI time. CLI reads pre-generated flat files. No binary download at launch time. | ✅ Resolved |
| F-005: No retry for cloud-init runcmd failures | High | Added retry loops (3 attempts, 10s backoff) for `apt-get update` and Sysbox download. Added `set -euo pipefail` to all runcmd blocks. CLI verifies `cloud-init status` exit code (0/1/2) before Phase 2. | ✅ Resolved |
| F-006: `launch()` trait change breaks test doubles | High | Replaced positional params with `LaunchParams` struct. Future additions won't break mocks. | ✅ Resolved |
| F-007: `polis-init` image not in release workflow | High | Clarified: `POLIS_HOST_INIT_VERSION` env var is kept (no rename). Release workflow loop unchanged. Only the image name on GHCR changes. | ✅ Resolved |
| F-008: Hardcoded amd64 in cloud-init | Medium | Acceptable — Sysbox only supports amd64. Added comment and P12 documenting the constraint. | ✅ Accepted |
| F-009: POC uses `docker load` not `docker compose pull` | Medium | Documented divergence. Added step 17 for GHCR pull integration test. | ✅ Tracked |
| F-010: Health check only validates workspace | Medium | Pre-existing behavior. Not a blocker. Documented as P11 for follow-up. | ✅ Accepted |
| F-011: Timeout 600 vs 900 | Medium | Spec already specifies 900. Implementation will update `multipass.rs`. | ✅ Resolved |
| F-012: Goss test suites have no replacement | Low | Documented as future `polis doctor --deep` enhancement. Not a blocker. | ✅ Accepted |
| F-013: `services/host-init/` deletion ordering | Low | Step 4 explicitly requires single atomic commit. | ✅ Resolved |
| F-014: Rollback plan references removed `--image` flag | Low | `--image` fully removed. Rollback plan updated — flag can be re-added from git history if needed. | ✅ Resolved |

## Security Audit Responses

This section documents resolutions to findings from the security audit (`docs/review/cloud-image-migration/security-audit.md`).

| Finding | Severity | Resolution | Status |
|---|---|---|---|
| V-001: yq supply chain risk | High | yq removed from cloud-init entirely. Agents pre-bundled at CI time. CLI reads `.generated/metadata.json` and `.generated/runtime-user` instead of shelling out to yq. Step 5 added. | ✅ Resolved |
| V-002: No retry loops in live cloud-init.yaml | High | Retry loops added to Docker `apt-get update` and Sysbox download. `set -euo pipefail` added to all multi-line runcmd blocks. | ✅ Resolved |
| V-003: GHCR images use mutable tags | High | Cosign keyless signing added to release workflow (step 10). Digest manifest (`image-digests.json`) generated at release time, embedded in CLI, verified after `docker compose pull`. | ✅ Resolved |
| V-004: POC script VM_NAME injection | Medium | Input validation added to POC script (step 5). `VM_NAME` must match `^[a-zA-Z][a-zA-Z0-9-]*$`. | ✅ Resolved |
| V-005: ubuntu user password not locked | Medium | `lock_passwd: true` set in cloud-init users block. `passwd -l ubuntu` and `passwd -l root` added to end of runcmd. | ✅ Resolved |
| V-006: Packer yq download unverified | Medium | Resolved by migration — entire `packer/` directory deleted. | ✅ Resolved |
| V-007: Docker socket in host-init | Medium | Pre-existing, not introduced by migration. Documented for future improvement (pre-compute bridge ID). | ✅ Accepted |
| V-008: cloud-init status false negatives | Low | `set -euo pipefail` in runcmd blocks ensures failures propagate. `verify_cloud_init()` uses exit codes (0/1/2) instead of string matching. | ✅ Resolved |
| V-009: Signing key on disk briefly | Low | Pre-existing. Added `trap` for cleanup on failure in release workflow. | ✅ Resolved |
| V-010: DHI base image not publicly verifiable | Low | SBOM generation added to release workflow via `anchore/sbom-action` (step 11). SBOMs attached to GitHub Release. | ✅ Resolved |
| V-011: Missing polis.service systemd unit | Low | `polis.service` added to cloud-init `write_files:`. `systemctl enable polis.service` added to runcmd. | ✅ Resolved |
