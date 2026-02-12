# Polis Repository Reorganization Plan

## Motivation

The current repo layout groups files by type (all configs in `config/`, all Dockerfiles in `build/`, all scripts in `scripts/`). This works at small scale but creates friction as the project grows:

- Adding a new service means touching 5+ directories
- It's hard to tell which config belongs to which service
- Scripts mix container init, health checks, setup utilities, and shared libraries
- C source code lives under `build/` alongside Dockerfiles
- CI/CD can't easily target individual services
- Ownership boundaries are unclear

The new layout groups files by **service function**. Each service owns its Dockerfile, config, scripts, and source code. Cross-cutting concerns live in shared directories.

---

## Target Structure

```text
polis/
├── services/
│   ├── gate/                           # TLS-intercepting traffic entry point
│   │   ├── Dockerfile
│   │   ├── config/
│   │   │   ├── g3proxy.yaml
│   │   │   ├── g3fcgen.yaml
│   │   │   └── seccomp.json
│   │   ├── scripts/
│   │   │   ├── init.sh
│   │   │   └── health.sh
│   │   └── README.md
│   │
│   ├── sentinel/                       # Content inspection (DLP, malware, approval)
│   │   ├── Dockerfile
│   │   ├── modules/                    # C source (c-icap native modules)
│   │   │   ├── dlp/
│   │   │   │   └── srv_polis_dlp.c
│   │   │   └── approval/
│   │   │       ├── srv_polis_approval.c
│   │   │       └── srv_polis_approval_rewrite.c
│   │   ├── config/
│   │   │   ├── c-icap.conf
│   │   │   ├── squidclamav.conf
│   │   │   ├── freshclam.conf
│   │   │   ├── polis_dlp.conf
│   │   │   ├── polis_approval.conf
│   │   │   └── seccomp.json
│   │   └── README.md
│   │
│   ├── workspace/                      # Isolated dev environment
│   │   ├── Dockerfile
│   │   ├── config/
│   │   │   └── polis-init.service
│   │   ├── scripts/
│   │   │   └── init.sh
│   │   └── README.md
│   │
│   ├── toolbox/                        # MCP agent + CLI tools
│   │   ├── Dockerfile
│   │   ├── crates/
│   │   │   ├── mcp-agent/             # MCP server (Axum + Valkey)
│   │   │   │   ├── Cargo.toml
│   │   │   │   └── src/
│   │   │   │       ├── main.rs
│   │   │   │       ├── state.rs
│   │   │   │       └── tools.rs
│   │   │   └── approve-cli/           # HITL approval CLI
│   │   │       ├── Cargo.toml
│   │   │       ├── src/
│   │   │       │   └── main.rs
│   │   │       └── tests/
│   │   │           └── cli_tests.rs
│   │   └── README.md
│   │
│   └── state/                          # Valkey data store
│       ├── config/
│       │   └── valkey.conf
│       ├── scripts/
│       │   ├── health.sh
│       │   ├── generate-certs.sh
│       │   └── generate-secrets.sh
│       └── README.md
│
├── lib/                                # Shared code across services
│   ├── crates/
│   │   └── polis-common/              # Shared Rust types (BlockReason, SecurityLevel, etc.)
│   │       ├── Cargo.toml
│   │       └── src/
│   │           ├── lib.rs
│   │           ├── config.rs
│   │           ├── redis_keys.rs
│   │           └── types.rs
│   └── shell/
│       └── network-helpers.sh          # Shared bash (is_wsl2, disable_ipv6)
│
├── agents/                             # Agent plugin system (unchanged)
│   ├── _template/
│   │   ├── agent.conf
│   │   ├── compose.override.yaml
│   │   ├── install.sh
│   │   ├── config/
│   │   │   └── agent.service
│   │   └── scripts/
│   │       ├── health.sh
│   │       └── init.sh
│   └── openclaw/
│       ├── agent.conf
│       ├── commands.sh
│       ├── compose.override.yaml
│       ├── install.sh
│       ├── config/
│       │   ├── SOUL.md
│       │   ├── env.example
│       │   └── openclaw.service
│       ├── scripts/
│       │   ├── health.sh
│       │   └── init.sh
│       └── README.md
│
├── config/
│   └── polis.yaml                      # Cross-cutting security policy only
│
├── deploy/
│   └── docker-compose.yml
│
├── tests/                              # BATS test suite (unchanged)
│   ├── unit/
│   ├── integration/
│   ├── e2e/
│   ├── helpers/
│   │   └── common.bash
│   ├── run-tests.sh
│   ├── setup_suite.bash
│   └── README.md
│
├── tools/
│   ├── polis.sh                        # Main CLI
│   └── fix-line-endings.sh
│
├── scripts/
│   └── install.sh                      # User-facing installer only
│
├── docs/                               # Project documentation
│
├── .github/
│   └── workflows/
│       ├── ci.yml
│       └── release.yml
│
├── Cargo.toml                          # Workspace root
├── Cargo.lock
├── README.md
├── .gitignore
├── .gitattributes
├── .dockerignore                       # NEW: root-level Docker ignore
├── CODEOWNERS                          # NEW: ownership mapping
├── Justfile                            # NEW: unified task runner
└── .github/dependabot.yml              # NEW: per-directory dependency updates
```

---

## File Migration Map

Every file in the current repo mapped to its new location. Files not listed here are unchanged.

### `build/` → `services/`

| Current Path | New Path |
|---|---|
| `build/g3proxy/Dockerfile` | `services/gate/Dockerfile` |
| `build/icap/Dockerfile` | `services/sentinel/Dockerfile` |
| `build/icap/srv_polis_dlp.c` | `services/sentinel/modules/dlp/srv_polis_dlp.c` |
| `build/icap/srv_polis_approval.c` | `services/sentinel/modules/approval/srv_polis_approval.c` |
| `build/icap/srv_polis_approval_rewrite.c` | `services/sentinel/modules/approval/srv_polis_approval_rewrite.c` |
| `build/icap/test_is_new_domain.c` | `tests/native/sentinel/test_is_new_domain.c` |
| `build/icap/test_is_allowed_domain.c` | `tests/native/sentinel/test_is_allowed_domain.c` |
| `build/icap/test_is_new_domain.exe` | **DELETE** (binary, should not be in VCS) |
| `build/mcp-server/Dockerfile.agent` | `services/toolbox/Dockerfile` |
| `build/workspace/Dockerfile` | `services/workspace/Dockerfile` |

### `config/` → `services/*/config/`

| Current Path | New Path |
|---|---|
| `config/g3proxy.yaml` | `services/gate/config/g3proxy.yaml` |
| `config/g3fcgen.yaml` | `services/gate/config/g3fcgen.yaml` |
| `config/seccomp/gateway.json` | `services/gate/config/seccomp.json` |
| `config/c-icap.conf` | `services/sentinel/config/c-icap.conf` |
| `config/squidclamav.conf` | `services/sentinel/config/squidclamav.conf` |
| `config/freshclam.conf` | `services/sentinel/config/freshclam.conf` |
| `config/polis_dlp.conf` | `services/sentinel/config/polis_dlp.conf` |
| `config/polis_approval.conf` | `services/sentinel/config/polis_approval.conf` |
| `config/seccomp/icap.json` | `services/sentinel/config/seccomp.json` |
| `config/polis-init.service` | `services/workspace/config/polis-init.service` |
| `config/valkey.conf` | `services/state/config/valkey.conf` |
| `config/polis.yaml` | `config/polis.yaml` (stays — cross-cutting) |

### `scripts/` → distributed

| Current Path | New Path |
|---|---|
| `scripts/g3proxy-init.sh` | `services/gate/scripts/init.sh` |
| `scripts/health-check.sh` | `services/gate/scripts/health.sh` |
| `scripts/workspace-init.sh` | `services/workspace/scripts/init.sh` |
| `scripts/valkey-health.sh` | `services/state/scripts/health.sh` |
| `scripts/generate-valkey-certs.sh` | `services/state/scripts/generate-certs.sh` |
| `scripts/generate-valkey-secrets.sh` | `services/state/scripts/generate-secrets.sh` |
| `scripts/network-helpers.sh` | `lib/shell/network-helpers.sh` |
| `scripts/install.sh` | `scripts/install.sh` (stays) |

### `crates/` → `services/toolbox/crates/` and `lib/crates/`

| Current Path | New Path |
|---|---|
| `crates/polis-mcp-agent/` | `services/toolbox/crates/mcp-agent/` |
| `crates/polis-approve-cli/` | `services/toolbox/crates/approve-cli/` |
| `crates/polis-mcp-common/` | `lib/crates/polis-common/` |

### Unchanged

- `agents/` — no changes
- `tests/` — no changes (except adding `tests/native/sentinel/` for C test files)
- `tools/polis.sh` — no changes (but internal paths need updating)
- `tools/fix-line-endings.sh` — no changes
- `deploy/docker-compose.yml` — paths need updating (see below)
- `Cargo.lock` — regenerated automatically

---

## Files That Need Internal Path Updates

### `Cargo.toml` (workspace root)

```toml
[workspace]
members = [
    "services/toolbox/crates/mcp-agent",
    "services/toolbox/crates/approve-cli",
    "lib/crates/polis-common",
]
resolver = "2"

[workspace.dependencies]
serde = { version = "1.0", features = ["derive"] }
tokio = { version = "1.0", features = ["full"] }
anyhow = "1.0"
chrono = { version = "0.4", features = ["serde"] }
thiserror = "1.0"
serde_json = "1.0"
```

### `services/toolbox/crates/mcp-agent/Cargo.toml`

Update the dependency path:

```toml
[dependencies]
polis-common = { path = "../../../lib/crates/polis-common" }
# ... rest unchanged
```

### `services/toolbox/crates/approve-cli/Cargo.toml`

```toml
[dependencies]
polis-common = { path = "../../../lib/crates/polis-common" }
# ... rest unchanged
```

### `deploy/docker-compose.yml`

All volume mounts and dockerfile paths need updating. Example for gate:

```yaml
gate:
  build:
    context: ..
    dockerfile: services/gate/Dockerfile
  volumes:
    - ../services/gate/config/g3proxy.yaml:/etc/g3proxy/g3proxy.yaml:ro
    - ../services/gate/config/g3fcgen.yaml:/etc/g3proxy/g3fcgen.yaml:ro
    - ../services/gate/scripts/init.sh:/init.sh:ro
    - ../certs/ca:/etc/g3proxy/ssl:ro
    - ../services/gate/scripts/health.sh:/scripts/health-check.sh:ro
  security_opt:
    - seccomp=../services/gate/config/seccomp.json
```

Similar updates for sentinel, workspace, state, and toolbox services.

### `tools/polis.sh`

Update all path references. Key changes:

- `COMPOSE_FILE` stays the same (`deploy/docker-compose.yml`)
- Script references change from `scripts/generate-valkey-*.sh` to `services/state/scripts/generate-*.sh`
- Build paths change from `build/*/Dockerfile` to `services/*/Dockerfile`
- Config references change from `config/*` to `services/*/config/*`

### `.github/workflows/ci.yml`

Update script permission and path references:

```yaml
- name: Set script permissions
  run: |
    chmod +x ./tools/polis.sh
    chmod +x ./tools/*.sh
    chmod +x ./tests/run-tests.sh
    chmod +x ./scripts/*.sh
    chmod +x ./services/*/scripts/*.sh
    chmod +x ./agents/openclaw/install.sh
    chmod +x ./agents/openclaw/scripts/*.sh

- name: Setup Valkey TLS certificates
  run: ./services/state/scripts/generate-certs.sh ./certs/valkey

- name: Setup Valkey secrets
  run: |
    touch .env
    ./services/state/scripts/generate-secrets.sh ./secrets .
```

### `.github/workflows/release.yml`

Update the Dockerfile matrix:

```yaml
matrix:
  include:
    - image: gate
      dockerfile: services/gate/Dockerfile
      description: Polis Gate (g3proxy)
    - image: sentinel
      dockerfile: services/sentinel/Dockerfile
      description: Polis Sentinel (c-icap + SquidClamav)
    - image: workspace
      dockerfile: services/workspace/Dockerfile
      description: Polis Workspace (base)
```

Update the source tarball step:

```yaml
- name: Create source tarball
  run: |
    VERSION="${{ needs.validate.outputs.version }}"
    tar czf "polis-core-${VERSION}.tar.gz" \
      --exclude='.git' \
      --exclude='certs/ca/*.key' \
      --exclude='.env' \
      --transform "s,^,polis-core-${VERSION}/," \
      services config deploy docs lib scripts tests tools agents \
      README.md .gitignore Cargo.toml Cargo.lock
```

### Dockerfiles

Each Dockerfile needs path updates for COPY instructions. Example for gate:

```dockerfile
# Was: COPY scripts/g3proxy-init.sh /init.sh
COPY services/gate/scripts/init.sh /init.sh
COPY services/gate/scripts/health.sh /scripts/health-check.sh
COPY lib/shell/network-helpers.sh /scripts/network-helpers.sh
```

### Init scripts (remove fallback duplication)

`services/gate/scripts/init.sh` and `services/workspace/scripts/init.sh` currently contain
fallback re-implementations of `is_wsl2()` and `disable_ipv6()`. Since the Dockerfiles control
the mount paths, we can guarantee `network-helpers.sh` is available. Remove the fallback
definitions and fail fast:

```bash
# Replace the fallback pattern with:
SCRIPT_DIR="$(dirname "$0")"
source "${SCRIPT_DIR}/network-helpers.sh" 2>/dev/null \
  || source "/scripts/network-helpers.sh" \
  || { echo "[FATAL] network-helpers.sh not found"; exit 1; }
```

---

## New Files to Create

### `CODEOWNERS`

```text
# Service ownership
/services/gate/       @OdraLabsHQ/network
/services/sentinel/   @OdraLabsHQ/security
/services/toolbox/    @OdraLabsHQ/platform
/services/state/      @OdraLabsHQ/platform
/services/workspace/  @OdraLabsHQ/runtime
/lib/                 @OdraLabsHQ/core
/agents/              @OdraLabsHQ/agents
/deploy/              @OdraLabsHQ/core
/tools/polis.sh       @OdraLabsHQ/core
```

Adjust team names to match your GitHub org structure.

### `.dockerignore`

```text
.git
.github
target
tests
docs
*.md
!README.md
Cargo.lock
.env
certs/ca/*.key
secrets/
tools/fix-line-endings.sh
```

### `Justfile` (unified task runner)

```just
# Polis — unified task runner
# Install just: https://github.com/casey/just

default:
    @just --list

# Build all containers
build:
    ./tools/polis.sh build

# Build a specific service
build-service service:
    docker build -f services/{{service}}/Dockerfile .

# Run all tests
test:
    ./tests/run-tests.sh all

# Run unit tests only
test-unit:
    ./tests/run-tests.sh unit

# Run Rust tests
test-rust:
    cargo test --workspace

# Start all services
up:
    ./tools/polis.sh up

# Stop all services
down:
    ./tools/polis.sh down

# Show service status
status:
    ./tools/polis.sh status

# Generate Valkey certs
setup-valkey-certs dir="./certs/valkey":
    ./services/state/scripts/generate-certs.sh {{dir}}

# Generate Valkey secrets
setup-valkey-secrets dir="./secrets":
    ./services/state/scripts/generate-secrets.sh {{dir}} .

# Setup CA certificates
setup-ca:
    ./tools/polis.sh setup-ca
```

### `.github/dependabot.yml`

```yaml
version: 2
updates:
  # Rust dependencies
  - package-ecosystem: cargo
    directory: "/"
    schedule:
      interval: weekly
    groups:
      rust-deps:
        patterns: ["*"]

  # GitHub Actions
  - package-ecosystem: github-actions
    directory: "/"
    schedule:
      interval: weekly

  # Docker — per service
  - package-ecosystem: docker
    directory: "services/gate"
    schedule:
      interval: monthly

  - package-ecosystem: docker
    directory: "services/sentinel"
    schedule:
      interval: monthly

  - package-ecosystem: docker
    directory: "services/workspace"
    schedule:
      interval: monthly

  - package-ecosystem: docker
    directory: "services/toolbox"
    schedule:
      interval: monthly
```

### Per-service README template

Each service should have a README. Template:

```markdown
# <Service Name>

<One-line description>

## Language

<Primary language(s)>

## Build

    docker build -f services/<name>/Dockerfile .

## Config

| File | Purpose |
|---|---|
| `config/<file>` | <description> |

## Dependencies

- <other services this depends on>

## Health Check

<how health is verified>
```

---

## Path-Based CI/CD

After reorganization, update CI to only build/test affected services on PRs.
The current `ci.yml` runs everything on every push. Add change detection:

```yaml
# .github/workflows/ci.yml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  changes:
    runs-on: ubuntu-latest
    outputs:
      gate: ${{ steps.filter.outputs.gate }}
      sentinel: ${{ steps.filter.outputs.sentinel }}
      toolbox: ${{ steps.filter.outputs.toolbox }}
      state: ${{ steps.filter.outputs.state }}
      workspace: ${{ steps.filter.outputs.workspace }}
      shared: ${{ steps.filter.outputs.shared }}
    steps:
      - uses: actions/checkout@v6.0.2
      - uses: dorny/paths-filter@v3
        id: filter
        with:
          filters: |
            gate:
              - 'services/gate/**'
              - 'lib/**'
            sentinel:
              - 'services/sentinel/**'
              - 'lib/**'
            toolbox:
              - 'services/toolbox/**'
              - 'lib/crates/**'
            state:
              - 'services/state/**'
            workspace:
              - 'services/workspace/**'
              - 'lib/**'
            shared:
              - 'deploy/**'
              - 'config/**'
              - 'agents/**'

  # Rust checks — only when Rust code changes
  rust:
    needs: changes
    if: ${{ needs.changes.outputs.toolbox == 'true' }}
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6.0.2
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo check --workspace
      - run: cargo test --workspace
      - run: cargo clippy --workspace -- -D warnings

  # Full integration test — when any service or shared config changes
  integration:
    needs: changes
    if: >-
      ${{ needs.changes.outputs.gate == 'true' ||
          needs.changes.outputs.sentinel == 'true' ||
          needs.changes.outputs.workspace == 'true' ||
          needs.changes.outputs.state == 'true' ||
          needs.changes.outputs.shared == 'true' ||
          github.ref == 'refs/heads/main' }}
    runs-on: ubuntu-24.04
    steps:
      # ... existing full build + test pipeline
```

This means a PR that only touches `services/toolbox/crates/` will run Rust checks
but skip the full Docker build + integration test cycle. PRs to `main` always run everything.

---

## Per-Service Versioning

Each service can have independent version tracking. Two approaches:

### VERSION file per service

```text
services/gate/VERSION        → 0.1.3
services/sentinel/VERSION    → 0.1.3
services/workspace/VERSION   → 0.1.3
services/toolbox/VERSION     → 0.1.0
```

Docker images tagged as: `ghcr.io/odralabshq/polis-gate-oss:0.1.3`

---

## Migration Order

Execute in this order to minimize breakage. Each step should be a separate PR.

### Phase 1: Create structure (no moves yet)

1. Create empty `services/gate/`, `services/sentinel/`, `services/workspace/`, `services/toolbox/`, `services/state/` directories
2. Create `lib/crates/` and `lib/shell/` directories
3. Create `tests/native/sentinel/` directory
4. Add `CODEOWNERS`, `.dockerignore`, `Justfile`, `.github/dependabot.yml`
5. Delete `build/icap/test_is_new_domain.exe`

### Phase 2: Move configs and scripts

6. Move config files to their service directories (see migration map)
7. Move scripts to their service directories
8. Move `network-helpers.sh` to `lib/shell/`
9. Update `docker-compose.yml` volume mounts
10. Update `polis.sh` path references
11. Update CI workflows

### Phase 3: Move Dockerfiles and source

12. Move Dockerfiles to service directories
13. Move C source files to `services/sentinel/modules/`
14. Move C test files to `tests/native/sentinel/`
15. Update Dockerfile COPY paths
16. Update release workflow matrix

### Phase 4: Move Rust crates

17. Move `polis-mcp-common` to `lib/crates/polis-common/`
18. Move `polis-mcp-agent` to `services/toolbox/crates/mcp-agent/`
19. Move `polis-approve-cli` to `services/toolbox/crates/approve-cli/`
20. Update `Cargo.toml` workspace members
21. Update inter-crate dependency paths
22. Add `[workspace.dependencies]` section
23. Run `cargo check --workspace` to verify

### Phase 5: Cleanup

24. Remove empty `build/`, `config/seccomp/` directories
25. Remove fallback function definitions from init scripts
26. Add per-service README files
27. Update root README with new structure
28. Update `tests/README.md` if needed
29. Update `scripts/install.sh` if it references moved paths

---

## Risks and Mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| Broken Docker builds | High | Test each Dockerfile after moving. Run full `polis.sh build` after each phase. |
| Broken CI | High | Update CI in the same PR as the moves it depends on. |
| `polis.sh` path breakage | High | This is the largest file to update (~1000 lines). Grep for all path references before and after. |
| Cargo workspace resolution | Medium | Run `cargo check --workspace` after Phase 4. Cargo will tell you exactly what's wrong. |
| Contributor confusion | Medium | Announce the reorg. Update CONTRIBUTING.md. Keep the PR descriptions clear. |
| `install.sh` breakage | Medium | The installer clones the repo, so it gets the new layout. But verify the `polis` symlink still works. |
| Git blame history | Low | Use `git mv` for all moves to preserve history. Avoid combining moves with content changes in the same commit. |

---

## Naming Convention: Polis vs Polis

Current state:

- **Polis**: project name, CLI (`polis.sh`), containers (`polis-gate`), service file (`polis-init.service`)
- **Polis**: security subsystem, Rust crates (`polis-mcp-agent`), configs (`polis.yaml`, `polis_dlp.conf`)

Decision needed: either rename all `polis-*` crates to `polis-*`, or document the convention.

If keeping both names, add this to the root README:

> **Naming**: "Polis" is the overall project and runtime. "Polis" is the security inspection
> subsystem (DLP, approval, credential scanning). Rust crates and security configs use the
> `polis` prefix. Infrastructure, containers, and the CLI use `polis`.

If renaming, do it in Phase 4 alongside the crate moves. Rename:

- `polis-mcp-common` → `polis-common`
- `polis-mcp-agent` → `polis-mcp-agent`
- `polis-approve-cli` → `polis-approve-cli`
- `polis.yaml` → keep as-is (it's the polis subsystem config)
- `polis_dlp.conf` → keep as-is (c-icap module config)
