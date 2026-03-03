#!/bin/bash
# =============================================================================
# Polis Installer
# =============================================================================
# One-line install: curl -fsSL https://raw.githubusercontent.com/OdraLabsHQ/polis/main/scripts/install.sh | bash
# =============================================================================

set -euo pipefail

INSTALL_DIR="${POLIS_HOME:-$HOME/.polis}"
REPO_OWNER="OdraLabsHQ"
REPO_NAME="polis"
CURL_PROTO="=https"
VERSION=""  # resolved by detect_version

# SHA256 hashes for Multipass downloads — update when bumping MULTIPASS_VERSION
MULTIPASS_SHA256_MACOS="${MULTIPASS_SHA256_MACOS:-758d10dc1b71872b0ee7a17070b93fc788dba5ba45c36b980e42fd895d273489}"

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

confirm_installer_proceed() {
    echo ""
    echo -e "${YELLOW}WARNING: If an existing 'polis' VM is found, the installer will attempt to repair it.${NC}"
    echo -e "${YELLOW}If repair fails, you can start fresh with: polis delete && polis start${NC}"
    if [[ "${POLIS_INSTALL_ASSUME_Y:-}" == "1" || "${CI:-}" == "true" || ! -t 0 ]]; then
        log_info "Non-interactive mode detected — proceeding automatically."
        return 0
    fi
    read -r -p "Continue with installation? (y/n) " reply
    case "${reply,,}" in
        y|yes) ;;
        *)
            log_warn "Installation cancelled by user."
            exit 1
            ;;
    esac
}

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
    local rc=0
    printf '%s\n%s\n' "$minimum" "$version" | sort -V -C || rc=$?
    return $rc
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
    log_info "Installing Multipass ${MULTIPASS_VERSION} from Snap Store..."
    sudo snap install multipass
    return 0
}

install_multipass_macos() {
    MACOS_PKG_FILE=$(mktemp /tmp/multipass-XXXXXX.pkg)
    trap 'rm -f "${MACOS_PKG_FILE}"' EXIT
    curl -fsSL --proto "${CURL_PROTO}" \
        "https://github.com/canonical/multipass/releases/download/v${MULTIPASS_VERSION}/multipass-${MULTIPASS_VERSION}+mac-Darwin.pkg" \
        -o "${MACOS_PKG_FILE}"

    local pkg_sha256
    pkg_sha256=$(shasum -a 256 "${MACOS_PKG_FILE}" | cut -d' ' -f1)
    if [[ "${pkg_sha256}" != "${MULTIPASS_SHA256_MACOS}" ]]; then
        log_error "Multipass pkg SHA256 mismatch!"
        echo "  Expected: ${MULTIPASS_SHA256_MACOS}" >&2
        echo "  Actual:   ${pkg_sha256}" >&2
        rm -f "${MACOS_PKG_FILE}"
        exit 1
    fi
    log_ok "Multipass pkg SHA256 verified"

    sudo installer -pkg "${MACOS_PKG_FILE}" -target /
    rm -f "${MACOS_PKG_FILE}"
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

detect_version() {
    if [[ -n "${POLIS_VERSION:-}" ]]; then
        VERSION="${POLIS_VERSION#v}"
        return 0
    fi
    log_info "Detecting latest Polis release..."
    # Use tags API sorted by date — works for pre-releases (unlike /releases/latest)
    local tag
    tag=$(curl -fsSL --proto "${CURL_PROTO}" \
        "https://api.github.com/repos/${REPO_OWNER}/${REPO_NAME}/releases?per_page=1" \
        | grep '"tag_name"' | head -1 | cut -d'"' -f4)
    if [[ -z "${tag}" ]]; then
        log_error "Could not detect latest version from GitHub."
        echo "  Set POLIS_VERSION manually: POLIS_VERSION=0.4.0 bash install.sh"
        exit 1
    fi
    VERSION="${tag#v}"
    log_ok "Detected version: ${VERSION}"
    return 0
}

download_cli() {
    local arch base_url bin_dir binary_name expected actual
    arch=$(check_arch)
    base_url="https://github.com/${REPO_OWNER}/${REPO_NAME}/releases/download/v${VERSION}"
    bin_dir="${INSTALL_DIR}/bin"
    binary_name="polis-linux-${arch}"
    local tarball="${binary_name}.tar.gz"
    CHECKSUM_FILE=$(mktemp)
    trap 'rm -f "${CHECKSUM_FILE}"' EXIT

    mkdir -p "${bin_dir}"

    log_info "Downloading CLI (${arch})..."
    curl -fsSL --proto "${CURL_PROTO}" "${base_url}/${tarball}" -o "/tmp/${tarball}"
    curl -fsSL --proto "${CURL_PROTO}" "${base_url}/${binary_name}.sha256" -o "${CHECKSUM_FILE}"

    log_info "Extracting CLI..."
    tar -xzf "/tmp/${tarball}" -C "${bin_dir}" --strip-components=0
    mv "${bin_dir}/${binary_name}" "${bin_dir}/polis"
    rm -f "/tmp/${tarball}"

    log_info "Verifying CLI SHA256..."
    expected=$(cut -d' ' -f1 < "${CHECKSUM_FILE}")
    actual=$(sha256sum "${bin_dir}/polis" | cut -d' ' -f1)

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

create_symlink() {
    mkdir -p "$HOME/.local/bin"
    ln -sf "${INSTALL_DIR}/bin/polis" "$HOME/.local/bin/polis"
    log_ok "Symlinked: ~/.local/bin/polis"
    if [[ ":$PATH:" != *":$HOME/.local/bin:"* ]]; then
        log_warn "\$HOME/.local/bin is not in PATH — add it to your shell rc"
    fi
    return 0
}

main() {
    print_logo
    confirm_installer_proceed
    check_arch >/dev/null
    check_multipass
    detect_version
    log_info "Installing Polis ${VERSION}"
    download_cli
    create_symlink

    # Repair existing VM instead of destroying it
    if multipass info polis &>/dev/null 2>&1; then
        log_warn "Existing polis VM found, attempting repair..."
        if "${INSTALL_DIR}/bin/polis" doctor --fix; then
            log_ok "VM repaired and running"
            exit 0
        else
            log_error "Repair failed. To start fresh (destroys VM data):"
            log_error "  polis delete && polis start"
            exit 1
        fi
    fi

    rm -f "${INSTALL_DIR}/state.json"

    # Start (creates VM, generates certs inside VM)
    "${INSTALL_DIR}/bin/polis" start

    echo ""
    log_ok "Polis installed successfully!"
    echo ""
}

main
