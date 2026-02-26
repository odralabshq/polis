# Polis — unified task runner
# Install just: https://github.com/casey/just

set shell := ["bash", "-euo", "pipefail", "-c"]
set windows-shell := ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command"]
set dotenv-load := false
set export

default:
	@just --list

# ── Install ─────────────────────────────────────────────────────────
install-tools:
	#!/usr/bin/env bash
	set -euo pipefail
	sudo apt-get update -qq
	sudo apt-get install -y build-essential pkg-config libssl-dev
	# Docker (docker-ce)
	if ! command -v docker &>/dev/null; then
		curl -fsSL https://download.docker.com/linux/ubuntu/gpg \
			| sudo gpg --dearmor -o /usr/share/keyrings/docker.gpg
		echo "deb [arch=amd64 signed-by=/usr/share/keyrings/docker.gpg] https://download.docker.com/linux/ubuntu $(lsb_release -cs) stable" \
			| sudo tee /etc/apt/sources.list.d/docker.list
		sudo apt-get update -qq
		sudo apt-get install -y docker-ce docker-ce-cli containerd.io docker-compose-plugin
		sudo usermod -aG docker "${USER}"
	fi
	# Sysbox container runtime
	if ! docker info 2>/dev/null | grep -q sysbox-runc; then
		SYSBOX_VERSION="0.6.6"
		SYSBOX_DEB="/tmp/sysbox-ce.deb"
		curl -fsSL -o "${SYSBOX_DEB}" \
			"https://downloads.nestybox.com/sysbox/releases/v${SYSBOX_VERSION}/sysbox-ce_${SYSBOX_VERSION}-0.linux_amd64.deb"
		sudo apt-get install -y "${SYSBOX_DEB}"
		rm "${SYSBOX_DEB}"
	fi
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
	# Rust toolchain
	if ! command -v cargo &>/dev/null; then
		curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
		source "${HOME}/.cargo/env"
	fi
	# Git submodules (bats-core)
	git submodule update --init

# Windows-only: Install all prerequisites for building Polis
# Installs: just, Docker Desktop, shellcheck, hadolint, Rust toolchain, zipsign
install-tools-windows:
	powershell -NoProfile -ExecutionPolicy Bypass scripts/install-tools-windows.ps1

# Windows-only: Check all prerequisites are present (no installs, just diagnose)
check-tools-windows:
	powershell -NoProfile -ExecutionPolicy Bypass scripts/check-tools-windows.ps1

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
	shellcheck tools/dev-vm.sh tools/blocked.sh scripts/install.sh

# ── Test ────────────────────────────────────────────────────────────
test: test-rust test-c test-unit

test-rust:
	cargo test --workspace --manifest-path cli/Cargo.toml --test unit -- --test-threads=1
	cargo test --workspace --manifest-path cli/Cargo.toml --test integration
	cargo test --workspace --manifest-path services/toolbox/Cargo.toml
	cargo test --manifest-path lib/crates/polis-common/Cargo.toml


# Run CLI BATS spec tests (tests/bats/cli-spec.bats) — needs multipass + built VM + installed polis
test-cli filter="":
	#!/usr/bin/env bash
	set -euo pipefail
	args=()
	[[ -n "{{filter}}" ]] && args+=(--filter "{{filter}}")
	./tests/bats/run-cli-spec-tests.sh "${args[@]}"

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

# Run integration tests (requires running containers)
test-integration:
	./tests/run-tests.sh --ci integration

# Run E2E tests (requires running containers)
test-e2e:
	./tests/run-tests.sh --ci e2e

# Run all test tiers (unit + integration + e2e)
test-all: test test-integration test-e2e

# Full clean-build-test cycle — CI equivalent, stops on first failure
test-clean: clean-all prepare-config build-docker setup up test-all

# ── Format (auto-fix) ───────────────────────────────────────────────
fmt:
	cargo fmt --all --manifest-path cli/Cargo.toml
	cargo fmt --all --manifest-path services/toolbox/Cargo.toml

# ── Build ───────────────────────────────────────────────────────────
prepare-config:
	#!/usr/bin/env bash
	set -euo pipefail
	mkdir -p .build/assets
	# Generate agent artifacts (skip _template)
	for agent_dir in agents/*/; do
		name=$(basename "$agent_dir")
		[ "$name" = "_template" ] && continue
		[ -f "${agent_dir}agent.yaml" ] || continue
		echo "→ Generating artifacts for agent: $name"
		./scripts/generate-agent.sh "$name" agents
	done
	# Build config tarball (sudo needed to read keys owned by container uid 65532)
	sudo tar cf .build/assets/polis-setup.config.tar \
		docker-compose.yml \
		scripts/ \
		agents/ \
		services/*/config/ \
		services/*/scripts/ \
		$([ -d certs ] && echo "certs/") \
		$([ -d secrets ] && echo "secrets/")
	sudo chown "$(id -u):$(id -g)" .build/assets/polis-setup.config.tar
	echo "✓ Built .build/assets/polis-setup.config.tar"
	# Copy cloud-init.yaml
	cp cloud-init.yaml .build/assets/cloud-init.yaml
	echo "✓ Copied cloud-init.yaml"
	# Create stub image-digests.json if not present
	if [ ! -f .build/assets/image-digests.json ]; then
		echo '{}' > .build/assets/image-digests.json
		echo "✓ Created stub image-digests.json"
	fi

build: prepare-config build-cli build-docker save-docker-images

# Windows-only: Build all components
build-windows: prepare-config build-cli build-docker save-docker-images

# Quick build — skips asset preparation
build-quick: build-cli

# Build the CLI binary
build-cli:
	cargo build --release --manifest-path cli/Cargo.toml

# Build Docker images
build-docker:
	docker compose build

# Save all Docker images as a compressed tarball for dev VM loading
save-docker-images:
	#!/usr/bin/env bash
	set -euo pipefail
	mkdir -p .build/assets
	VERSION="v$(cargo metadata --no-deps --format-version 1 --manifest-path cli/Cargo.toml \
		| jq -r '.packages[0].version')"
	IMAGES=$(docker compose config --images | sort -u)
	# Tag all images with the CLI version so docker-compose .env resolves
	for img in $IMAGES; do
		base="${img%%:*}"
		if [ "$base" != "$img" ] && docker image inspect "$base:latest" &>/dev/null; then
			docker tag "$base:latest" "$base:$VERSION"
		fi
	done
	echo "→ Saving $(echo "$IMAGES" | wc -w) images..."
	docker save $IMAGES | zstd -T0 -3 -o .build/polis-images.tar.zst --force
	echo "✓ Saved .build/polis-images.tar.zst ($(du -h .build/polis-images.tar.zst | cut -f1))"

# Build a specific service
build-service service:
	docker build -f services/{{service}}/Dockerfile .

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
	chmod 600 "$CA_KEY"
	chmod 644 "$CA_PEM"
	sudo chown "$(id -u):65532" "$CA_KEY"
	chmod 640 "$CA_KEY"
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
	docker compose -f docker-compose.yml --env-file .env up -d
	docker compose -f docker-compose.yml --env-file .env ps
	echo ""
	echo "Control plane started. Start an agent with: polis start --agent=<name>"
down:
	docker compose down --volumes --remove-orphans

status:
	docker compose -f docker-compose.yml --env-file .env ps

logs service="":
	docker compose -f docker-compose.yml --env-file .env logs --tail=50 -f {{service}}

# ── Clean ───────────────────────────────────────────────────────────
clean:
	docker compose down --volumes --remove-orphans
	docker system prune -af --volumes
	rm -rf output/ .build/

# WARNING: Removes certs, secrets, and .env
clean-all: clean
	rm -rf certs/ secrets/ .env
