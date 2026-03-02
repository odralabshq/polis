#!/bin/bash
# =============================================================================
# Polis Dev Installer — installs from local build artifacts
# =============================================================================
# Usage: ./scripts/install-dev.sh [--repo /path/to/polis]
#
# Does NOT use `polis start`. Creates the VM and provisions it directly via
# multipass commands so the dev flow is fully self-contained and debuggable.
# =============================================================================

set -euo pipefail

REPO_DIR="${POLIS_REPO:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
INSTALL_DIR="${POLIS_HOME:-$HOME/.polis}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Logo colors (purple → teal gradient)
cO='\033[38;2;107;33;168m'
cD='\033[38;2;93;37;163m'
cR='\033[38;2;64;47;153m'
cA1='\033[38;2;46;53;147m'
cL='\033[38;2;37;56;144m'
cA2='\033[38;2;26;107;160m'
cB='\033[38;2;26;151;179m'
cS='\033[38;2;20;184;166m'
X='\033[0m'

print_logo() {
    echo ""
    echo -e "${cO} ▄████▄ ${X} ${cD}█████▄ ${X} ${cR}█████▄ ${X} ${cA1} ▄████▄ ${X}   ${cL}██      ${X} ${cA2} ▄████▄ ${X} ${cB}█████▄ ${X} ${cS} ▄████▄${X}"
    echo -e "${cO}██    ██${X} ${cD}██   ██${X} ${cR}██   ██${X} ${cA1}██    ██${X}   ${cL}██      ${X} ${cA2}██    ██${X} ${cB}██   ██${X} ${cS}██     ${X}"
    echo -e "${cO}██    ██${X} ${cD}██   ██${X} ${cR}██   ██${X} ${cA1}██    ██${X}   ${cL}██      ${X} ${cA2}██    ██${X} ${cB}██   ██${X} ${cS}██     ${X}"
    echo -e "${cO}██    ██${X} ${cD}██   ██${X} ${cR}█████▀ ${X} ${cA1}████████${X}   ${cL}██      ${X} ${cA2}████████${X} ${cB}█████▀ ${X} ${cS} ▀████▄${X}"
    echo -e "${cO}██    ██${X} ${cD}██   ██${X} ${cR}██  ██ ${X} ${cA1}██    ██${X}   ${cL}██      ${X} ${cA2}██    ██${X} ${cB}██   ██${X} ${cS}      ██${X}"
    echo -e "${cO}██    ██${X} ${cD}██   ██${X} ${cR}██   ██${X} ${cA1}██    ██${X}   ${cL}██      ${X} ${cA2}██    ██${X} ${cB}██   ██${X} ${cS}      ██${X}"
    echo -e "${cO} ▀████▀ ${X} ${cD}█████▀ ${X} ${cR}██   ██${X} ${cA1}██    ██${X}   ${cL}████████${X} ${cA2}██    ██${X} ${cB}█████▀ ${X} ${cS} ▀████▀${X}"
    echo ""
    return 0
}

log_info() { echo -e "${BLUE}[INFO]${NC} $*"; return 0; }
log_ok()   { echo -e "${GREEN}[OK]${NC} $*"; return 0; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $*"; return 0; }
log_error(){ echo -e "${RED}[ERROR]${NC} $*" >&2; return 0; }

confirm_destructive_reinstall() {
    echo ""
    echo -e "${RED}WARNING: Continuing will delete the existing 'polis' VM and remove previous workspace data.${NC}"
    if [[ "${POLIS_INSTALL_ASSUME_Y:-}" == "1" || "${CI:-}" == "true" || ! -t 0 ]]; then
        log_info "Non-interactive mode detected — proceeding automatically."
        return 0
    fi
    read -r -p "Proceed with clean reinstall? (y/n) " reply
    case "${reply,,}" in
        y|yes) ;;
        *)
            log_warn "Installation cancelled by user."
            exit 1
            ;;
    esac
}

check_multipass() {
    if ! command -v multipass &>/dev/null; then
        log_error "Multipass is required but not installed."
        echo "  Install: https://multipass.run/install"
        exit 1
    fi

    local version_line installed_version ver_clean
    version_line=$(multipass version 2>/dev/null | head -1 || true)
    installed_version=$(echo "${version_line}" | awk '{print $2}')
    if [[ -n "${installed_version}" ]]; then
        ver_clean="${installed_version%%+*}"
        if ! printf '%s\n%s\n' "1.16.0" "${ver_clean}" | sort -V -C; then
            log_error "Multipass ${installed_version} is too old (need ≥ 1.16.0)."
            echo "  Update: https://multipass.run/install"
            exit 1
        fi
        log_ok "Multipass ${installed_version} OK"
    fi
    return 0
}

install_cli() {
    local cli_bin="${REPO_DIR}/cli/target/release/polis"
    if [[ ! -f "${cli_bin}" ]]; then
        log_error "CLI binary not found at ${cli_bin}"
        echo "  Build it first: cd ${REPO_DIR} && just build"
        exit 1
    fi
    mkdir -p "${INSTALL_DIR}/bin"
    cp "${cli_bin}" "${INSTALL_DIR}/bin/polis"
    chmod +x "${INSTALL_DIR}/bin/polis"
    log_ok "Installed CLI from ${cli_bin}"
    return 0
}

create_symlink() {
    mkdir -p "$HOME/.local/bin"
    ln -sf "${INSTALL_DIR}/bin/polis" "$HOME/.local/bin/polis"
    log_ok "Symlinked: ~/.local/bin/polis"
    if [[ ":$PATH:" != *":$HOME/.local/bin:"* ]]; then
        log_warn "\$HOME/.local/bin is not in PATH — add it to your shell rc"
    fi
    return 0
}

run_init() {
    local images_tar="${REPO_DIR}/.build/polis-images.tar.zst"
    local config_tar="${REPO_DIR}/.build/assets/polis-setup.config.tar"
    local cloud_init="${REPO_DIR}/cloud-init.yaml"

    for f in "${images_tar}" "${config_tar}" "${cloud_init}"; do
        if [[ ! -f "${f}" ]]; then
            log_error "Required file not found: ${f}"
            echo "  Build first: cd ${REPO_DIR} && just build"
            return 1
        fi
    done

    # ── Step 1: Create VM ─────────────────────────────────────────────────
    log_info "Creating VM with cloud-init..."
    multipass launch 24.04 \
        --name polis \
        --cpus 2 \
        --memory 8G \
        --disk 40G \
        --cloud-init "${cloud_init}" \
        --timeout 900 || {
        log_error "VM creation failed"
        return 1
    }
    log_ok "VM created"

    log_info "Waiting for cloud-init..."
    multipass exec polis -- cloud-init status --wait || {
        log_error "cloud-init failed"
        return 1
    }
    log_ok "cloud-init complete"

    # ── Step 2: Transfer config ───────────────────────────────────────────
    log_info "Transferring config tarball..."
    multipass transfer "${config_tar}" polis:/tmp/polis-setup.config.tar || {
        log_error "Failed to transfer config tarball"
        return 1
    }
    multipass exec polis -- tar xf /tmp/polis-setup.config.tar -C /opt/polis --no-same-owner || {
        log_error "Failed to extract config tarball"
        return 1
    }
    multipass exec polis -- rm -f /tmp/polis-setup.config.tar

    # Write .env with version
    local cli_version
    cli_version=$("${INSTALL_DIR}/bin/polis" --version 2>&1 | awk '{print $2}')
    local tag="v${cli_version}"
    multipass exec polis -- bash -c "cat > /opt/polis/.env << 'ENVEOF'
# Generated by install-dev.sh
POLIS_RESOLVER_VERSION=${tag}
POLIS_CERTGEN_VERSION=${tag}
POLIS_GATE_VERSION=${tag}
POLIS_SENTINEL_VERSION=${tag}
POLIS_SCANNER_VERSION=${tag}
POLIS_WORKSPACE_VERSION=${tag}
POLIS_HOST_INIT_VERSION=${tag}
POLIS_STATE_VERSION=${tag}
POLIS_TOOLBOX_VERSION=${tag}
ENVEOF"

    # Fix script permissions and strip Windows CRLF line endings
    multipass exec polis -- find /opt/polis -name '*.sh' -exec chmod +x {} +
    multipass exec polis -- find /opt/polis -name '*.sh' -exec sed -i 's/\r//' {} +
    log_ok "Config transferred"

    # ── Step 3: Load Docker images ────────────────────────────────────────
    log_info "Loading Docker images into VM..."
    multipass transfer "${images_tar}" polis:/tmp/polis-images.tar.zst || {
        log_error "Failed to transfer images tarball"
        return 1
    }
    multipass exec polis -- bash -c 'zstd -d /tmp/polis-images.tar.zst --stdout | docker load && rm -f /tmp/polis-images.tar.zst' || {
        log_error "Failed to load Docker images"
        return 1
    }
    log_ok "Docker images loaded"

    # Tag images with CLI version
    log_info "Tagging images as ${tag}..."
    multipass exec polis -- bash -c "
        docker images --format '{{.Repository}}:{{.Tag}}' | grep ':latest' | while read -r img; do
            base=\"\${img%%:*}\"
            docker tag \"\$img\" \"\${base}:${tag}\"
        done
    "

    # go-httpbin is a test-only image (profiles: ["test"]).
    # It is included in the tarball when built via `just build`.
    # Skip pulling it here — it is not needed for normal operation.

    # ── Step 4: Generate certs and secrets ────────────────────────────────
    log_info "Generating certificates and secrets..."
    multipass exec polis -- sudo bash -c '/opt/polis/scripts/generate-ca.sh /opt/polis/certs/ca'
    multipass exec polis -- sudo bash -c '/opt/polis/services/state/scripts/generate-certs.sh /opt/polis/certs/valkey'
    multipass exec polis -- sudo bash -c '/opt/polis/services/state/scripts/generate-secrets.sh /opt/polis/secrets /opt/polis'
    multipass exec polis -- sudo bash -c '/opt/polis/services/toolbox/scripts/generate-certs.sh /opt/polis/certs/toolbox /opt/polis/certs/ca'
    multipass exec polis -- sudo bash -c '/opt/polis/scripts/fix-cert-ownership.sh /opt/polis'
    log_ok "Certificates and secrets ready"

    # ── Step 5: Start services ────────────────────────────────────────────
    log_info "Starting services..."
    multipass exec polis -- bash -c 'cd /opt/polis && docker compose --env-file .env up -d --remove-orphans' || {
        log_error "Failed to start services"
        return 1
    }
    log_ok "Services started"

    # ── Step 6: Wait for workspace container to become healthy ────────────
    log_info "Waiting for workspace to become healthy (up to 120s)..."
    local healthy=false
    for ((i=1; i<=60; i++)); do
        local json
        json=$(multipass exec polis -- docker compose -f /opt/polis/docker-compose.yml ps --format json workspace 2>/dev/null || true)
        if [[ -n "${json}" ]]; then
            local first_line
            first_line=$(echo "${json}" | head -1)
            # Check for both "running" state and "healthy" health in the JSON line
            if echo "${first_line}" | grep -q '"State".*"running"' && echo "${first_line}" | grep -q '"Health".*"healthy"'; then
                healthy=true
                break
            fi
        fi
        sleep 2
    done
    if ${healthy}; then
        log_ok "Workspace is healthy"
    else
        log_warn "Workspace did not become healthy within 120s — check with: polis status"
    fi
    return 0
}

# Parse flags
while [[ $# -gt 0 ]]; do
    case "$1" in
        --repo)   [[ $# -ge 2 ]] || { log_error "--repo requires a value"; exit 1; }
                  REPO_DIR="$2"; shift 2 ;;
        --repo=*) REPO_DIR="${1#*=}"; shift ;;
        *)        log_error "Unknown flag: $1"; exit 1 ;;
    esac
done

print_logo
log_info "Repo: ${REPO_DIR}"
log_info "Install dir: ${INSTALL_DIR}"
echo ""

check_multipass
install_cli
create_symlink

# Remove any existing VM for a clean reinstall
log_info "Checking for existing polis VM..."
vm_found=false
if timeout 30 multipass list --format csv 2>/dev/null | grep -q '^polis,'; then
    vm_found=true
fi

if ${vm_found}; then
    confirm_destructive_reinstall
    log_info "Existing polis VM found — deleting for clean reinstall..."
    multipass delete polis --purge 2>/dev/null
    log_ok "VM deleted"
fi
rm -f "${INSTALL_DIR}/state.json"

run_init

echo ""
log_ok "Polis (dev build) installed successfully!"
echo ""
echo "NEXT STEPS:"
echo "1. Verify status:"
echo "   polis status"
echo "2. Start an AI agent (pass your provider API key directly):"
echo "   polis start --agent openclaw -e OPENAI_API_KEY=sk-..."
echo "   Note: agent initialization may take several minutes depending on the selected agent."
echo "3. Connect to the dashboard:"
echo "   polis connect"
echo ""
