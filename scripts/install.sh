#!/bin/bash
# =============================================================================
# Polis Installer
# =============================================================================
# One-line install: curl -fsSL https://raw.githubusercontent.com/OdraLabsHQ/polis/main/scripts/install.sh | bash
# =============================================================================

set -euo pipefail

VERSION="${POLIS_VERSION:-0.3.0}"
INSTALL_DIR="${POLIS_HOME:-$HOME/.polis}"
REPO_OWNER="OdraLabsHQ"
REPO_NAME="polis"
CURL_PROTO="=https"

# SHA256 hashes for Multipass downloads — update when bumping MULTIPASS_VERSION
MULTIPASS_SHA256_LINUX_AMD64="${MULTIPASS_SHA256_LINUX_AMD64:-PLACEHOLDER_UPDATE_WHEN_BUMPING_VERSION}"
MULTIPASS_SHA256_MACOS="${MULTIPASS_SHA256_MACOS:-PLACEHOLDER_UPDATE_WHEN_BUMPING_VERSION}"

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
    local snap_file
    snap_file=$(mktemp --suffix=".snap")
    trap 'rm -f "${snap_file}"' EXIT
    curl -fsSL --proto "${CURL_PROTO}" \
        "https://github.com/canonical/multipass/releases/download/v${MULTIPASS_VERSION}/multipass_${MULTIPASS_VERSION}_amd64.snap" \
        -o "${snap_file}"

    local snap_sha256
    snap_sha256=$(sha256sum "${snap_file}" | cut -d' ' -f1)
    if [[ "${snap_sha256}" != "${MULTIPASS_SHA256_LINUX_AMD64}" ]]; then
        log_error "Multipass snap SHA256 mismatch!"
        echo "  Expected: ${MULTIPASS_SHA256_LINUX_AMD64}" >&2
        echo "  Actual:   ${snap_sha256}" >&2
        rm -f "${snap_file}"
        exit 1
    fi
    log_ok "Multipass snap SHA256 verified"

    sudo snap install "${snap_file}" --dangerous
    rm -f "${snap_file}"
    return 0
}

install_multipass_macos() {
    local pkg_file
    pkg_file=$(mktemp --suffix=".pkg")
    trap 'rm -f "${pkg_file}"' EXIT
    curl -fsSL --proto "${CURL_PROTO}" \
        "https://github.com/canonical/multipass/releases/download/v${MULTIPASS_VERSION}/multipass-${MULTIPASS_VERSION}+mac-Darwin.pkg" \
        -o "${pkg_file}"

    local pkg_sha256
    pkg_sha256=$(shasum -a 256 "${pkg_file}" | cut -d' ' -f1)
    if [[ "${pkg_sha256}" != "${MULTIPASS_SHA256_MACOS}" ]]; then
        log_error "Multipass pkg SHA256 mismatch!"
        echo "  Expected: ${MULTIPASS_SHA256_MACOS}" >&2
        echo "  Actual:   ${pkg_sha256}" >&2
        rm -f "${pkg_file}"
        exit 1
    fi
    log_ok "Multipass pkg SHA256 verified"

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
    tarball="${binary_name}.tar.gz"
    checksum_file=$(mktemp)
    trap 'rm -f "${checksum_file}"' EXIT

    mkdir -p "${bin_dir}"

    log_info "Downloading CLI (${arch})..."
    curl -fsSL --proto "${CURL_PROTO}" "${base_url}/${tarball}" -o "/tmp/${tarball}"
    curl -fsSL --proto "${CURL_PROTO}" "${base_url}/${binary_name}.sha256" -o "${checksum_file}"

    log_info "Extracting CLI..."
    tar -xzf "/tmp/${tarball}" -C "${bin_dir}" --strip-components=0
    mv "${bin_dir}/${binary_name}" "${bin_dir}/polis"
    rm -f "/tmp/${tarball}"

    log_info "Verifying CLI SHA256..."
    expected=$(cut -d' ' -f1 < "${checksum_file}")
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
    check_arch >/dev/null
    check_multipass
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
