#!/bin/bash
# =============================================================================
# Polis Dev Installer — installs from local build artifacts
# =============================================================================
# Usage: ./scripts/install-dev.sh [--repo /path/to/polis]
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
}

log_info() { echo -e "${BLUE}[INFO]${NC} $*"; }
log_ok()   { echo -e "${GREEN}[OK]${NC} $*"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }
log_error(){ echo -e "${RED}[ERROR]${NC} $*" >&2; }

check_multipass() {
    if ! command -v multipass &>/dev/null; then
        log_error "Multipass is required but not installed"
        case "$(uname -s)" in
            Darwin) echo "  Install: https://multipass.run/install  (requires macOS 13 Ventura or later)" ;;
            Linux)  echo "  Install: sudo snap install multipass" ;;
        esac
        exit 1
    fi

    local os version_line installed_version
    os=$(uname -s)
    version_line=$(multipass version 2>/dev/null | head -1 || true)
    installed_version=$(echo "${version_line}" | awk '{print $2}')
    if [[ -n "${installed_version}" ]]; then
        if ! printf '%s\n%s\n' "1.16.0" "${installed_version}" | sort -V -C; then
            log_error "Multipass ${installed_version} is too old (need ≥ 1.16.0)."
            case "${os}" in
                Linux)  echo "  Update: sudo snap refresh multipass" ;;
                Darwin) echo "  Update: https://multipass.run/install" ;;
            esac
            exit 1
        fi
        log_ok "Multipass ${installed_version} OK"
    fi

    if [[ "${os}" == "Linux" ]] && command -v snap &>/dev/null; then
        local socket="/var/snap/multipass/common/multipass_socket"
        if [[ -S "${socket}" ]] && ! test -r "${socket}" -a -w "${socket}"; then
            local socket_group
            socket_group=$(stat -c '%G' "${socket}" 2>/dev/null || true)
            log_warn "Your user cannot access the multipass socket."
            echo "  Fix: sudo usermod -aG ${socket_group} \$USER"
            echo "  Then log out and back in, or run: newgrp ${socket_group}"
        fi
    fi
}

install_cli() {
    local cli_bin="${REPO_DIR}/cli/target/release/polis"
    if [[ ! -f "${cli_bin}" ]]; then
        log_error "CLI binary not found at ${cli_bin}"
        echo "  Build it first: cd ${REPO_DIR}/cli && cargo build --release"
        exit 1
    fi
    mkdir -p "${INSTALL_DIR}/bin"
    cp "${cli_bin}" "${INSTALL_DIR}/bin/polis"
    chmod +x "${INSTALL_DIR}/bin/polis"
    log_ok "Installed CLI from ${cli_bin}"
}

run_init() {
    local image
    image=$(find "${REPO_DIR}/packer/output" -name "*.qcow2" | sort | tail -1)
    if [[ -z "${image}" ]]; then
        log_error "No VM image found in ${REPO_DIR}/packer/output"
        echo "  Build it first: just build-vm"
        exit 1
    fi
    local sidecar="${image%.qcow2}.qcow2.sha256"
    if [[ ! -f "${sidecar}" ]]; then
        log_error "No signed sidecar found at ${sidecar}"
        echo "  Run: just build-vm  (signing happens automatically)"
        exit 1
    fi
    local pub_key="${REPO_DIR}/.secrets/polis-release.pub"
    if [[ ! -f "${pub_key}" ]]; then
        log_error "Dev public key not found at ${pub_key}"
        echo "  Run: just build-vm  (keypair is generated automatically)"
        exit 1
    fi
    local verifying_key_b64
    verifying_key_b64=$(base64 -w0 "${pub_key}")
    log_info "Running: polis start --image ${image}"
    POLIS_VERIFYING_KEY_B64="${verifying_key_b64}" \
        "${INSTALL_DIR}/bin/polis" start --image "${image}" || {
        log_warn "polis start failed. Run manually:"
        echo "  POLIS_VERIFYING_KEY_B64=$(base64 -w0 "${pub_key}") polis start --image ${image}"
    }
}

create_symlink() {
    mkdir -p "$HOME/.local/bin"
    ln -sf "${INSTALL_DIR}/bin/polis" "$HOME/.local/bin/polis"
    log_ok "Symlinked: ~/.local/bin/polis"
    if [[ ":$PATH:" != *":$HOME/.local/bin:"* ]]; then
        log_warn "\$HOME/.local/bin is not in PATH — add it to your shell rc"
    fi
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

# Clean up any existing workspace for a fresh test environment
if multipass info polis &>/dev/null 2>&1; then
    log_warn "An existing polis VM was found."
    read -r -p "Remove it and start fresh? [y/N] " confirm
    if [[ "${confirm,,}" != "y" ]]; then
        log_info "Keeping existing VM. Skipping removal."
    else
        log_info "Removing existing polis VM..."
        multipass delete polis && multipass purge
    fi
fi
rm -f "${INSTALL_DIR}/state.json"

run_init

echo ""
log_ok "Polis (dev build) installed successfully!"
echo ""
echo "Get started:"
echo "  polis run"
echo ""
