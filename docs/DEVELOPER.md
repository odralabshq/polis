# Polis Developer Guide

This guide covers the development workflow, tools, and CI/CD for Polis.

## Quick Start

```bash
# 1. Setup (generate certs and secrets)
just setup

# 2. Build all Docker images
just build

# 3. Start services
just up

# 4. Run tests
just test-bats
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
| `just build` | Build all Docker images |
| `just build-service <name>` | Build a specific service |
| `just build-vm` | Build VM image via Packer |

### Lifecycle

| Recipe | Description |
|--------|-------------|
| `just up` | Start all services |
| `just down` | Stop all services |
| `just status` | Show service status |
| `just logs [service]` | View logs (optionally filter by service) |

### Testing

| Recipe | Description |
|--------|-------------|
| `just test` | Run all tests (Rust, C, BATS unit) |
| `just test-all` | Run all test tiers (unit + integration + e2e) |
| `just test-clean` | Full clean → build → setup → up → test-all (stops on first failure) |
| `just test-rust` | Run Rust tests |
| `just test-c` | Run C tests |
| `just test-bats` | Run BATS unit tests |
| `just test-integration` | Run integration tests (requires running containers) |
| `just test-e2e` | Run E2E tests (requires full stack) |

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

## CI/CD Pipelines

### Workflows

| Workflow | Trigger | Purpose |
|----------|---------|---------|
| `ci.yml` | Push/PR to main/develop | Lint, build, test, security scan |
| `release.yml` | Tag `v*` or manual | Build images, VM, CLI → GitHub Release |
| `release-vm.yml` | Manual | Build VM image only (standalone) |
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
validate → docker (build + push to GHCR)
                ↓
              vm (Packer build with Goss tests)
                ↓
              cli (Rust binary build)
                ↓
            release (GitHub Release with all artifacts)
```

### Release Artifacts

| Artifact | Description |
|----------|-------------|
| `polis-workspace-vX.X.X-amd64.qcow2` | VM image with Docker + Sysbox + Polis |
| `checksums.sha256` | SHA256 checksums for VM |
| `polis-linux-amd64` | CLI binary |
| `polis-linux-amd64.sha256` | CLI checksum |
| `install.sh` | Installation script |

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
   - Builds and pushes Docker images to GHCR
   - Builds VM image with Goss validation
   - Builds CLI binary
   - Creates GitHub Release with attestations

### Verifying Artifacts

```bash
# Verify VM image provenance
gh attestation verify polis-workspace-v0.3.0-amd64.qcow2 --owner OdraLabsHQ

# Verify CLI binary provenance
gh attestation verify polis-linux-amd64 --owner OdraLabsHQ
```

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
# Build Docker images first
just build

# Build VM image (amd64, default)
just build-vm

# Build for arm64
just build-vm arch=arm64

# Debug: open QEMU console to watch boot progress
just build-vm headless=false
```

This runs:
1. `docker compose build` - Build all service images
2. `docker save` - Export images to `.build/polis-images.tar`
3. `packer build` - Create VM with Docker, Sysbox, and pre-loaded images

Output: `output/polis-vm-<version>-amd64.qcow2`

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

## Polis CLI

The user-facing CLI is built in Rust under `cli/src/`. To build and install locally:

```bash
cd cli && cargo build --release
cp target/release/polis ~/.local/bin/
```

Or use the pre-built binary from GitHub releases.

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
