# Polis — unified task runner
# Install just: https://github.com/casey/just

set shell := ["bash", "-euo", "pipefail", "-c"]

default:
    @just --list

# ── Install ─────────────────────────────────────────────────────────
install-tools:
    #!/usr/bin/env bash
    set -euo pipefail
    sudo apt-get update -qq
    # Docker
    command -v docker &>/dev/null || sudo apt-get install -y docker.io
    # shellcheck
    command -v shellcheck &>/dev/null || sudo apt-get install -y shellcheck
    # hadolint
    if ! command -v hadolint &>/dev/null; then
        curl -fsSL -o /tmp/hadolint https://github.com/hadolint/hadolint/releases/download/v2.12.0/hadolint-Linux-x86_64
        sudo install -m 755 /tmp/hadolint /usr/local/bin/hadolint
        rm /tmp/hadolint
    fi
    # container-structure-test
    if ! command -v container-structure-test &>/dev/null; then
        curl -fsSL -o /tmp/container-structure-test https://github.com/GoogleContainerTools/container-structure-test/releases/download/v1.19.3/container-structure-test-linux-amd64
        sudo install -m 755 /tmp/container-structure-test /usr/local/bin/container-structure-test
        rm /tmp/container-structure-test
    fi
    # Multipass
    command -v multipass &>/dev/null || sudo snap install multipass
    # HashiCorp repo for packer
    if ! command -v packer &>/dev/null; then
        sudo apt-get install -y gnupg curl
        curl -fsSL https://apt.releases.hashicorp.com/gpg \
            | sudo gpg --dearmor -o /usr/share/keyrings/hashicorp.gpg
        echo "deb [signed-by=/usr/share/keyrings/hashicorp.gpg] https://apt.releases.hashicorp.com $(lsb_release -cs) main" \
            | sudo tee /etc/apt/sources.list.d/hashicorp.list
        sudo apt-get update -qq
        sudo apt-get install -y packer
    fi
    # QEMU + xorriso for VM builds
    sudo apt-get install -y qemu-system-x86 qemu-utils ovmf xorriso

# ── Lint ────────────────────────────────────────────────────────────
lint: lint-rust lint-c lint-shell

lint-rust:
    cargo fmt --all --check --manifest-path cli/Cargo.toml
    cargo fmt --all --check --manifest-path services/toolbox/Cargo.toml
    cargo clippy --workspace --all-targets --manifest-path cli/Cargo.toml -- -D warnings -A dead-code
    cargo clippy --workspace --all-targets --manifest-path services/toolbox/Cargo.toml -- -D warnings

lint-c:
    find services/sentinel/modules -name '*.c' -print0 | \
      xargs -0 cppcheck --enable=warning,performance --error-exitcode=1

lint-shell:
    shellcheck tools/dev-vm.sh tools/blocked.sh scripts/install.sh packer/scripts/*.sh

# ── Test ────────────────────────────────────────────────────────────
test: test-rust test-c test-unit

test-rust:
    cargo test --workspace --manifest-path cli/Cargo.toml -- --skip proptests
    cargo test --workspace --manifest-path services/toolbox/Cargo.toml
    cargo test --manifest-path lib/crates/polis-common/Cargo.toml

test-rust-proptests:
    cargo test --workspace --manifest-path cli/Cargo.toml -- proptests

test-c:
    #!/usr/bin/env bash
    set -euo pipefail
    for src in tests/native/sentinel/test_*.c; do
        bin="${src%.c}"
        gcc -Wall -Werror -o "$bin" "$src"
        "$bin"
    done

test-unit:
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

# Full clean-build-test cycle — CI equivalent, stops on first failure
test-clean: clean-all build setup up test-all

# ── Format (auto-fix) ───────────────────────────────────────────────
fmt:
    cargo fmt --all --manifest-path cli/Cargo.toml
    cargo fmt --all --manifest-path services/toolbox/Cargo.toml

# ── Build ───────────────────────────────────────────────────────────
build:
    docker compose build

# Build a specific service
build-service service:
    docker build -f services/{{service}}/Dockerfile .

# Build VM image (requires packer)
# Usage: just build-vm [arch=amd64|arm64] [headless=true|false]
build-vm arch="amd64" headless="true": build _export-images _bundle-config
    #!/usr/bin/env bash
    set -euo pipefail
    cd packer
    packer init .
    packer build \
        -var "images_tar=${PWD}/../.build/polis-images.tar" \
        -var "config_tar=${PWD}/../.build/polis-config.tar.gz" \
        -var "arch={{arch}}" \
        -var "headless={{headless}}" \
        polis-vm.pkr.hcl

# Internal: export Docker images for VM build
_export-images:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ -z "${POLIS_IMAGE_VERSION:-}" ]]; then
        echo "ERROR: POLIS_IMAGE_VERSION is not set" >&2
        exit 1
    fi
    # Set per-service vars so docker compose config resolves the image refs
    for svc in RESOLVER CERTGEN GATE SENTINEL SCANNER WORKSPACE HOST_INIT STATE TOOLBOX; do
        export "POLIS_${svc}_VERSION=${POLIS_IMAGE_VERSION}"
    done
    # Get all images with env vars resolved
    IMAGES=$(docker compose -f docker-compose.yml config | grep -oP 'image:\s+\K\S+' | sort -u | grep -v 'go-httpbin')
    if [[ -z "${IMAGES}" ]]; then
        echo "ERROR: No images found in docker-compose.yml" >&2
        exit 1
    fi
    mkdir -p .build
    chmod 700 .build
    echo "Pulling external images..."
    EXPORT_IMAGES=""
    for img in ${IMAGES}; do
        if [[ ! "$img" =~ ^ghcr\.io/odralabshq/polis- ]]; then
            docker pull "$img" || true
            # Strip @sha256:... suffix for export (docker load doesn't preserve digests)
            simple_tag="${img%%@sha256:*}"
            if [[ "$simple_tag" != "$img" ]]; then
                echo "Tagging $img as $simple_tag"
                docker tag "$img" "$simple_tag"
                EXPORT_IMAGES="$EXPORT_IMAGES $simple_tag"
            else
                EXPORT_IMAGES="$EXPORT_IMAGES $img"
            fi
        else
            EXPORT_IMAGES="$EXPORT_IMAGES $img"
        fi
    done
    echo "Exporting:${EXPORT_IMAGES}"
    # shellcheck disable=SC2086
    docker save -o .build/polis-images.tar ${EXPORT_IMAGES}

# Internal: bundle config files for VM build
_bundle-config:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ -z "${POLIS_IMAGE_VERSION:-}" ]]; then
        echo "Resolving latest image version from GitHub..."
        POLIS_IMAGE_VERSION=$(curl -fsSL --proto '=https' \
            -H 'Accept: application/vnd.github+json' \
            'https://api.github.com/repos/OdraLabsHQ/polis/releases/latest' \
            | grep '"tag_name"' | head -1 | cut -d'"' -f4)
        if [[ -z "${POLIS_IMAGE_VERSION}" ]]; then
            echo "ERROR: Failed to resolve latest image version" >&2
            exit 1
        fi
        echo "Using image version: ${POLIS_IMAGE_VERSION}"
    fi
    export POLIS_IMAGE_VERSION
    bash packer/scripts/bundle-polis-config.sh

build-all: build-vm

# ── Setup ───────────────────────────────────────────────────────────
setup: setup-ca setup-valkey setup-toolbox
    @echo "✓ Setup complete"

setup-ca:
    #!/usr/bin/env bash
    set -euo pipefail
    CA_DIR=certs/ca
    CA_KEY="${CA_DIR}/ca.key"
    CA_PEM="${CA_DIR}/ca.pem"
    if [[ -f "$CA_KEY" && -f "$CA_PEM" ]]; then echo "✓ CA exists"; exit 0; fi
    echo "→ Generating CA..."
    rm -f "$CA_KEY" "$CA_PEM"
    mkdir -p "$CA_DIR"
    openssl genrsa -out "$CA_KEY" 4096 2>/dev/null
    openssl req -new -x509 -days 3650 -key "$CA_KEY" -out "$CA_PEM" \
        -subj "/C=US/ST=Local/L=Local/O=Polis/OU=Gateway/CN=Polis CA" 2>/dev/null
    chmod 644 "$CA_KEY" "$CA_PEM"
    echo "✓ CA generated"

setup-valkey:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "→ Generating Valkey certs and secrets..."
    sudo rm -f ./certs/valkey/*.key ./certs/valkey/*.crt 2>/dev/null || true
    ./services/state/scripts/generate-certs.sh ./certs/valkey &>/dev/null
    ./services/state/scripts/generate-secrets.sh ./secrets . &>/dev/null
    sudo chown 65532:65532 ./certs/valkey/server.key ./certs/valkey/client.key
    echo "✓ Valkey certs and secrets ready"

setup-toolbox:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "→ Generating Toolbox certs..."
    sudo rm -f ./certs/toolbox/*.key ./certs/toolbox/*.pem 2>/dev/null || true
    ./services/toolbox/scripts/generate-certs.sh ./certs/toolbox ./certs/ca >/dev/null
    sudo chown 65532:65532 ./certs/toolbox/toolbox.key
    echo "✓ Toolbox certs ready"

# Generates the compose.override.yaml that mounts agent scripts into the workspace
# container. Required before `just up` for HITL workspace tests to work.
# Note: this does not start agents — it only sets up the volume mount layer.
setup-agents:
    #!/usr/bin/env bash
    set -euo pipefail
    OVERRIDE="agents/openclaw/.generated/compose.override.yaml"
    if [[ -f "$OVERRIDE" ]]; then echo "Agent override already exists."; exit 0; fi
    mkdir -p agents/openclaw/.generated
    printf '%s\n' \
        '# CI-generated minimal override — mounts agent scripts into workspace' \
        '# Full override is generated by the polis CLI on first run' \
        'services:' \
        '  workspace:' \
        '    volumes:' \
        '      - ./agents/openclaw/:/tmp/agents/openclaw/:ro' \
        > "$OVERRIDE"
    echo "✓ Agent override generated."

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
    #!/usr/bin/env bash
    set -euo pipefail
    docker compose down --remove-orphans 2>/dev/null || true
    sudo systemctl restart sysbox 2>/dev/null || true
    timeout 15 bash -c 'until sudo systemctl is-active sysbox &>/dev/null; do sleep 1; done' || true
    touch .env
    OVERRIDE="agents/openclaw/.generated/compose.override.yaml"
    OVERRIDE_FLAG=""
    [[ -f "$OVERRIDE" ]] && OVERRIDE_FLAG="-f $OVERRIDE"
    # shellcheck disable=SC2086
    docker compose -f docker-compose.yml $OVERRIDE_FLAG --env-file .env up -d
    # shellcheck disable=SC2086
    docker compose -f docker-compose.yml $OVERRIDE_FLAG --env-file .env ps
down:
    docker compose down --volumes --remove-orphans

status:
    #!/usr/bin/env bash
    OVERRIDE="agents/openclaw/.generated/compose.override.yaml"
    OVERRIDE_FLAG=""
    [[ -f "$OVERRIDE" ]] && OVERRIDE_FLAG="-f $OVERRIDE"
    # shellcheck disable=SC2086
    docker compose -f docker-compose.yml $OVERRIDE_FLAG --env-file .env ps

logs service="":
    #!/usr/bin/env bash
    OVERRIDE="agents/openclaw/.generated/compose.override.yaml"
    OVERRIDE_FLAG=""
    [[ -f "$OVERRIDE" ]] && OVERRIDE_FLAG="-f $OVERRIDE"
    # shellcheck disable=SC2086
    docker compose -f docker-compose.yml $OVERRIDE_FLAG --env-file .env logs --tail=50 -f {{service}}

# ── Release ─────────────────────────────────────────────────────────
package-vm arch="amd64":
    #!/usr/bin/env bash
    set -euo pipefail
    VERSION=$(git describe --tags --always)
    cp packer/output/polis-workspace-*.qcow2 "polis-workspace-${VERSION}-{{arch}}.qcow2"
    sha256sum "polis-workspace-${VERSION}-{{arch}}.qcow2" > "polis-workspace-${VERSION}-{{arch}}.qcow2.sha256"

# ── Clean ───────────────────────────────────────────────────────────
clean:
    docker compose down --volumes --remove-orphans
    docker system prune -af --volumes
    rm -rf output/ .build/

# WARNING: Removes certs, secrets, and .env
clean-all: clean
    rm -rf certs/ secrets/ .env
