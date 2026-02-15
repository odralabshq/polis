# Polis — unified task runner
# Install just: https://github.com/casey/just

default:
    @just --list

# Build all containers
build:
    ./cli/polis.sh build

# Build a specific service
build-service service:
    docker build -f services/{{service}}/Dockerfile .

# Compile Rust workspace + run Rust tests
build-code:
    cargo test --workspace

# Compile and run C native tests
test-native:
    #!/usr/bin/env bash
    set -euo pipefail
    for src in tests/native/sentinel/test_*.c; do
        bin="${src%.c}"
        gcc -Wall -Werror -o "$bin" "$src"
        "$bin"
    done

# Run all tests
test:
    ./tests/run-tests.sh all

# Run unit tests only (BATS, no Docker)
test-unit:
    ./tests/run-tests.sh unit

# Run Rust tests
test-rust:
    cargo test --workspace

# Run integration tests (requires running containers)
test-integration:
    ./tests/run-tests.sh --ci integration

# Run E2E tests (requires running containers)
test-e2e:
    ./tests/run-tests.sh --ci e2e

# Start all services
up:
    ./cli/polis.sh up

# Stop all services
down:
    ./cli/polis.sh down

# Show service status
status:
    ./cli/polis.sh status

# Generate Valkey certs
setup-valkey-certs dir="./certs/valkey":
    ./services/state/scripts/generate-certs.sh {{dir}}

# Generate Valkey secrets
setup-valkey-secrets dir="./secrets":
    ./services/state/scripts/generate-secrets.sh {{dir}} .

# Setup CA certificates
setup-ca:
    ./cli/polis.sh setup-ca
