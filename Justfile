# Polis — unified task runner
# Install just: https://github.com/casey/just

default:
    @just --list

# ── Lint ────────────────────────────────────────────────────────────
lint: lint-rust lint-c

lint-rust:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings

lint-c:
    find services/sentinel/modules -name '*.c' -print0 | \
      xargs -0 cppcheck --enable=warning,style,performance

# ── Test ────────────────────────────────────────────────────────────
test: test-rust test-c test-bats

test-rust:
    cargo test --workspace

test-c:
    #!/usr/bin/env bash
    set -euo pipefail
    for src in tests/native/sentinel/test_*.c; do
        bin="${src%.c}"
        gcc -Wall -Werror -o "$bin" "$src"
        "$bin"
    done

test-bats:
    ./tests/run-tests.sh unit

# Alias for test-c
test-native: test-c

# Run integration tests (requires running containers)
test-integration:
    ./tests/run-tests.sh --ci integration

# Run E2E tests (requires running containers)
test-e2e:
    ./tests/run-tests.sh --ci e2e

# ── Format (auto-fix) ───────────────────────────────────────────────
fmt:
    cargo fmt --all

# ── Build ───────────────────────────────────────────────────────────
build:
    ./cli/polis.sh build

# Build a specific service
build-service service:
    docker build -f services/{{service}}/Dockerfile .

# ── Lifecycle ───────────────────────────────────────────────────────
up:
    ./cli/polis.sh up

down:
    ./cli/polis.sh down

status:
    ./cli/polis.sh status

# ── Setup ───────────────────────────────────────────────────────────
setup-ca:
    ./cli/polis.sh setup-ca

setup-valkey-certs dir="./certs/valkey":
    ./services/state/scripts/generate-certs.sh {{dir}}

setup-valkey-secrets dir="./secrets":
    ./services/state/scripts/generate-secrets.sh {{dir}} .
