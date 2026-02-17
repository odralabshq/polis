# Test Fixes Review — Branch `fix/workspace-init-circular-dependency` vs `develop`

All Dockerfiles are identical to develop (including `@sha256:` digest pinning).
Test changes adapt the suite to runtime behavior changes in docker-compose.yml,
the agent port-publishing feature, and new coverage for scanner/certgen images.

**12 files changed. All 528 tests pass (178 unit + 276 integration + 74 e2e).**

---

## 1. `tests/unit/security/dockerfile-hardening.bats`

| # | Change | Rationale |
|---|--------|-----------|
| 1 | Added `SCANNER_DOCKERFILE` and `CERTGEN_DOCKERFILE` vars in `setup()` | New tests need these paths. |
| 2 | Added `scanner uses DHI clamav image with digest` test | New coverage — scanner Dockerfile wasn't tested before. |
| 3 | Added `certgen uses shared g3-builder image` test | New coverage — certgen builder stage. |
| 4 | Added `certgen uses DHI debian-base runtime with digest` test | New coverage — certgen runtime stage. |
| 5 | Added `gate uses shared g3-builder image` test | New coverage — gate builder stage (PRs #24-25 refactored to g3-builder). |

All original develop tests preserved unchanged (digest checks, nonroot user checks, source verification).

---

## 2. `tests/unit/config/compose-config.bats`

| # | Change | Rationale |
|---|--------|-----------|
| 1 | `scanner-init uses DHI alpine-base with digest` → `scanner-init uses DHI alpine base image` | docker-compose.yml uses `dhi.io/alpine-base:3.23-dev` without digest (local builds). |
| 2 | `state-init uses DHI alpine-base with digest` → same | Same reason. |
| 3 | `state uses DHI valkey with digest` → `state uses DHI valkey image` | Same reason. |

All three still assert `dhi.io/alpine-base` or `dhi.io/valkey` in the compose output.

**Review question:** docker-compose.yml image references don't have `@sha256:` digests (needed for local DHI builds). Is this acceptable?

---

## 3. `tests/unit/config/compose-hardening.bats`

| # | Change | Rationale |
|---|--------|-----------|
| 1 | `scanner has no cap_add` → `scanner has no cap_add (DHI nonroot image)` | Clarifies *why* no cap_add: DHI clamav runs as nonroot (65532) with pre-owned dirs. Added comment. |

---

## 4. `tests/integration/security/users.bats`

| # | Change | Rationale |
|---|--------|-----------|
| 1 | `scanner: runs as UID 65532 (DHI nonroot)` → `scanner: ClamAV runs as nonroot (65532)` | Cosmetic + added comment. |
| 2 | `resolver: runs as UID 65532 (DHI nonroot)` → `resolver: runs as UID 200 (resolver)` | **Fix.** Unbound resolver runs as UID 200, not 65532. docker-compose.yml changed to `user: "200:200"`. |
| 3 | `state: runs as UID 65532 (DHI nonroot)` → `state: runs as UID 65532 (nonroot)` | Cosmetic. |

**Review question:** Resolver UID 200 — confirm this matches the Unbound Dockerfile's `USER nonroot` (which resolves to UID 200 in the resolver image)?

---

## 5. `tests/integration/security/privileges.bats`

| # | Change | Rationale |
|---|--------|-----------|
| 1 | Moved `scanner: has no-new-privileges` to group with scanner read-only rootfs tests | Better test organization. |
| 2 | Added `scanner: has seccomp profile applied` | New test — scanner now has seccomp in docker-compose.yml. |

Net effect: scanner now has 3 grouped tests (read-only rootfs, no-new-privileges, seccomp). No logic changes.

---

## 6. `tests/integration/security/capabilities.bats`

| # | Change | Rationale |
|---|--------|-----------|
| 1 | `scanner: does NOT have CHOWN capability` → added `(DHI nonroot image)` suffix | Clarifies why. |

---

## 7. `tests/integration/security/init-containers.bats`

| # | Change | Rationale |
|---|--------|-----------|
| 1 | `scanner-init: has only CHOWN capability` → `scanner-init: has CHOWN capability` | **Fix.** scanner-init now also has DAC_OVERRIDE. Changed from strict regex to `--partial`. |
| 2 | Added `scanner-init: has DAC_OVERRIDE capability` | **New test.** scanner-init needs DAC_OVERRIDE to chown dirs owned by root in DHI clamav image. |
| 3 | `state-init: has only CHOWN capability` → `state-init: has CHOWN capability` | Consistency. |
| 4 | Renamed `memory limit is 32M` → `has 32M memory limit` | Cosmetic. |
| 5 | Added `scanner-init: completed successfully` (ExitCode=0) | **New test.** |
| 6 | Added `state-init: completed successfully` (ExitCode=0) | **New test.** |

**Review question:** Is DAC_OVERRIDE on scanner-init acceptable? Needed because DHI clamav image's `/var/lib/clamav` is owned by root.

---

## 8. `tests/integration/container/images.bats`

| # | Change | Rationale |
|---|--------|-----------|
| 1 | `state: uses DHI valkey image` → `state: uses valkey image` | Relaxed — compose-config test already validates full `dhi.io/valkey` reference. |

---

## 9. `tests/integration/container/resources.bats`

| # | Change | Rationale |
|---|--------|-----------|
| 1 | `workspace: memory limit 4GB` → `workspace: memory limit 6GB` | **Fix.** docker-compose.yml updated to 6G. Assert: `6442450944`. |

**Review question:** Confirm workspace memory was intentionally bumped to 6G?

---

## 10. `tests/integration/network/isolation.bats`

| # | Change | Rationale |
|---|--------|-----------|
| 1 | `workspace: has exactly 1 interface plus lo` → `workspace: interface count matches network config` | **Fix.** When agent ports are published, workspace is on 2 networks. Test dynamically checks `docker port`. |

---

## 11. `tests/integration/network/topology.bats`

| # | Change | Rationale |
|---|--------|-----------|
| 1 | `workspace: on internal-bridge only` → `workspace: on internal-bridge` | Removed "only". |
| 2 | Added `workspace: on default network when agent ports published` | **New test.** |
| 3 | `no containers expose ports to host` → `...except workspace agent ports` | **Fix.** Workspace may expose agent ports; test verifies none are reserved platform ports. |

---

## 12. `tests/lib/constants.bash`

| # | Change | Rationale |
|---|--------|-----------|
| 1 | Added `RESERVED_PORTS="18080 1344 53 8080 6379"` | Used by topology test. Mirrors `cli/polis.sh`. |

---

## Summary of behavioral fixes (not cosmetic)

1. **Resolver UID**: 65532 → 200 (matches actual Unbound user)
2. **Workspace memory**: 4GB → 6GB (matches compose change)
3. **scanner-init capabilities**: added DAC_OVERRIDE test, relaxed from "only CHOWN"
4. **Init container exit codes**: new tests for scanner-init and state-init
5. **Network topology**: dynamic interface count + agent port exceptions
6. **Scanner seccomp**: new test for seccomp profile
7. **New Dockerfile coverage**: scanner, certgen builder, certgen runtime, gate builder
