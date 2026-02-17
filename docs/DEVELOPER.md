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
| Docker | Yes | `sudo apt install docker.io` |
| just | Yes | `curl -sSf https://just.systems/install.sh \| bash` |
| shellcheck | For linting | `sudo apt install shellcheck` |
| Multipass | For dev VM | `sudo snap install multipass` |
| Packer | For VM builds | `brew install packer` or download from hashicorp.com |

## Project Structure

```
polis/
├── cli/polis.sh              # Main CLI script
├── tools/dev-vm.sh           # Development VM management
├── scripts/install.sh        # One-line installer
├── packer/                   # VM image build
│   ├── polis-vm.pkr.hcl      # Packer template
│   └── scripts/              # Provisioner scripts
├── services/                 # Docker service definitions
├── tests/                    # BATS test suites
│   ├── unit/                 # Unit tests (no Docker)
│   ├── integration/          # Integration tests (requires containers)
│   └── e2e/                  # End-to-end tests (full stack)
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
| `just clean` | Remove build artifacts, stop containers |
| `just clean-all` | ⚠️ Also removes certs, secrets, .env |

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
| Integration | `tests/integration/` | Running containers | Medium (<3min) |
| E2E | `tests/e2e/` | Full stack + network | Slow (<10min) |

### Running Tests

```bash
# All unit tests
just test-bats

# Specific test file
./tests/bats/bats-core/bin/bats tests/unit/scripts/dev-vm-validation.bats

# Integration tests (start containers first)
just up
just test-integration

# E2E tests
just up
just test-e2e

# Run tests with verbose output
./tests/run-tests.sh --verbose unit
```

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
| `ci.yml` | Push/PR | Lint, test, security scan |
| `release.yml` | Tag `v*` | Build images, create release |
| `build-vm.yml` | Tag `v*` | Build VM image |

### Release Process

1. Create and push a tag:
   ```bash
   git tag v0.3.0
   git push origin v0.3.0
   ```

2. CI automatically:
   - Builds and pushes Docker images to GHCR
   - Signs images with cosign
   - Generates SBOMs
   - Creates SLSA attestations
   - Builds VM image (if `build-vm.yml` runs)
   - Creates GitHub Release with artifacts

### Artifacts in Release

| Artifact | Description |
|----------|-------------|
| `polis.sh` | CLI script |
| `polis.sh.sha256` | CLI checksum |
| `polis-core-vX.X.X.tar.gz` | Source tarball |
| `sbom-*.spdx.json` | Software Bill of Materials |
| `polis-vm-vX.X.X-amd64.qcow2` | VM image (from build-vm) |
| `checksums.sha256` | VM checksums |

### Verifying Artifacts

```bash
# Verify CLI attestation
gh attestation verify polis.sh --owner OdraLabsHQ

# Verify VM image attestation
gh attestation verify polis-vm-v0.3.0-amd64.qcow2 --owner OdraLabsHQ

# Verify container image
gh attestation verify oci://ghcr.io/odralabshq/polis-gate-oss:v0.3.0 --owner OdraLabsHQ
```

---

## Building VM Images

### Local Build

```bash
# Build Docker images first
just build

# Build VM image
just build-vm
```

This runs:
1. `docker compose build` - Build all service images
2. `docker save` - Export images to `.build/polis-images.tar`
3. `packer build` - Create VM with Docker, Sysbox, and pre-loaded images

Output: `output/polis-vm-<version>-amd64.qcow2`

### Packer Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `polis_version` | `dev` | Version tag for VM name |
| `sysbox_version` | `0.6.7` | Sysbox version to install |
| `arch` | `amd64` | Target architecture |
| `ubuntu_serial` | `20250115` | Ubuntu cloud image serial |

---

## Installation

### One-Line Install

```bash
curl -fsSL https://raw.githubusercontent.com/OdraLabsHQ/polis/main/scripts/install.sh | bash
```

### What the Installer Does

1. Checks for Multipass
2. Downloads `polis.sh` from GitHub Releases
3. Verifies SHA256 checksum
4. Optionally verifies GitHub attestation (if `gh` CLI available)
5. Creates symlink at `~/.local/bin/polis`

### Manual Install

```bash
# Download specific version
VERSION=v0.3.0
curl -fsSL "https://github.com/OdraLabsHQ/polis/releases/download/${VERSION}/polis.sh" -o ~/.local/bin/polis
chmod +x ~/.local/bin/polis
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
# macOS
brew install multipass

# Linux
sudo snap install multipass
```

**Shellcheck not found (just lint-shell):**
```bash
sudo apt install shellcheck
```

**Packer plugin missing:**
```bash
cd packer && packer init .
```

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
just clean

# Also remove certs, secrets, .env (full reset)
just clean-all

# Rebuild from scratch
just setup && just build && just up
```
