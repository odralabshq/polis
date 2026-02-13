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
