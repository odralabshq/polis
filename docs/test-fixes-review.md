# Test Fixes Review — Branch `fix/workspace-init-circular-dependency` vs `develop`

Only 4 test files changed from develop, each with the minimum fix required.
All 520 tests pass (174 unit + 272 integration + 74 e2e).

---

## 1. `tests/unit/config/compose-config.bats`

Removed `@sha256:` digest assertions from 3 tests. docker-compose.yml references
`dhi.io/alpine-base:3.23-dev` and `dhi.io/valkey:8.1` without digests (digests were
removed to support local DHI builds). The tests still assert the `dhi.io/` image names.

| Test | Change |
|------|--------|
| `scanner-init uses DHI alpine-base` | Removed `assert_output --partial "@sha256:"` |
| `state-init uses DHI alpine-base` | Same |
| `state uses DHI valkey` | Same |

---

## 2. `tests/integration/container/resources.bats`

Workspace memory limit changed from 4G to 6G in docker-compose.yml.

| Test | Change |
|------|--------|
| `workspace: memory limit 6GB` | Expected value `4294967296` → `6442450944` |

---

## 3. `tests/integration/security/init-containers.bats`

scanner-init now has `DAC_OVERRIDE` in addition to `CHOWN` (needed to chown
directories owned by root inside the DHI clamav image).

| Test | Change |
|------|--------|
| `scanner-init: has CHOWN and DAC_OVERRIDE capabilities` | Changed from strict regex `^(CHOWN\|CAP_CHOWN)$` to two `--partial` assertions for CHOWN and DAC_OVERRIDE |

---

## 4. `tests/integration/security/users.bats`

Resolver runs as UID 200 (the `resolver` user in the Unbound image), not 65532.
docker-compose.yml changed to `user: "200:200"`.

| Test | Change |
|------|--------|
| `resolver: runs as UID 200 (resolver)` | Expected UID `65532` → `200` |
