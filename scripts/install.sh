#!/bin/bash
# Polis CLI Installer
# Usage: curl -fsSL https://raw.githubusercontent.com/odralabshq/polis/main/scripts/install.sh | bash

set -euo pipefail

REPO="odralabshq/polis"
INSTALL_DIR="/usr/local/bin"
BINARY_NAME="polis"

# Temp directory for downloads
TMP_DIR=""
cleanup() {
    if [ -n "$TMP_DIR" ] && [ -d "$TMP_DIR" ]; then
        rm -rf "$TMP_DIR"
    fi
}
trap cleanup EXIT

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

info() { echo -e "${GREEN}[INFO]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }

# Detect if running in WSL
is_wsl() {
    grep -qiE "(microsoft|wsl)" /proc/version 2>/dev/null
}

# Check if running on Debian-based distro
check_distro() {
    if [ -f /etc/os-release ]; then
        . /etc/os-release
        case "$ID" in
            debian|ubuntu|linuxmint|pop|elementary|zorin|kali)
                return 0
                ;;
            *)
                if [ -n "$ID_LIKE" ]; then
                    case "$ID_LIKE" in
                        *debian*|*ubuntu*)
                            return 0
                            ;;
                    esac
                fi
                error "Unsupported Linux distribution: $ID. Currently only Debian-based distros (Debian, Ubuntu, etc.) are supported. Other distros coming soon!"
                ;;
        esac
    else
        warn "Could not detect Linux distribution. Proceeding anyway..."
    fi
}

# Detect OS and architecture
detect_platform() {
    local os arch

    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Linux)
            os="linux"
            check_distro
            ;;
        Darwin) error "macOS is not yet supported. Stay tuned!" ;;
        *)      error "Unsupported OS: $os" ;;
    esac

    case "$arch" in
        x86_64|amd64) arch="amd64" ;;
        aarch64|arm64) error "ARM64 is not yet supported. Stay tuned!" ;;
        *)             error "Unsupported architecture: $arch" ;;
    esac

    echo "${os}-${arch}"
}

# Get latest release version (includes pre-releases)
get_latest_version() {
    curl -fsSL "https://api.github.com/repos/${REPO}/releases" | \
        grep '"tag_name":' | head -1 | sed -E 's/.*"([^"]+)".*/\1/'
}

# Download and install
install_polis() {
    local platform version download_url

    info "Detecting platform..."
    platform="$(detect_platform)"
    info "Platform: $platform"

    if is_wsl; then
        info "WSL detected - Polis will configure Sysbox automatically during 'polis install'"
    fi

    info "Fetching latest version..."
    version="$(get_latest_version)"
    if [ -z "$version" ]; then
        error "Failed to fetch latest version. Check your internet connection."
    fi
    info "Version: $version"

    download_url="https://github.com/${REPO}/releases/download/${version}/${BINARY_NAME}-${platform}"

    TMP_DIR="$(mktemp -d)"

    info "Downloading Polis CLI..."
    if ! curl -fsSL "$download_url" -o "$TMP_DIR/$BINARY_NAME"; then
        error "Failed to download from $download_url"
    fi

    chmod +x "$TMP_DIR/$BINARY_NAME"

    info "Installing to $INSTALL_DIR..."
    if [ -w "$INSTALL_DIR" ]; then
        mv "$TMP_DIR/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"
    else
        sudo mv "$TMP_DIR/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"
    fi

    info "Polis CLI installed successfully!"
    echo ""
    echo "Run 'polis version' to verify the installation."
    echo "Run 'polis install' to set up your secure workspace."
}

install_polis
