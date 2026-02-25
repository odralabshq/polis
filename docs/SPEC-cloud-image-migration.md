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
- Install Docker CE (version-pinned)
- Install Sysbox (version-pinned + SHA256 verified)
- Configure Docker daemon with Sysbox runtime
- Apply VM hardening (sysctl, auditd, AppArmor)
- Install utilities (netcat-openbsd, yq, jq)
- Configure ubuntu user with docker group membership

**Phase 2 — CLI orchestration (runs after `multipass launch` completes)**
- Transfer polis config bundle (docker-compose.yml, scripts, certs) via `multipass transfer`
- Transfer agent configs
- Pull Docker images via `multipass exec -- docker compose pull`
- Start services via `multipass exec -- docker compose up -d`
- Run health check

This split keeps cloud-init static (no version templating) while the CLI handles all version-specific orchestration.

### Why Two Phases?

- Cloud-init YAML is a static file — no templating engine, no variable substitution for image tags
- Docker image versions change per release; the CLI already knows the version
- `multipass transfer` is the idiomatic way to move files into an instance
- Keeps cloud-init focused on OS-level setup (cacheable, testable independently)

## Cloud-Init Design

The production `cloud-init.yaml` replaces all Packer provisioner scripts. It ships alongside the CLI binary (or is embedded in it).

```yaml
#cloud-config
# Polis Workspace — cloud-init provisioning
# Installs Docker CE, Sysbox, applies hardening.
# Polis-specific config is transferred by the CLI after launch.

hostname: polis-vm
manage_etc_hosts: true

users:
  - name: ubuntu
    sudo: ALL=(ALL) NOPASSWD:ALL
    shell: /bin/bash
    lock_passwd: false
    groups: [docker]

# ── Docker CE via apt module ────────────────────────────────────────
# Uses keyid (GPG fingerprint) for reproducible key verification.
# Avoids curl|sh pattern. cloud-init fetches and dearmors the key.
apt:
  sources:
    docker:
      keyid: 9DC858229FC7DD38854AE2D88D81803C0EBFCD88
      keyserver: https://download.docker.com/linux/ubuntu/gpg
      source: "deb [arch=amd64 signed-by=$KEY_FILE] https://download.docker.com/linux/ubuntu $RELEASE stable"

package_update: true
package_upgrade: false

packages:
  - docker-ce=5:27.5.1-1~ubuntu.24.04~noble
  - docker-ce-cli=5:27.5.1-1~ubuntu.24.04~noble
  - containerd.io
  - docker-compose-plugin
  - docker-buildx-plugin
  - netcat-openbsd
  - jq
  - auditd
  - ca-certificates
  - curl

# ── Write config files before runcmd ────────────────────────────────
write_files:
  # Docker daemon config with Sysbox runtime
  - path: /etc/docker/daemon.json
    content: |
      {
        "runtimes": {
          "sysbox-runc": { "path": "/usr/bin/sysbox-runc" }
        },
        "no-new-privileges": true,
        "live-restore": true,
        "userland-proxy": false
      }
    permissions: '0644'

  # Sysctl hardening (CIS Ubuntu 24.04 Level 1)
  - path: /etc/sysctl.d/99-polis-hardening.conf
    content: |
      kernel.randomize_va_space = 2
      kernel.dmesg_restrict = 1
      kernel.kptr_restrict = 2
      fs.suid_dumpable = 0
      kernel.yama.ptrace_scope = 2
    permissions: '0644'

  # Docker audit rules
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

runcmd:
  # ── Install yq ──────────────────────────────────────────────────
  - wget -qO /usr/local/bin/yq https://github.com/mikefarah/yq/releases/download/v4.44.6/yq_linux_amd64
  - chmod +x /usr/local/bin/yq

  # ── Install Sysbox (SHA256 verified) ────────────────────────────
  - |
    SYSBOX_VERSION="0.6.7"
    SYSBOX_SHA256="b7ac389e5a19592cadf16e0ca30e40919516128f6e1b7f99e1cb4ff64554172e"
    DEB_NAME="sysbox-ce_${SYSBOX_VERSION}.linux_amd64.deb"
    DEB_URL="https://github.com/nestybox/sysbox/releases/download/v${SYSBOX_VERSION}/${DEB_NAME}"
    curl -fsSL -o "/tmp/${DEB_NAME}" "${DEB_URL}"
    ACTUAL=$(sha256sum "/tmp/${DEB_NAME}" | awk '{print $1}')
    if [ "${ACTUAL}" != "${SYSBOX_SHA256}" ]; then
      echo "ERROR: Sysbox SHA256 mismatch! Expected ${SYSBOX_SHA256}, got ${ACTUAL}" >&2
      exit 1
    fi
    apt-get install -y "/tmp/${DEB_NAME}"
    rm -f "/tmp/${DEB_NAME}"

  # ── Restart Docker with Sysbox runtime ──────────────────────────
  - systemctl restart docker

  # ── Apply hardening ─────────────────────────────────────────────
  - sysctl --system
  - systemctl enable apparmor
  - systemctl start apparmor || true
  - systemctl enable auditd
  - systemctl restart auditd || true

  # ── Create polis directory structure ────────────────────────────
  - mkdir -p /opt/polis
  - chown ubuntu:ubuntu /opt/polis

final_message: "Polis VM ready after $UPTIME seconds"
```

### Key Design Decisions

1. **Docker via `apt:` module** — cloud-init's native apt source management handles GPG key download, dearmoring, and repo setup atomically. The `keyid` field verifies the GPG fingerprint `9DC858229FC7DD38854AE2D88D81803C0EBFCD88` (same fingerprint verified in current `install-docker.sh`). This runs before `packages:`, so Docker is installed via apt with proper signing.

2. **Docker version pinning** — `docker-ce=5:27.5.1-1~ubuntu.24.04~noble` pins to an exact version. Update this when upgrading Docker. The format is `5:<major>.<minor>.<patch>-1~ubuntu.<version>~<codename>`.

3. **Sysbox via `runcmd:`** — No apt repo exists for Sysbox; it's distributed as a .deb on GitHub. The SHA256 verification matches the current `install-sysbox.sh` approach.

4. **`package_upgrade: false`** — Skipping full upgrade saves 2-3 minutes. Security updates come from the base image (Multipass downloads the latest 24.04.x point release).

5. **Hardening via `write_files:`** — Sysctl, audit rules, and Docker daemon config are written as static files before `runcmd:` executes. This is more reliable than heredocs in shell scripts.

## CLI Changes

### `cli/src/multipass.rs` — Update `launch` Signature

Add `--cloud-init` parameter to the `launch` method:

```rust
// Before:
async fn launch(&self, image_url: &str, cpus: &str, memory: &str, disk: &str) -> Result<Output>;

// After:
async fn launch(&self, image: &str, cpus: &str, memory: &str, disk: &str, cloud_init: Option<&str>) -> Result<Output>;
```

Production implementation:

```rust
async fn launch(&self, image: &str, cpus: &str, memory: &str, disk: &str, cloud_init: Option<&str>) -> Result<Output> {
    let mut args = vec![
        "launch", image,
        "--name", VM_NAME,
        "--cpus", cpus,
        "--memory", memory,
        "--disk", disk,
        "--timeout", "900",
    ];
    if let Some(ci) = cloud_init {
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
    let output = mp
        .launch("24.04", VM_CPUS, VM_MEMORY, VM_DISK, Some(&ci_str))
        .await
        .context("launching workspace")?;

    // ... error handling, credential transfer, service start ...
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
/// Returns the path to the file (caller must clean up).
pub fn write_cloud_init() -> Result<PathBuf> {
    let yaml = include_str!("../../cloud-init.yaml");
    let dir = tempfile::tempdir().context("creating temp dir")?;
    let path = dir.into_path().join("cloud-init.yaml");
    std::fs::write(&path, yaml).context("writing cloud-init.yaml")?;
    Ok(path)
}
```

The cloud-init.yaml is embedded in the CLI binary at compile time via `include_str!`. This means:
- No separate file to distribute
- Version is locked to the CLI release
- Users can override with `--cloud-init <path>` flag (future enhancement)

### `cli/src/commands/start.rs` — Simplified Flow

The `create_and_start_vm` function changes from:

```
1. resolve_image_source() → download qcow2 → verify signature
2. vm::create(mp, &image_path) → multipass launch file://image.qcow2
3. setup_agent → start_compose
```

To:

```
1. image::write_cloud_init() → write embedded YAML to temp file
2. vm::create(mp, &cloud_init_path) → multipass launch 24.04 --cloud-init <path>
3. transfer_config(mp) → multipass transfer config bundle
4. pull_images(mp) → multipass exec -- docker compose pull
5. setup_agent → start_compose
```

New helper functions needed:

```rust
/// Transfer polis config bundle into the VM.
async fn transfer_config(mp: &impl Multipass) -> Result<()> {
    // Bundle docker-compose.yml, scripts, certs into tar
    // Transfer via multipass transfer
    // Extract inside VM via multipass exec
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
```

### `StartArgs` — Remove `--image` Flag

The `--image` flag becomes unnecessary since there's no image to download. Remove:

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
  docker:      # build + push to GHCR (no tarball export)
  cli:         # build CLI binary (unchanged)
  release:     # create GitHub Release with CLI binary only
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

The entire `packer/` directory can be deleted.

## Files to Modify

| Path | Change |
|---|---|
| `cloud-init.yaml` | Rewrite with production config (see Cloud-Init Design above) |
| `cli/src/multipass.rs` | Add `cloud_init` param to `launch()` |
| `cli/src/workspace/vm.rs` | Use cloud-init path instead of image path |
| `cli/src/workspace/image.rs` | Replace with cloud-init embed + write helper |
| `cli/src/commands/start.rs` | Remove image download, add config transfer + image pull |
| `.github/workflows/release.yml` | Remove vm/vm-hyperv/cdn jobs, simplify release |
| `Justfile` | Remove `build-vm`, `build-vm-hyperv`, related internal targets |
| `scripts/install-tools-windows.ps1` | Remove Packer, QEMU dependencies |
| `Justfile` install-tools | Remove Packer, QEMU, xorriso dependencies |

## Reproducibility Strategy

| Concern | Mitigation |
|---|---|
| Docker CE version drift | Pin exact version: `docker-ce=5:27.5.1-1~ubuntu.24.04~noble` |
| Sysbox version drift | Pin version + SHA256 checksum in cloud-init |
| Ubuntu base image drift | `multipass launch 24.04` tracks 24.04.x point releases (not rolling) |
| Docker GPG key rotation | Verify by fingerprint `9DC858229FC7DD38854AE2D88D81803C0EBFCD88` via `keyid` |
| Docker image version drift | Compose file pins images by version tag; CLI sets version env vars |
| yq version drift | Pin download URL to specific release tag (`v4.44.6`) |
| Network unavailable at launch | Fail fast with clear error message — internet is required |
| Partial provisioning | cloud-init is atomic per module; `runcmd` failures are logged |

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

### P3: cloud-init `apt:` module + GPG key race condition

[Known issue](https://github.com/canonical/cloud-init/issues/5223): On Ubuntu 24.04 cloud images, `gpg` may not be pre-installed. If cloud-init tries to dearmor a GPG key before `gpg` is available, package installation fails.

**Mitigation**: The `keyid` + `keyserver` approach in the `apt:` module handles this correctly — cloud-init installs `gpg` as a dependency before processing apt sources. This was fixed in cloud-init 24.2+, which ships with Ubuntu 24.04.1+.

### P4: Sysbox .deb download fails (GitHub rate limiting)

GitHub may rate-limit unauthenticated downloads from `github.com/nestybox/sysbox/releases`.

**Mitigation**: 
- Sysbox .deb is ~70 MB, well within GitHub's anonymous download limits
- If rate-limited, the `curl -fsSL` in runcmd will fail, cloud-init will report the error, and `multipass launch` will fail with a clear message
- Future: Mirror Sysbox .deb to GHCR or a project-controlled URL

### P5: Docker image pull fails (GHCR unavailable)

If GHCR is down during `docker compose pull`, the launch will partially succeed (VM is up, Docker is installed) but services won't start.

**Mitigation**:
- `docker compose pull` is run by the CLI (Phase 2), not cloud-init
- CLI can retry the pull on failure
- Health check detects missing images and reports actionable error
- User can manually retry: `polis start` (CLI detects running VM, retries pull)

### P6: cloud-init runs again on reboot

Cloud-init may re-run modules on subsequent boots.

**Mitigation**: cloud-init's default behavior on Multipass instances is to run once per instance. The `runcmd` module only runs on first boot by default. No explicit cleanup needed.

### P7: Disk space — 40 GB default may be excessive

Current VM uses 40 GB disk. With no baked images, the actual usage is lower.

**Mitigation**: Reduce `VM_DISK` from `"40G"` to `"20G"`. Docker images + overlay2 + OS ≈ 8-10 GB. 20 GB provides comfortable headroom.

### P8: Multipass not installed

Users need Multipass installed before running `polis start`.

**Mitigation**: Already handled — `polis doctor` checks for Multipass and provides installation instructions. The `check_prerequisites` function in `vm.rs` validates Multipass version ≥ 1.16.0.

## Rollback Plan

If the Multipass + cloud-init approach has critical issues in production:

1. **Short-term**: The Packer pipeline exists in git history. Revert the deletion commit to restore `packer/` directory.
2. **Medium-term**: The `--image` flag can be re-added to `polis start` to support pre-built images alongside cloud-init.
3. **Long-term**: If cloud-init proves unreliable, consider Multipass custom images built with Packer (Multipass supports `file://` URLs to local images).

The Docker images on GHCR are useful regardless of approach — they can be pulled at launch time OR baked into a VM image.

## Migration Steps (Implementation Order)

1. **Rewrite `cloud-init.yaml`** — Production config with Docker, Sysbox, hardening (as specified above)
2. **Update `cli/src/multipass.rs`** — Add `cloud_init` parameter to `launch()` trait and implementation
3. **Update `cli/src/workspace/vm.rs`** — Use cloud-init path, update `create()` signature
4. **Rewrite `cli/src/workspace/image.rs`** — Replace download logic with `include_str!` embed
5. **Update `cli/src/commands/start.rs`** — New flow: cloud-init → transfer config → pull images → start
6. **Update `.github/workflows/release.yml`** — Remove vm/vm-hyperv/cdn jobs
7. **Update `Justfile`** — Remove build-vm targets, update install-tools
8. **Delete `packer/` directory** — All contents
9. **Test on Windows** — `polis start` with Hyper-V backend
10. **Test on Linux** — `polis start` with QEMU backend
11. **Test on macOS** — `polis start` with QEMU backend (if applicable)

## Tradeoffs Summary

| Aspect | Baked Image (Before) | Cloud-Init (After) |
|---|---|---|
| First launch time | ~30s | ~5-10 min |
| Subsequent start | ~5s | ~5s |
| Release artifact size | ~7 GB (qcow2 + VHDX) | ~5 MB (CLI only) |
| CI build time | ~30 min | ~10 min |
| Infrastructure cost | S3 + CloudFront CDN | None (GHCR is free for public repos) |
| Reproducibility | High (baked) | Medium (version-pinned, network-dependent) |
| Internet required | At download time | At first launch |
| Cross-platform | Separate builds per hypervisor | Single cloud-init, Multipass handles hypervisor |
| Maintenance | 10+ shell scripts + Packer HCL + goss tests | 1 cloud-init YAML + CLI code |
