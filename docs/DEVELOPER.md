# Polis Developer Guide

This guide covers the development workflow, tools, and CI/CD for building Polis from source.

> For user installation (pre-built binaries), see the [README](../README.md).

## Quick Start

```bash
# 1. Setup (generate certs and secrets)
just setup

# 2. Build CLI + Docker images + VM image
just build

# 3. Install dev build
bash scripts/install-dev.sh

# 4. Run workspace
polis run
```

## Prerequisites

| Tool | Required | Install |
|------|----------|---------|
| just | Yes | `curl -sSf https://just.systems/install.sh \| bash` |
| Docker | Yes | `just install-tools` |
| shellcheck | For linting | `just install-tools` |
| hadolint | For Dockerfile linting | `just install-tools` |
| container-structure-test | For image validation | `just install-tools` |
| Multipass | For dev VM | `just install-tools` |
| Packer | For VM builds | `just install-tools` (adds HashiCorp apt repo) |
| QEMU + xorriso | For VM builds | `just install-tools` |

## Project Structure

```
polis/
├── cli/src/                  # Rust CLI (polis binary)
├── tools/dev-vm.sh           # Development VM management
├── cloud-init.yaml           # Cloud-init config for dev VMs
├── packer/                   # VM image build
│   ├── polis-vm.pkr.hcl      # Packer template
│   ├── goss/                 # Goss tests for VM validation
│   └── scripts/              # Provisioner scripts
├── services/                 # Docker service definitions
├── tests/                    # Test suites
│   ├── unit/                 # Unit tests (no Docker)
│   │   ├── packer/           # Packer config validation
│   │   └── docker/           # Dockerfile/Compose linting
│   ├── integration/          # Integration tests (requires containers)
│   ├── e2e/                  # End-to-end tests (full stack)
│   ├── container-structure/  # Container structure test configs
│   └── lib/                  # Test helpers and constants
├── Justfile                  # Task runner recipes
└── .github/workflows/        # CI/CD pipelines
```

---

## Justfile Recipes

Run `just --list` to see all available recipes.

### Setup & Build

| Recipe | Description |
|--------|-------------|
| `just setup` | Generate all certificates and secrets |
| `just setup-ca` | Generate CA certificate only |
| `just setup-valkey` | Generate Valkey certs and secrets |
| `just setup-toolbox` | Generate Toolbox certificates |
| `just build` | Build CLI + Docker images + VM image |
| `just build-cli` | Build the Rust CLI binary |
| `just build-docker` | Build all Docker images |
| `just build-vm` | Build VM image via Packer (sign included) |
| `just build-service <name>` | Build a specific Docker service |

### Lifecycle

| Recipe | Description |
|--------|-------------|
| `just up` | Start control plane only (no agent) |
| `just down` | Stop all services |
| `just status` | Show service status |
| `just logs [service]` | View logs (optionally filter by service) |

### Testing

| Recipe | Description |
|--------|-------------|
| `just test` | Run Rust + C + BATS unit tests |
| `just test-all` | Run all tiers (unit + integration + e2e) |
| `just test-clean` | Full clean → build-docker → setup → up → test-all (stops on first failure) |
| `just test-rust` | Run all Rust tests (cli + toolbox + polis-common) |
| `just test-c` | Run C unit tests (sentinel modules) |
| `just test-unit` | Run BATS unit tests (~30s, no Docker) |
| `just test-cli [filter]` | Run CLI BATS spec tests (requires multipass + built VM + installed polis) |
| `just test-integration` | Run integration tests (~3min, requires running containers) |
| `just test-e2e` | Run E2E tests (~10min, requires full stack) |

### Linting

| Recipe | Description |
|--------|-------------|
| `just lint` | Run all linters |
| `just lint-rust` | Run cargo fmt + clippy |
| `just lint-c` | Run cppcheck |
| `just lint-shell` | Run shellcheck |
| `just fmt` | Auto-format Rust code |

### Cleanup

| Recipe | Description |
|--------|-------------|
| `just down` | Stop containers, remove volumes + orphans |
| `just clean` | `down` + `docker system prune -af --volumes` (wipes all images, cache, networks) |
| `just clean-all` | ⚠️ `clean` + removes `certs/`, `secrets/`, `.env` |

---

## Development VM

For development on macOS or Windows, use the Multipass-based dev VM.

### Commands

```bash
# Create a new dev VM
./tools/dev-vm.sh create

# Enter the VM shell
./tools/dev-vm.sh shell

# Get SSH config for VS Code Remote
./tools/dev-vm.sh ssh-config

# Stop/start/delete
./tools/dev-vm.sh stop
./tools/dev-vm.sh start
./tools/dev-vm.sh delete

# Rebuild Polis inside VM
./tools/dev-vm.sh rebuild

# Fix file permissions after Docker builds
./tools/dev-vm.sh fix-perms
```

### Configuration

Environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `POLIS_VM_NAME` | `polis-dev` | VM name |
| `POLIS_VM_CPUS` | `4` | CPU count |
| `POLIS_VM_MEMORY` | `8G` | Memory size |
| `POLIS_VM_DISK` | `50G` | Disk size |

Or use CLI flags: `./tools/dev-vm.sh create --cpus=8 --memory=16G`

### VS Code Remote SSH

1. Get SSH config: `./tools/dev-vm.sh ssh-config >> ~/.ssh/config`
2. In VS Code: `Cmd+Shift+P` → "Remote-SSH: Connect to Host" → select `polis-dev`
3. Open folder: `/home/ubuntu/polis`

---

## Testing

### Test Tiers

| Tier | Directory | Dependencies | Speed |
|------|-----------|--------------|-------|
| Unit | `tests/unit/` | None | Fast (<30s) |
| Packer | `tests/unit/packer/` | Packer CLI | Fast (<10s) |
| Docker | `tests/unit/docker/` | Docker CLI | Fast (<10s) |
| Integration | `tests/integration/` | Running containers | Medium (<3min) |
| E2E | `tests/e2e/` | Full stack + network | Slow (<10min) |

### Running Tests

```bash
# All unit tests (includes packer, docker)
just test-bats

# Specific test tiers
./tests/run-tests.sh unit          # Unit tests only
./tests/run-tests.sh packer        # Packer config validation
./tests/run-tests.sh docker        # Dockerfile/Compose linting
./tests/run-tests.sh integration   # Integration tests
./tests/run-tests.sh e2e           # E2E tests

# Integration tests (start containers first)
just up
just test-integration

# E2E tests
just up
just test-e2e

# Run tests with verbose output
./tests/run-tests.sh --verbose unit
```

### Test Tools

| Tool | Purpose | Install |
|------|---------|---------|
| BATS | Test framework | Bundled in `tests/bats/` |
| Hadolint | Dockerfile linter | `brew install hadolint` or Docker |
| container-structure-test | Image validation | [GitHub releases](https://github.com/GoogleContainerTools/container-structure-test) |
| Goss | VM image testing | Bundled in Packer build |

### Packer Tests (`tests/unit/packer/`)

Validates Packer configuration without building:

```bash
./tests/run-tests.sh packer
```

Tests include:
- `packer validate` syntax check
- `packer fmt` formatting check
- Shellcheck on provisioner scripts
- Security patterns (SHA256, GPG verification, hardening)

### Docker Tests (`tests/unit/docker/`)

Validates Dockerfiles and docker-compose.yml:

```bash
./tests/run-tests.sh docker
```

Tests include:
- Hadolint Dockerfile linting (if installed)
- `docker compose config` validation
- Security constraints (cap_drop, read_only, no-new-privileges)
- Static IP consistency with `tests/lib/constants.bash`
- No secrets in environment variables

### Container Structure Tests (`tests/container-structure/`)

Validates built Docker images have correct structure:

```bash
# Requires container-structure-test CLI and built images
container-structure-test test --image polis-gate-oss:latest --config tests/container-structure/gate.yaml
```

Or run via BATS wrapper (part of integration tests):
```bash
./tests/run-tests.sh integration
```

Tests validate:
- Required binaries exist (g3proxy, c-icap, coredns, etc.)
- Correct user (65532/nonroot)
- Config files in place
- Commands execute successfully

### Goss Tests (VM Image)

Goss tests run automatically during `packer build` to validate the VM image before finalization:

```
packer/goss/
├── goss.yaml           # Main entry point
├── goss-docker.yaml    # Docker installation
├── goss-sysbox.yaml    # Sysbox runtime
├── goss-hardening.yaml # VM hardening (sysctl, auditd)
└── goss-polis.yaml     # Polis installation
```

If any Goss test fails, the Packer build fails and no image is produced.

### Writing Tests

Unit test example (`tests/unit/scripts/example.bats`):

```bash
#!/usr/bin/env bats
# bats file_tags=unit,scripts

setup() {
    load "../../lib/test_helper.bash"
}

@test "example: validates input" {
    run some_command
    assert_success
    assert_output --partial "expected output"
}
```

Key rules:
- One assertion per test
- Use `run` before commands to capture output
- Unit tests must NOT call Docker
- Integration tests must use `require_container` guard

---

## Branching & Git Flow

Polis uses a **GitLab Flow** variant optimised for mixed human + AI agent development. See [GitLab Flow](https://about.gitlab.com/topics/version-control/what-is-gitlab-flow/) and [GitFlow vs trunk-based](https://pullpanda.io/blog/git-flow-vs-trunk-based-development) for background.

### Branch Model

```
feature/my-change ──┐
agent/task-xyz    ──┤  merge commit (no review required)
fix/some-bug      ──┘
                     ↓
                  develop  ← integration branch, CI gates only
                     │
              squash merge (1 human approval required)
                     ↓
                   main  ← stable, release-ready
                     │
                  git tag vX.X.X
                     ↓
                  release
```

### Rules

| Branch | Who targets it | Review required | Merge type | Direct push |
|--------|---------------|-----------------|------------|-------------|
| `develop` | humans + AI agents | No — CI must pass | Merge commit | ✗ |
| `main` | `develop` only | 1 human approval | Squash merge | ✗ |

- **AI agents target `develop`** — no human review gate, but lint + unit tests must pass
- **`develop → main`** is a deliberate human-promoted release, squash-merged to keep `main` history clean (one commit per release)
- PRs to `main` from any branch other than `develop` are blocked by CI

### Required CI checks on `develop`

`lint-rust` · `lint-c` · `lint-shell` · `test-rust` · `test-c` · `unit-tests`

### Creating a release

```bash
# 1. Open a PR from develop → main, get 1 approval, squash merge
# 2. Tag main
git checkout main && git pull
git tag v0.4.0
git push origin v0.4.0
```

---

## CI/CD Pipelines

### Workflows

| Workflow | Trigger | Purpose |
|----------|---------|---------|
| `ci.yml` | Push/PR to main/develop | Lint, build, test, security scan |
| `release.yml` | Tag `v*` or manual | Build images + VM + CLI → S3/CDN upload → GitHub Release |
| `g3-builder.yml` | Push to main (g3 Dockerfile changes) | Rebuild g3-builder base image |

### CI Pipeline Stages (`ci.yml`)

```
Stage 1 (parallel):
├── lint-rust      → cargo fmt + clippy
└── lint-c         → cppcheck

Stage 2 (parallel, after lint):
├── test-rust      → cargo test (skip proptests)
├── test-rust-proptests → cargo test proptests
└── test-c         → gcc + run native tests

Stage 3 (parallel, after test):
├── build-containers → docker buildx bake
├── unit-tests       → BATS unit tests
└── scan-images      → Snyk container scan (matrix)

Stage 4 (after build):
└── integration-tests → BATS integration tests

Stage 5 (after integration):
└── e2e-tests        → BATS E2E tests

Security (parallel):
├── security-snyk-code → SAST scan
├── security-snyk-iac  → IaC scan
└── security-sonarcloud → SonarCloud analysis
```

### Release Pipeline (`release.yml`)

Triggered by `v*` tag push or manual workflow dispatch.

```
validate → docker (build + tag + push to GHCR + generate versions.json)
                ↓
              vm (bundle config with .env, Packer build with Goss tests)
                ↓
              cli (Rust binary build)
                ↓
              cdn (upload VM image to S3, invalidate CloudFront cache)
                ↓
            release (GitHub Release with all artifacts)
```

### Release Artifacts

| Artifact | Description |
|----------|-------------|
| `polis-vX.X.X-amd64.qcow2` | VM image with Docker + Sysbox + Polis |
| `checksums.sha256` | SHA256 checksums for VM |
| `polis-linux-amd64` | CLI binary (Linux) |
| `polis-linux-amd64.sha256` | CLI checksum (Linux) |
| `polis-windows-amd64.exe` | CLI binary (Windows) |
| `polis-windows-amd64.exe.sha256` | CLI checksum (Windows) |
| `install.sh` | Linux/macOS installation script |
| `install.ps1` | Windows installation script |
| `versions.json` | Signed container version manifest (ed25519) |

VM images are also uploaded to S3 (`polis-releases` bucket) and served via CloudFront CDN for faster downloads. The `install.sh` and `install.ps1` scripts download images from the CDN by default, with automatic fallback to GitHub Releases.

Docker images pushed to GHCR:
- `ghcr.io/odralabshq/polis-gate-oss:vX.X.X`
- `ghcr.io/odralabshq/polis-sentinel-oss:vX.X.X`
- `ghcr.io/odralabshq/polis-resolver-oss:vX.X.X`
- etc.

### Release Process

1. Create and push a tag:
   ```bash
   git tag v0.3.0
   git push origin v0.3.0
   ```

2. Or trigger manually via Actions UI with version input.

3. Pipeline automatically:
   - Validates version format (`vX.X.X` or `vX.X.X-suffix`)
   - Builds and pushes Docker images to GHCR tagged `:vX.X.X`
   - Generates `versions.json` and signs it with the release ed25519 key
   - Builds VM image: bakes images + `.env` (with `POLIS_*_VERSION=vX.X.X`) via Packer
   - Builds CLI binary
   - Creates GitHub Release with attestations

### Container Version Manifest (`versions.json`)

Each release publishes a signed `versions.json` that maps container names to their versions:

```json
{
  "manifest_version": 1,
  "vm_image": { "version": "v0.3.0", "asset": "polis-v0.3.0-amd64.qcow2" },
  "containers": {
    "polis-gate-oss": "v0.3.0",
    "polis-sentinel-oss": "v0.3.0",
    ...
  }
}
```

`polis update` downloads this manifest, verifies the ed25519 signature against the public key compiled into the CLI binary, then updates containers in the VM accordingly.

### Release Signing Key Setup (one-time)

The release signing key is an ed25519 keypair. The private key lives only in GitHub secrets; the public key is compiled into the CLI binary.

**If you need to rotate or set up the key from scratch:**

```bash
cargo install zipsign --version 0.2.1 --locked
zipsign gen-key .secrets/polis-release.key .secrets/polis-release.pub

# Get the base64 public key for update.rs:
base64 -w0 .secrets/polis-release.pub

# Get the base64 private key for the GitHub secret:
base64 -w0 .secrets/polis-release.key
```

1. Update `POLIS_PUBLIC_KEY_B64` in `cli/src/commands/update.rs` with the public key output
2. Add the private key as GitHub secret `POLIS_SIGNING_KEY` (repo → Settings → Secrets → Actions)
3. Keep `.secrets/polis-release.key` backed up securely (password manager). It is gitignored.

### Verifying Artifacts

```bash
# Verify VM image provenance
gh attestation verify polis-v0.3.0-amd64.qcow2 --owner OdraLabsHQ

# Verify CLI binary provenance
gh attestation verify polis-linux-amd64 --owner OdraLabsHQ
```

---

## Testing `polis update` Locally

Any developer can test the full `polis update` manifest flow without the release signing key:

```bash
# Generates a throwaway keypair, signs a local versions.json, serves it over HTTP
./tools/test-update-local.sh v0.3.1
```

This exercises the full path: manifest download → ed25519 signature verification → version comparison. It stops at "Workspace not running" when it tries to update containers, which is expected without a running VM.

The script uses `POLIS_VERIFYING_KEY_B64` env var to override the compiled-in public key at runtime, so no access to the real signing key is needed.

---

## Building VM Images

### Install Build Tools

Packer is not in the default Ubuntu apt repos — it requires the HashiCorp apt repo. Run:

```bash
just install-tools
```

This installs: docker.io, shellcheck, hadolint (v2.12.0), container-structure-test (v1.19.3), multipass, packer (via HashiCorp repo), qemu-system-x86, qemu-utils, ovmf, xorriso.

### Local Build

```bash
# Build CLI binary
just build-cli

# Build Docker images
just build-docker

# Build VM image (amd64, default)
just build-vm

# Build everything
just build

# Build VM for arm64
just build-vm arch=arm64

# Debug: open QEMU console to watch boot progress
just build-vm headless=false
```

This runs:
1. `cargo build --release` - Build the CLI binary
2. `docker compose build` - Build all service images
3. `docker save` - Export images to `.build/polis-images.tar`
4. `packer build` - Create VM with Docker, Sysbox, and pre-loaded images

Output: `packer/output/polis-<version>-amd64.qcow2`

### VM Image Validation (Goss)

The Packer build includes Goss tests that validate the VM before finalizing:

```yaml
# packer/goss/goss.yaml includes:
- goss-docker.yaml    # Docker CE installed, daemon hardened
- goss-sysbox.yaml    # Sysbox runtime available
- goss-hardening.yaml # CIS sysctl values, auditd, AppArmor
- goss-polis.yaml     # Polis files, images loaded, systemd service
```

If any test fails, the build aborts — no broken images are produced.

To run Goss tests manually inside a VM:
```bash
goss -g /path/to/goss.yaml validate
```

### KVM Acceleration

Without KVM the QEMU build runs in software emulation and can take hours. Verify KVM is available:

```bash
ls /dev/kvm
```

**Native Linux**

KVM should work out of the box. If `/dev/kvm` is missing or you get `Qemu failed to start`:

```bash
# Load the module
sudo modprobe kvm_intel   # or kvm_amd on AMD CPUs

# Add your user to the kvm group (required for /dev/kvm access)
sudo usermod -aG kvm $USER
# Log out and back in — newgrp kvm hangs in non-interactive shells
```

Verify before building:
```bash
ls -la /dev/kvm
groups | grep kvm
```

**Inside a VM (nested virtualization)**

If developing inside a VM (e.g. Multipass on Windows), enable nested virtualization on the host first.

Multipass / Hyper-V — PowerShell as Admin on Windows host:
```powershell
multipass stop polis-dev
Set-VMProcessor -VMName "polis-dev" -ExposeVirtualizationExtensions $true
multipass start polis-dev
```

**Disk space requirements**

`polis run` causes multipassd to copy the workspace image (~3.4 GB) into its vault, then expand a 50 GB virtual disk inside the nested VM. The dev VM needs enough free space to hold both. Recommended minimum: **200 GB**.

To check and resize from the host:

```bash
# Find the VM name
multipass list

# Stop, resize, restart
multipass stop <name>
multipass set local.<name>.disk=200G
multipass start <name>
```

The filesystem inside the VM expands automatically on next boot via `growpart`.

VMware Workstation: VM Settings → Processors → enable "Virtualize Intel VT-x/EPT or AMD-V/RVI"

VirtualBox — PowerShell on Windows host:
```powershell
VBoxManage modifyvm "polis-dev" --nested-hw-virt on
```

After enabling, restart the VM and confirm `/dev/kvm` exists. With KVM the full build takes ~10-20 min.

**Monitoring the build**

The VM runs headless. Once QEMU starts, just wait — Packer will print `Connected to SSH!` when cloud-init finishes (2-5 min with KVM), then run the provisioner scripts. Total build time is ~10-20 min.

### Packer Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `polis_version` | `dev` | Version tag for VM name |
| `sysbox_version` | `0.6.7` | Sysbox version to install |
| `arch` | `amd64` | Target architecture (`amd64` or `arm64`) |
| `ubuntu_serial` | `20260128` | Ubuntu cloud image release serial |
| `use_minimal_image` | `true` | Use Ubuntu Minimal image (~248MB vs ~2GB) |
| `headless` | `true` | Run QEMU headless (set `false` to open console for debugging) |

---

## OpenClaw Agent

OpenClaw runs as a systemd service inside the workspace container, exposing a Control UI on port 18789. It installs at first boot (~3-5 min).

### Setup and startup

```bash
echo "OPENAI_API_KEY=sk-proj-..." >> .env   # or ANTHROPIC_API_KEY / OPENROUTER_API_KEY
polis start --agent=openclaw
```

### Checking progress

```bash
just logs workspace                                                        # workspace init
docker exec polis-workspace systemctl status openclaw                     # service status
docker exec polis-workspace journalctl -u openclaw -f                     # gateway log
docker exec polis-workspace cat /home/polis/.openclaw/gateway-token.txt   # get token
```

Open `http://<host>:18789/#token=<token>`. On Multipass use the VM IP; on native Linux use `localhost`.

### Agent management

```bash
polis agent list                    # list installed agents and active status
polis agent restart                 # restart active agent's workspace
polis agent update                  # re-generate artifacts and recreate workspace
polis agent remove openclaw         # remove agent (stops workspace if active)
polis agent add --path ./my-agent   # install a new agent from a local folder
```

### Reset config (new token, re-detect API keys)

```bash
docker exec polis-workspace rm /home/polis/.openclaw/.initialized
docker exec polis-workspace systemctl restart openclaw
```

### Key paths inside the workspace container

| Path | Purpose |
|------|---------|
| `/home/polis/.openclaw/openclaw.json` | Gateway config |
| `/home/polis/.openclaw/gateway-token.txt` | Control UI token |
| `/home/polis/.openclaw/agents/default/agent/auth-profiles.json` | API keys per provider |
| `/run/openclaw-env` | Host `.env` bind-mounted for init script |
| `/var/lib/openclaw-installed` | Idempotency marker |

---

The user-facing CLI is built in Rust under `cli/src/`. To build and install locally:

```bash
just build-cli
cp cli/target/release/polis ~/.local/bin/
```

Or use the pre-built binary from GitHub releases.

---

## Local Install (Dev Build)

`scripts/install-dev.sh` installs Polis from local build artifacts instead of GitHub releases. Use this to test the full install flow without publishing a release.

### Prerequisites

```bash
# 1. Build the CLI
just build-cli

# 2. Build the VM image
just build-vm
```

### Install

```bash
./scripts/install-dev.sh
```

This installs:
- CLI from `cli/target/release/polis` → `~/.polis/bin/polis`
- Symlink at `~/.local/bin/polis`
- VM image from `packer/output/*.qcow2` via `polis init --image file://...`

### Options

```bash
# Override repo path (e.g. when running from outside the repo)
./scripts/install-dev.sh --repo /path/to/polis

# Or via env var
POLIS_REPO=/path/to/polis ./scripts/install-dev.sh

# Override install dir (default: ~/.polis)
POLIS_HOME=/opt/polis ./scripts/install-dev.sh
```

---

## Troubleshooting

### Common Issues

**Docker permission denied:**
```bash
sudo usermod -aG docker $USER
# Log out and back in
```

**Multipass not found (dev-vm.sh):**
```bash
just install-tools
```

**Shellcheck not found (just lint-shell):**
```bash
just install-tools
```

**Packer plugin missing:**
```bash
cd packer && packer init .
```

**`E: Package 'packer' has no installation candidate`:**
Packer isn't in the default Ubuntu repos. Use `just install-tools` — it adds the HashiCorp apt repo automatically.

**`could not find a supported CD ISO creation command`:**
```bash
sudo apt-get install -y xorriso
```
Or just re-run `just install-tools` which includes xorriso.

### Logs

```bash
# All service logs
just logs

# Specific service
just logs gate
just logs sentinel

# Follow logs
docker compose logs -f gate
```

### Reset Everything

```bash
# Stop and remove containers, volumes, networks
just down

# Also wipe all Docker images and build cache
just clean

# Also remove certs, secrets, .env (full reset)
just clean-all

# Full clean-build-test cycle (CI equivalent)
just test-clean
```