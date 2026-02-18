# Polis — unified task runner
# Install just: https://github.com/casey/just

set shell := ["bash", "-euo", "pipefail", "-c"]

default:
    @just --list

# ── Lint ────────────────────────────────────────────────────────────
lint: lint-rust lint-c lint-shell

lint-rust:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings

lint-c:
    find services/sentinel/modules -name '*.c' -print0 | \
      xargs -0 cppcheck --enable=warning,performance --error-exitcode=1

lint-shell:
    shellcheck tools/dev-vm.sh scripts/install.sh packer/scripts/*.sh

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

# Run all test tiers (unit + integration + e2e)
test-all: test test-integration test-e2e

# ── Format (auto-fix) ───────────────────────────────────────────────
fmt:
    cargo fmt --all

# ── Build ───────────────────────────────────────────────────────────
build:
    docker compose build

# Build a specific service
build-service service:
    docker build -f services/{{service}}/Dockerfile .

# Build VM image (requires packer)
build-vm: build _export-images
    #!/usr/bin/env bash
    set -euo pipefail
    cd packer
    packer init .
    packer build -var "images_tar=${PWD}/../.build/polis-images.tar" polis-vm.pkr.hcl

# Internal: export Docker images for VM build
_export-images:
    #!/usr/bin/env bash
    set -euo pipefail
    IMAGES=$(grep -oP 'image:\s+\Kpolis-[a-z]+-oss:\S+' docker-compose.yml | sort -u)
    if [[ -z "${IMAGES}" ]]; then
        echo "ERROR: No polis-*-oss images found in docker-compose.yml" >&2
        exit 1
    fi
    mkdir -p .build
    chmod 700 .build
    echo "Exporting: ${IMAGES}"
    # shellcheck disable=SC2086
    docker save -o .build/polis-images.tar ${IMAGES}

build-all: build-vm

# ── Setup ───────────────────────────────────────────────────────────
setup: setup-ca setup-valkey setup-toolbox
    @echo "✓ All certificates and secrets generated"

setup-ca:
    ./cli/polis.sh setup-ca

setup-valkey:
    ./services/state/scripts/generate-certs.sh ./certs/valkey
    ./services/state/scripts/generate-secrets.sh ./secrets .
    sudo chown 65532:65532 ./certs/valkey/server.key ./certs/valkey/client.key

setup-toolbox:
    ./services/toolbox/scripts/generate-certs.sh ./certs/toolbox ./certs/ca
    sudo chown 65532:65532 ./certs/toolbox/toolbox.key

# ── Dev VM ──────────────────────────────────────────────────────────
dev-create:
    ./tools/dev-vm.sh create

dev-shell:
    ./tools/dev-vm.sh shell

dev-stop:
    ./tools/dev-vm.sh stop

dev-delete:
    ./tools/dev-vm.sh delete

# ── Lifecycle ───────────────────────────────────────────────────────
up:
    docker compose stop 2>/dev/null || true
    ./cli/polis.sh up

down:
    docker compose down --volumes --remove-orphans
    docker system prune -af --volumes

status:
    ./cli/polis.sh status

logs service="":
    ./cli/polis.sh logs {{service}}

# ── Release ─────────────────────────────────────────────────────────
package-vm arch="amd64":
    #!/usr/bin/env bash
    set -euo pipefail
    VERSION=$(git describe --tags --always)
    cp output/polis-vm-*.qcow2 "polis-vm-${VERSION}-{{arch}}.qcow2"
    sha256sum "polis-vm-${VERSION}-{{arch}}.qcow2" > "polis-vm-${VERSION}-{{arch}}.qcow2.sha256"

# ── Clean ───────────────────────────────────────────────────────────
clean:
    rm -rf output/ .build/
    docker compose down -v --remove-orphans 2>/dev/null || true

# WARNING: Removes certs, secrets, and .env
clean-all: clean
    rm -rf certs/ secrets/ .env
    docker system prune -af --volumes
