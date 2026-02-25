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

### Unified Init Image (`polis-init`)

The current compose file uses three init containers:

| Container | Image | Purpose |
|---|---|---|
| `host-init` | `dhi.io/alpine-base:3.23-dev` (built) | Host iptables + bridge setup |
| `scanner-init` | `dhi.io/alpine-base:3.23-dev` (pulled) | `chown` ClamAV volume |
| `state-init` | `dhi.io/alpine-base:3.23-dev` (pulled) | `chown` Valkey volume |

`scanner-init` and `state-init` pull directly from `dhi.io` at runtime, which requires DHI authentication. This breaks the cloud-init migration where users pull all images from public GHCR.

The fix: consolidate all three into a single `polis-init` image built in CI and pushed to GHCR. Rename `services/host-init` to `services/init`.

```dockerfile
# services/init/Dockerfile
FROM dhi.io/alpine-base:3.23-dev@sha256:2b318097...
RUN apk add --no-cache docker-cli iptables
```

The Dockerfile is identical to the current `host-init` — DHI alpine base + `docker-cli` + `iptables`. The `docker-cli` and `iptables` packages add ~15 MB, negligible for an init container.

In `docker-compose.yml`, all three init containers use the same built image with different commands:

```yaml
  host-init:
    image: ghcr.io/odralabshq/polis-init-oss:${POLIS_INIT_VERSION:-latest}
    build:
      context: .
      dockerfile: services/init/Dockerfile
    command: ["sh", "-c", "INT_BR=\"br-$$(docker network inspect ... )\" ..."]
    # ... existing host-init config (NET_ADMIN, host network, etc.)

  scanner-init:
    image: ghcr.io/odralabshq/polis-init-oss:${POLIS_INIT_VERSION:-latest}
    command: chown -R 65532:65532 /var/lib/clamav
    # ... existing scanner-init config (CHOWN cap, read_only, etc.)

  state-init:
    image: ghcr.io/odralabshq/polis-init-oss:${POLIS_INIT_VERSION:-latest}
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
    lock_passwd: false
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

runcmd:
  # Install yq
  - wget -qO /usr/local/bin/yq https://github.com/mikefarah/yq/releases/download/v4.44.6/yq_linux_amd64
  - chmod +x /usr/local/bin/yq

  # Install Docker CE from official apt repo (GPG fingerprint verified)
  - |
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
    apt-get update
    apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
    usermod -aG docker ubuntu

  # Install Sysbox (SHA256 verified)
  - |
    SYSBOX_VERSION="0.6.7"
    SYSBOX_SHA256="b7ac389e5a19592cadf16e0ca30e40919516128f6e1b7f99e1cb4ff64554172e"
    DEB="sysbox-ce_${SYSBOX_VERSION}.linux_amd64.deb"
    curl -fsSL -o "/tmp/${DEB}" "https://github.com/nestybox/sysbox/releases/download/v${SYSBOX_VERSION}/${DEB}"
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

  # Create polis directory
  - mkdir -p /opt/polis
  - chown ubuntu:ubuntu /opt/polis

final_message: "Polis VM provisioned in $UPTIME seconds"
```

### Key Design Decisions

1. **Docker CE via `runcmd` (not `apt:` module)** — Docker CE is installed via a shell block in `runcmd` rather than cloud-init's `apt:` module. This mirrors the working `install-docker.sh` from the Packer build exactly. The GPG fingerprint `9DC858229FC7DD38854AE2D88D81803C0EBFCD88` is verified before the repo is added.

2. **Docker CE, not `docker.io`** — Ubuntu's `docker.io` package ships runc 1.3.x which conflicts with Sysbox's hardened `/proc` (`unsafe procfs detected: openat2 fsmount`). Docker CE ships its own runc that works correctly with Sysbox. This is the same package used in the Packer build.

3. **Sysbox via `runcmd`** — No apt repo exists for Sysbox; it's distributed as a .deb on GitHub. The SHA256 verification matches the current `install-sysbox.sh` approach.

4. **Clean Docker restart after Sysbox install** — Sysbox's postinst script stops Docker. The restart sequence explicitly stops `docker.socket` (not just `docker.service`), stops all Sysbox services, wipes stale `/var/run/docker/netns/*` files, then starts Sysbox and Docker in the correct order. Skipping any of these steps causes bind-mount failures when containers start.

5. **`live-restore: false` (omitted from daemon.json)** — `live-restore: true` causes Docker to attempt to re-attach to existing network namespaces on restart, which fails with Sysbox's netns management. It is intentionally omitted.

6. **`package_upgrade: false`** — Skipping full upgrade saves 2-3 minutes. Security updates come from the base image (Multipass downloads the latest 24.04.x point release).

7. **Hardening via `write_files:`** — Sysctl, audit rules, and Docker daemon config are written as static files before `runcmd:` executes. This is more reliable than heredocs in shell scripts.

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
| `services/host-init/` | Replaced by `services/init/` (unified init image) |

The entire `packer/` directory can be deleted.

## Files to Modify

| Path | Change |
|---|---|
| `cloud-init.yaml` | Production config (done — see Cloud-Init Design above) |
| `docker-compose.yml` | Replace `dhi.io/alpine-base` in `scanner-init` and `state-init` with `ghcr.io/odralabshq/polis-init-oss`; update `host-init` image reference |
| `cli/src/multipass.rs` | Add `cloud_init` param to `launch()` |
| `cli/src/workspace/vm.rs` | Use cloud-init path instead of image path |
| `cli/src/workspace/image.rs` | Replace with cloud-init embed + write helper |
| `cli/src/commands/start.rs` | Remove image download, add config transfer + image pull |
| `.github/workflows/release.yml` | Remove vm/vm-hyperv/cdn jobs, simplify release |
| `Justfile` | Remove `build-vm`, `build-vm-hyperv`, related internal targets |
| `scripts/install-tools-windows.ps1` | Remove Packer, QEMU dependencies |

## New Files

| Path | Purpose |
|---|---|
| `services/init/Dockerfile` | Unified init image: DHI alpine-base + docker-cli + iptables |

## Reproducibility Strategy

| Concern | Mitigation |
|---|---|
| Docker CE version drift | GPG fingerprint verified; latest stable from official repo |
| Sysbox version drift | Pin version + SHA256 checksum in cloud-init |
| Ubuntu base image drift | `multipass launch 24.04` tracks 24.04.x point releases (not rolling) |
| Docker GPG key rotation | Verify by fingerprint `9DC858229FC7DD38854AE2D88D81803C0EBFCD88` |
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

## Rollback Plan

If the Multipass + cloud-init approach has critical issues in production:

1. **Short-term**: The Packer pipeline exists in git history. Revert the deletion commit to restore `packer/` directory.
2. **Medium-term**: The `--image` flag can be re-added to `polis start` to support pre-built images alongside cloud-init.
3. **Long-term**: If cloud-init proves unreliable, consider Multipass custom images built with Packer (Multipass supports `file://` URLs to local images).

The Docker images on GHCR are useful regardless of approach — they can be pulled at launch time OR baked into a VM image.

## Migration Steps (Implementation Order)

1. ✅ **Rewrite `cloud-init.yaml`** — Production config with Docker CE, Sysbox, hardening
2. ✅ **POC scripts** — `scripts/poc-cloud-init.ps1` and `scripts/poc-cloud-init.sh` verified on Windows and Linux
3. **Consolidate init image** — Create `services/init/Dockerfile`, update `docker-compose.yml` to use `ghcr.io/odralabshq/polis-init-oss` for `host-init`, `scanner-init`, and `state-init`; delete `services/host-init/`
4. **Update `cli/src/multipass.rs`** — Add `cloud_init` parameter to `launch()` trait and implementation
5. **Update `cli/src/workspace/vm.rs`** — Use cloud-init path, update `create()` signature
6. **Rewrite `cli/src/workspace/image.rs`** — Replace download logic with `include_str!` embed
7. **Update `cli/src/commands/start.rs`** — New flow: cloud-init → transfer config → pull images → start
8. **Update `.github/workflows/release.yml`** — Remove vm/vm-hyperv/cdn jobs
9. **Update `Justfile`** — Remove build-vm targets, update install-tools
10. **Delete `packer/` directory** — All contents
11. **Test on Windows** — `polis start` with Hyper-V backend
12. **Test on Linux** — `polis start` with QEMU backend
13. **Test on macOS** — `polis start` with QEMU backend (if applicable)

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
