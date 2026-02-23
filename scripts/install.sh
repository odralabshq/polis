#!/bin/bash
# =============================================================================
# Polis Installer
# =============================================================================
# One-line install: curl -fsSL https://raw.githubusercontent.com/OdraLabsHQ/polis/main/scripts/install.sh | bash
# =============================================================================

set -euo pipefail

VERSION="${POLIS_VERSION:-0.3.0-preview-2}"
INSTALL_DIR="${POLIS_HOME:-$HOME/.polis}"
REPO_OWNER="OdraLabsHQ"
REPO_NAME="polis"
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

log_info()  { echo -e "${BLUE}[INFO]${NC} $*"; return 0; }
log_ok()    { echo -e "${GREEN}[OK]${NC} $*"; return 0; }
log_warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; return 0; }
log_error() { echo -e "${RED}[ERROR]${NC} $*" >&2; return 0; }

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

MULTIPASS_MIN_VERSION="1.16.0"
MULTIPASS_VERSION="${MULTIPASS_VERSION:-1.16.1}"

semver_gte() {
    local version="$1"
    local minimum="$2"
    printf '%s\n%s\n' "$minimum" "$version" | sort -V -C
    return 0
}

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
        exit 1
    fi
    local snap_file="/tmp/multipass_${MULTIPASS_VERSION}_amd64.snap"
    curl -fsSL --proto "${CURL_PROTO}" \
        "https://github.com/canonical/multipass/releases/download/v${MULTIPASS_VERSION}/multipass_${MULTIPASS_VERSION}_amd64.snap" \
        -o "${snap_file}"
    sudo snap install "${snap_file}" --dangerous
    rm -f "${snap_file}"
    return 0
}

install_multipass_macos() {
    local pkg_file="/tmp/multipass-${MULTIPASS_VERSION}+mac-Darwin.pkg"
    curl -fsSL --proto "${CURL_PROTO}" \
        "https://github.com/canonical/multipass/releases/download/v${MULTIPASS_VERSION}/multipass-${MULTIPASS_VERSION}+mac-Darwin.pkg" \
        -o "${pkg_file}"
    sudo installer -pkg "${pkg_file}" -target /
    rm -f "${pkg_file}"
    return 0
}

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

    if [[ "${os}" == "Linux" ]]; then
        local socket="/var/snap/multipass/common/multipass_socket"
        if [[ -S "${socket}" ]] && ! [[ -r "${socket}" && -w "${socket}" ]]; then
            local socket_group
            socket_group=$(stat -c '%G' "${socket}" 2>/dev/null || true)
            log_warn "Your user cannot access the multipass socket."
            echo "  Fix: sudo usermod -aG ${socket_group} \$USER"
            echo "  Then log out and back in, or run: newgrp ${socket_group}"
        fi
    fi
    return 0
}



download_cli() {
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

    log_info "Verifying CLI SHA256..."
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
    log_ok "CLI SHA256 verified: ${expected}"
    chmod +x "${bin_dir}/polis"
    return 0
}

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

create_symlink() {
    mkdir -p "$HOME/.local/bin"
    ln -sf "${INSTALL_DIR}/bin/polis" "$HOME/.local/bin/polis"
    log_ok "Symlinked: ~/.local/bin/polis"
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

download_image() {
    local arch base_url image_name dest expected actual
    arch=$(check_arch)
    base_url="https://github.com/${REPO_OWNER}/${REPO_NAME}/releases/download/${VERSION}"

    # Discover the actual image filename from versions.json
    image_name=$(curl -fsSL -L --proto "${CURL_PROTO}" "${base_url}/versions.json" | \
        grep -o '"asset"[[:space:]]*:[[:space:]]*"[^"]*"' | cut -d'"' -f4)
    if [[ -z "${image_name}" ]]; then
        # Fall back: list release assets via GitHub API
        image_name=$(curl -fsSL --proto "${CURL_PROTO}" \
            "https://api.github.com/repos/${REPO_OWNER}/${REPO_NAME}/releases/tags/${VERSION}" | \
            grep -o '"name"[[:space:]]*:[[:space:]]*"polis-[^"]*\.qcow2"' | head -1 | cut -d'"' -f4)
    fi
    if [[ -z "${image_name}" ]]; then
        log_error "Could not determine image filename"
        exit 1
    fi

    dest="${INSTALL_DIR}/images/${image_name}"
    mkdir -p "${INSTALL_DIR}/images"

    log_info "Downloading VM image..."
    curl -fL --proto "${CURL_PROTO}" --progress-bar \
        "${base_url}/${image_name}" -o "${dest}"

    log_info "Verifying image SHA256..."
    expected=$(curl -fsSL --proto "${CURL_PROTO}" "${base_url}/checksums.sha256" | grep "${image_name}" | awk '{print $1}')
    actual=$(sha256sum "${dest}" | awk '{print $1}')
    if [[ "${expected}" != "${actual}" ]]; then
        log_error "Image SHA256 mismatch!"
        echo "  Expected: ${expected}" >&2
        echo "  Actual:   ${actual}" >&2
        rm -f "${dest}"
        exit 1
    fi
    log_ok "Image SHA256 verified: ${expected}"

    echo "${dest}"
    return 0
}

run_init() {
    local image_path="$1"
    local bin="${INSTALL_DIR}/bin/polis"

    # Clean up any existing workspace for a fresh install
    if multipass info polis &>/dev/null 2>&1; then
        log_warn "An existing polis VM was found."
        read -r -p "Remove it and start fresh? [y/N] " confirm
        if [[ "${confirm,,}" != "y" ]]; then
            log_info "Keeping existing VM. Skipping image init."
            return 0
        fi
        log_info "Removing existing polis VM..."
        multipass delete polis && multipass purge
    fi
    rm -f "${INSTALL_DIR}/state.json"

    log_info "Running: polis start --image ${image_path}"
    "${bin}" start --image "${image_path}" || {
        log_warn "polis start failed. Run manually:"
        echo "  polis start --image ${image_path}"
        return 0
    }
    return 0
}

main() {
    print_logo

    check_arch >/dev/null
    check_multipass
    log_info "Installing Polis ${VERSION}"
    download_cli
    verify_attestation
    create_symlink

    local image_path
    image_path=$(download_image)
    run_init "${image_path}"

    echo ""
    log_ok "Polis installed successfully!"
    echo ""
    echo "Get started:"
    echo "  polis start          # Create workspace"
    echo "  polis start claude   # Create workspace with Claude agent"
    echo ""
    return 0
}

main
