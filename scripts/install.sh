#!/bin/bash
# =============================================================================
# Polis Installer
# =============================================================================
# One-line install: curl -fsSL https://raw.githubusercontent.com/OdraLabsHQ/polis/main/scripts/install.sh | bash
# =============================================================================

set -euo pipefail

# Configuration
VERSION="${POLIS_VERSION:-latest}"
INSTALL_DIR="${POLIS_HOME:-$HOME/.polis}"
REPO_OWNER="OdraLabsHQ"
REPO_NAME="polis"
IMAGE_URL=""
CURL_PROTO="=https"

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
log_ok() { echo -e "${GREEN}[OK]${NC} $*"; return 0; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $*"; return 0; }
log_error() { echo -e "${RED}[ERROR]${NC} $*" >&2; return 0; }

# Check architecture
check_arch() {
    local arch
    arch=$(uname -m)
    case "${arch}" in
        x86_64|amd64) echo "amd64" ;;
        aarch64|arm64) echo "arm64" ;;
        *)
            log_error "Unsupported architecture: ${arch}"
            exit 1
            ;;
    esac
    return 0
}

# Minimum required Multipass version
MULTIPASS_MIN_VERSION="1.16.0"
# Latest known release (used when auto-installing)
MULTIPASS_VERSION="${MULTIPASS_VERSION:-1.16.1}"

# Compare semver strings: returns 0 if $1 >= $2
semver_gte() {
    local v1="$1" v2="$2"
    printf '%s\n%s\n' "${v2}" "${v1}" | sort -V -C
    return 0
}

# Install Multipass on Linux via snap
install_multipass_linux() {
    local arch
    arch=$(check_arch)
    if [[ "${arch}" == "arm64" ]]; then
        log_error "ARM64 polis workspace images are not yet available."
        echo "  Supported: x86_64 (amd64) only."
        exit 1
    fi
    if ! command -v snap &>/dev/null; then
        log_error "snapd is required to install Multipass on Linux."
        echo "  Install snapd: https://snapcraft.io/docs/installing-snapd"
        echo "  Then re-run this installer."
        exit 1
    fi
    local snap_file="/tmp/multipass_${MULTIPASS_VERSION}_amd64.snap"
    local url="https://github.com/canonical/multipass/releases/download/v${MULTIPASS_VERSION}/multipass_${MULTIPASS_VERSION}_amd64.snap"
    log_info "Downloading Multipass ${MULTIPASS_VERSION} for Linux..."
    curl -fsSL --proto "${CURL_PROTO}" "${url}" -o "${snap_file}"
    log_info "Installing Multipass snap..."
    sudo snap install "${snap_file}" --dangerous
    rm -f "${snap_file}"
    return 0
}

# Install Multipass on macOS via .pkg
install_multipass_macos() {
    local pkg_file="/tmp/multipass-${MULTIPASS_VERSION}+mac-Darwin.pkg"
    local url="https://github.com/canonical/multipass/releases/download/v${MULTIPASS_VERSION}/multipass-${MULTIPASS_VERSION}+mac-Darwin.pkg"
    log_info "Downloading Multipass ${MULTIPASS_VERSION} for macOS..."
    curl -fsSL --proto "${CURL_PROTO}" "${url}" -o "${pkg_file}"
    log_info "Installing Multipass (requires sudo)..."
    sudo installer -pkg "${pkg_file}" -target /
    rm -f "${pkg_file}"
    return 0
}

# Check for Multipass: auto-install if missing, verify version >= minimum
check_multipass() {
    local os
    os=$(uname -s)

    if ! command -v multipass &>/dev/null; then
        log_info "Multipass not found — installing..."
        case "${os}" in
            Linux)  install_multipass_linux ;;
            Darwin) install_multipass_macos ;;
            *)
                log_error "Automatic Multipass install is not supported on ${os}."
                echo "  Install manually: https://multipass.run/install"
                exit 1
                ;;
        esac
    fi

    # Version check
    local version_line installed_version
    version_line=$(multipass version 2>/dev/null | head -1 || true)
    installed_version=$(echo "${version_line}" | awk '{print $2}')
    if [[ -z "${installed_version}" ]]; then
        log_warn "Could not determine Multipass version — proceeding anyway."
    elif ! semver_gte "${installed_version}" "${MULTIPASS_MIN_VERSION}"; then
        log_error "Multipass ${installed_version} is too old (need ≥ ${MULTIPASS_MIN_VERSION})."
        case "${os}" in
            Linux)  echo "  Update: sudo snap refresh multipass" ;;
            Darwin) echo "  Update: brew upgrade multipass" ;;
            *)      echo "  Update: https://multipass.run/install" ;;
        esac
        exit 1
    else
        log_ok "Multipass ${installed_version} OK"
    fi

    # Linux post-install config
    if [[ "${os}" == "Linux" ]]; then
        configure_multipass_linux
    fi
    return 0
}

# Post-install Linux config: socket group check
configure_multipass_linux() {
    local socket="/var/snap/multipass/common/multipass_socket"
    if [[ -S "${socket}" ]] && ! [[ -r "${socket}" && -w "${socket}" ]]; then
        local socket_group
        socket_group=$(stat -c '%G' "${socket}" 2>/dev/null || true)
        log_warn "Your user cannot access the multipass socket."
        echo "  Fix: sudo usermod -aG ${socket_group} \$USER"
        echo "  Then log out and back in, or run: newgrp ${socket_group}"
    fi
    return 0
}

# Resolve version tag
resolve_version() {
    if [[ "${VERSION}" == "latest" ]]; then
        local response http_code body
        response=$(curl -fsSL --proto "${CURL_PROTO}" -w "\n%{http_code}" \
            "https://api.github.com/repos/${REPO_OWNER}/${REPO_NAME}/releases/latest" \
            2>&1) || true
        http_code=$(echo "${response}" | tail -1)
        body=$(echo "${response}" | sed '$d')

        if [[ "${http_code}" == "403" ]]; then
            log_error "GitHub API rate limit exceeded (60 requests/hour unauthenticated)"
            echo "  Set GITHUB_TOKEN or use: install.sh --version v0.3.0"
            exit 1
        fi

        if command -v jq &>/dev/null; then
            VERSION=$(echo "${body}" | jq -r '.tag_name')
        else
            VERSION=$(echo "${body}" | grep '"tag_name"' | head -1 | cut -d'"' -f4)
        fi

        if [[ -z "${VERSION}" || "${VERSION}" == "null" ]]; then
            log_error "Failed to resolve latest version"
            exit 1
        fi
    fi
    log_info "Installing Polis ${VERSION}"
    return 0
}

# Download and verify with SHA256
download_and_verify() {
    local arch base_url bin_dir binary_name checksum_file expected actual
    arch=$(check_arch)
    base_url="https://github.com/${REPO_OWNER}/${REPO_NAME}/releases/download/${VERSION}"
    bin_dir="${INSTALL_DIR}/bin"
    binary_name="polis-linux-${arch}"
    checksum_file="/tmp/polis.sha256.$$"

    mkdir -p "${bin_dir}"

    log_info "Downloading polis CLI (${arch})..."
    curl -fsSL --proto "${CURL_PROTO}" "${base_url}/${binary_name}" -o "${bin_dir}/polis"
    curl -fsSL --proto "${CURL_PROTO}" "${base_url}/${binary_name}.sha256" -o "${checksum_file}"

    log_info "Verifying SHA256 checksum..."
    expected=$(cut -d' ' -f1 < "${checksum_file}")
    actual=$(sha256sum "${bin_dir}/polis" | cut -d' ' -f1)
    rm -f "${checksum_file}"

    if [[ "${actual}" != "${expected}" ]]; then
        log_error "SHA256 checksum mismatch!"
        echo "  Expected: ${expected}" >&2
        echo "  Actual:   ${actual}" >&2
        rm -f "${bin_dir}/polis"
        exit 1
    fi
    log_ok "SHA256 verified: ${expected}"
    chmod +x "${bin_dir}/polis"
    return 0
}

# Optional attestation verification
verify_attestation() {
    if command -v gh &>/dev/null; then
        log_info "Verifying GitHub attestation..."
        if gh attestation verify "${INSTALL_DIR}/bin/polis" --owner "${REPO_OWNER}" 2>/dev/null; then
            log_ok "Attestation verified"
        else
            log_info "Attestation verification skipped (not available or failed)"
        fi
    fi
    return 0
}

# Non-fatal image init step
init_image() {
    local bin="${INSTALL_DIR}/bin/polis"
    if [[ ! -x "${bin}" ]]; then
        log_warn "Image download failed. Run 'polis init' to retry."
        return 0
    fi
    if [[ -n "${IMAGE_URL}" ]]; then
        "${bin}" init --image "${IMAGE_URL}" || {
            log_warn "Image download failed. Run 'polis init' to retry."
            return 0
        }
    else
        "${bin}" init || {
            log_warn "Image download failed. Run 'polis init' to retry."
            return 0
        }
    fi
    return 0
}

# Create symlink
create_symlink() {
    mkdir -p "$HOME/.local/bin"
    ln -sf "${INSTALL_DIR}/bin/polis" "$HOME/.local/bin/polis"
    log_ok "Created symlink: ~/.local/bin/polis"

    if [[ ":$PATH:" != *":$HOME/.local/bin:"* ]]; then
        log_warn "\$HOME/.local/bin is not in your PATH"
        echo ""
        echo "Add it by running:"
        # shellcheck disable=SC2016
        echo '  export PATH="$HOME/.local/bin:$PATH"'
        echo ""
        echo "To make it permanent, add to your shell rc file (~/.bashrc or ~/.zshrc)"
    fi
    return 0
}

# Main
main() {
    print_logo

    check_arch >/dev/null
    check_multipass
    resolve_version
    download_and_verify
    verify_attestation
    create_symlink
    init_image

    echo ""
    log_ok "Polis installed successfully!"
    echo ""
    echo "Get started:"
    echo "  polis start          # Create workspace"
    echo "  polis start claude   # Create workspace with Claude agent"
    echo ""
    return 0
}

# Parse flags
while [[ $# -gt 0 ]]; do
    case "$1" in
        --image)
            [[ $# -ge 2 ]] || { log_error "--image requires a value"; exit 1; }
            IMAGE_URL="$2"; shift 2 ;;
        --image=*)
            IMAGE_URL="${1#*=}"; shift ;;
        --version)
            [[ $# -ge 2 ]] || { log_error "--version requires a value"; exit 1; }
            VERSION="$2"; shift 2 ;;
        --version=*)
            VERSION="${1#*=}"; shift ;;
        *)
            log_error "Unknown flag: $1"; exit 1 ;;
    esac
done

main
