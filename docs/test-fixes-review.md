# Test Fixes Review — Branch `fix/workspace-init-circular-dependency` vs `develop`

The test suite was completely restructured: old monolithic test files were replaced
with focused, categorized files under `tests/{unit,integration,e2e}/`. All new test
files are written from scratch for the new architecture.

Only 2 test files contain behavioral changes that differ from what develop would
expect. 1 new test file was added for HITL coverage.

**All 532 tests pass (174 unit + 272 integration + 86 e2e).**

---

## 1. `tests/unit/config/compose-config.bats` (3 assertions relaxed)

docker-compose.yml uses `dhi.io/alpine-base:3.23-dev` and `dhi.io/valkey:8.1`
without `@sha256:` digests (digests removed to support local DHI image builds).
Tests still assert the `dhi.io/` image names.

| Test | Change |
|------|--------|
| `scanner-init uses DHI alpine-base` | Removed `assert_output --partial "@sha256:"` |
| `state-init uses DHI alpine-base` | Same |
| `state uses DHI valkey` | Same |

---

## 2. `tests/integration/security/init-containers.bats` (1 capability added)

scanner-init needs `DAC_OVERRIDE` in addition to `CHOWN` to chown directories
owned by root inside the DHI clamav image.

| Test | Change |
|------|--------|
| `scanner-init: has CHOWN and DAC_OVERRIDE capabilities` | Asserts both CHOWN and DAC_OVERRIDE via `--partial` (was strict regex for CHOWN only) |

---

## 3. `tests/e2e/toolbox/hitl-approval.bats` (new — 12 tests)

End-to-end tests for the `polis blocked` CLI command (HITL approval workflow).

| Test | What it verifies |
|------|------------------|
| unknown subcommand shows usage | CLI error handling |
| pending with no requests | Empty state handling |
| pending lists seeded request | Blocked key visibility |
| approve moves key from blocked to approved | Core approval flow |
| approve sets TTL ≤300s | Time-bounded approvals |
| approve creates host-based approval key | Host-level approval propagation |
| approve for nonexistent request fails | Error on missing request |
| deny removes blocked key | Core deny flow |
| check shows pending | Status reporting |
| check shows approved after approval | Status transition |
| check shows not found for unknown | Unknown request handling |
| approved request not in pending list | State consistency |
