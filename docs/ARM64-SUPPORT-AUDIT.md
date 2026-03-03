# ARM64 Support Audit — Polis

**Date:** 2026-03-03
**Status:** Research complete, not yet implemented

---

## 1. External Dependency ARM64 Readiness

| Component | Linux ARM64 | macOS ARM64 | Windows ARM64 | Notes |
|-----------|:-----------:|:-----------:|:-------------:|-------|
| Sysbox v0.6.7 | ✅ | N/A (Linux-only) | N/A (Linux-only) | Ships `arm64.deb` since v0.5.0. **The CLI error message claiming "Sysbox does not support arm64" is incorrect.** |
| Multipass | ✅ (QEMU driver) | ✅ (Apple Silicon) | ❌ No ARM build | No Windows ARM64 installer exists |
| g3proxy (Rust) | ✅ Compiles | ✅ Compiles | N/A | Rust Tier 1 target for `aarch64-unknown-linux-gnu` and `aarch64-apple-darwin` |
| ClamAV | ✅ | N/A | N/A | Official Docker images are multi-arch (amd64/arm64/ppc64le) |
| c-icap | ✅ | N/A | N/A | Pure C, builds from source, no arch-specific code |
| CoreDNS | ✅ | N/A | N/A | Go static binary, `CGO_ENABLED=0`, fully portable |
| Valkey | ✅ | N/A | N/A | Upstream supports ARM64 |
| Docker/Compose | ✅ | ✅ | ✅ (via WSL2) | Full ARM64 support |

**Bottom line:** All dependencies support Linux ARM64. macOS ARM64 works via Multipass (VM-based). Windows ARM64 is blocked by Multipass.

---

## 2. Codebase Blockers (must fix)

### A. Incorrect architecture gate in CLI

**File:** `cli/src/domain/workspace.rs:38-47`

```rust
pub fn check_architecture() -> Result<()> {
    if std::env::consts::ARCH == "aarch64" {
        anyhow::bail!(
            "Polis requires an amd64 host. \
Sysbox (the container runtime used by Polis) does not support arm64 as of v0.6.7. \
Please use an amd64 machine."
        );
    }
    Ok(())
}
```

This is factually wrong — Sysbox has shipped ARM64 `.deb` packages since v0.5.0 (March 2022). Remove or gate this behind a feature flag.

### B. Install script blocks ARM64

**File:** `scripts/install.sh:100-102`

```bash
if [[ "${arch}" == "arm64" ]]; then
    log_error "ARM64 polis workspace images are not yet available."
    echo "  Supported: x86_64 (amd64) only."
    exit 1
fi
```

### C. Hardcoded x86_64 library paths in Sentinel Dockerfile

**File:** `services/sentinel/Dockerfile:96-101`

```dockerfile
RUN mkdir -p /runtime-libs/lib/x86_64-linux-gnu /runtime-libs/usr/lib/x86_64-linux-gnu && \
    cp -L /lib/x86_64-linux-gnu/libatomic.so.1 \
          /lib/x86_64-linux-gnu/libhiredis.so.1.1.0 \
          /lib/x86_64-linux-gnu/libhiredis_ssl.so.1.1.0 \
          /runtime-libs/lib/x86_64-linux-gnu/ && \
    cp -L /usr/lib/x86_64-linux-gnu/libdb-5.3.so \
          /runtime-libs/usr/lib/x86_64-linux-gnu/
```

Needs to use `$(dpkg-architecture -qDEB_HOST_MULTIARCH)` or `TARGETARCH` build arg to resolve the correct lib path (`aarch64-linux-gnu` on ARM64).

### D. Hardcoded LD_LIBRARY_PATH in docker-compose.yml

**File:** `docker-compose.yml:212`

```yaml
- LD_LIBRARY_PATH=/usr/local/lib:/usr/lib/x86_64-linux-gnu:/lib/x86_64-linux-gnu:/usr/lib
```

Needs to be dynamically set or use a generic path.

### E. CI Sysbox install is amd64-only

**File:** `scripts/ci-helpers.sh:11`

```bash
"https://github.com/nestybox/sysbox/releases/download/v${version}/sysbox-ce_${version}.linux_amd64.deb"
```

Needs arch detection. The ARM64 SHA256 for v0.6.7 is `16d80123ba53058cf90f5a68686e297621ea97942602682e34b3352783908f91`.

### F. Windows install script blocks non-AMD64

**File:** `scripts/install.ps1:166-168`

```powershell
if ($env:PROCESSOR_ARCHITECTURE -ne "AMD64") {
    Write-Host "  Polis currently requires x86_64 (AMD64)."
```

---

## 3. DHI Base Images (unknown risk)

All Dockerfiles use `dhi.io/*` images pinned by SHA256 digest:

- `dhi.io/debian-base:trixie@sha256:...`
- `dhi.io/debian-base:trixie-dev@sha256:...`
- `dhi.io/rust:1-dev@sha256:...`
- `dhi.io/golang:1-dev@sha256:...`
- `dhi.io/static@sha256:...`
- `dhi.io/alpine-base:3.23-dev@sha256:...`
- `dhi.io/clamav:1.5@sha256:...`
- `dhi.io/valkey:8.1@sha256:...`

These are from a private registry. **You need to verify whether these digests point to multi-arch manifests or single-arch (amd64) images.** If single-arch, you'll need ARM64 variants or multi-arch manifest lists for each.

---

## 4. Release Pipeline Changes Required

### Current state

The release workflow (`release.yml`) only builds for two targets:

```yaml
matrix:
  include:
    - os: ubuntu-24.04
      target: x86_64-unknown-linux-gnu
      artifact: polis-linux-amd64
    - os: windows-2022
      target: x86_64-pc-windows-gnu
      artifact: polis-windows-amd64
```

### What needs to change

| Item | Current | Required |
|------|---------|----------|
| CLI build matrix | 2 targets (linux-amd64, windows-amd64) | Add `aarch64-unknown-linux-gnu` (linux-arm64), `aarch64-apple-darwin` (darwin-arm64) |
| Docker image builds | Single-arch via `docker buildx bake --load` | Multi-arch via `docker buildx bake --push` with `--platform linux/amd64,linux/arm64` |
| g3-builder workflow | Builds on `ubuntu-24.04` only | Add QEMU + buildx multi-platform, or use ARM64 runner |
| CI runners | `ubuntu-24.04` (amd64) | Add `ubuntu-24.04-arm` or use QEMU emulation for ARM64 tests |
| QEMU setup | Not present | Add `docker/setup-qemu-action` before buildx in CI and release |
| Release assets | `polis-linux-amd64`, `polis-windows-amd64` | Add `polis-linux-arm64`, `polis-darwin-arm64` |
| Image digests | Single-arch digests | Multi-arch manifest digests |
| Install script | Blocks ARM64 | Download correct binary per arch |

> **Note:** The `get_asset_name()` function in `cli/src/infra/update.rs` already maps `("linux", "aarch64")` → `polis-linux-arm64.tar.gz` and `("macos", "aarch64")` → `polis-darwin-arm64.tar.gz`. The self-update path is partially ready.

---

## 5. Platform Feasibility Summary

| Platform | Feasibility | Blocking Issues |
|----------|-------------|-----------------|
| **Linux ARM64** | ✅ Fully feasible | Remove arch gate, fix Dockerfiles, multi-arch CI builds, verify DHI images |
| **macOS ARM64** | ✅ Feasible (via Multipass VM) | Build darwin-arm64 CLI, Multipass runs Linux ARM64 VM with Sysbox inside |
| **Windows ARM64** | ❌ Not feasible today | Multipass has no Windows ARM64 build. Would need alternative VM backend (WSL2?) which is a significant architecture change |

---

## 6. Recommended Execution Order

1. **Verify DHI base images** support multi-arch (or produce ARM64 variants) — this is the unknown gatekeeper
2. **Fix Sentinel Dockerfile** — replace hardcoded `x86_64-linux-gnu` paths with arch-aware logic
3. **Fix docker-compose.yml** — remove hardcoded x86_64 `LD_LIBRARY_PATH`
4. **Build multi-arch g3-builder** — add QEMU + buildx multi-platform to `g3-builder.yml`
5. **Add multi-arch to release pipeline** — QEMU setup, platform flags, ARM64 CLI targets
6. **Remove architecture gates** — `check_architecture()` in Rust, `install.sh`, `install.ps1`
7. **Update CI helpers** — arch-aware Sysbox download in `ci-helpers.sh`
8. **Add ARM64 CI testing** — either ARM64 runners or QEMU-based integration tests
9. **Build and publish macOS CLI** — add `aarch64-apple-darwin` to release matrix (needs `macos-14` runner)

---

## 7. Files That Need Changes

| File | Change |
|------|--------|
| `cli/src/domain/workspace.rs` | Remove/update `check_architecture()` |
| `cli/src/commands/start.rs` | Remove call to `check_architecture` if gated there |
| `services/sentinel/Dockerfile` | Replace hardcoded `x86_64-linux-gnu` with arch-aware paths |
| `docker-compose.yml` | Fix hardcoded `LD_LIBRARY_PATH` for sentinel |
| `scripts/install.sh` | Remove ARM64 block, download correct binary per arch |
| `scripts/install.ps1` | Update architecture check |
| `scripts/ci-helpers.sh` | Arch-aware Sysbox download + SHA256 |
| `.github/workflows/ci.yml` | Add QEMU setup, consider ARM64 test jobs |
| `.github/workflows/release.yml` | Add ARM64 CLI targets, multi-arch Docker builds |
| `.github/workflows/g3-builder.yml` | Add multi-platform build |
| All `services/*/Dockerfile` | Verify DHI base image multi-arch support |
