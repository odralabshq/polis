#!/bin/bash
# =============================================================================
# Polis Installer
# =============================================================================
# One-line install: curl -fsSL https://raw.githubusercontent.com/OdraLabsHQ/polis-core/main/scripts/install.sh | bash
# =============================================================================

set -euo pipefail

# Configuration
REPO_OWNER="OdraLabsHQ"
REPO_NAME="polis-core"
INSTALL_DIR="${POLIS_INSTALL_DIR:-$HOME/.polis}"
BRANCH="${POLIS_BRANCH:-main}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log_info() { echo -e "${BLUE}[INFO]${NC} $*"; }
log_success() { echo -e "${GREEN}[OK]${NC} $*"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }
log_error() { echo -e "${RED}[ERROR]${NC} $*" >&2; }

# Detect OS and architecture
detect_platform() {
    local os arch
    os=$(uname -s | tr '[:upper:]' '[:lower:]')
    arch=$(uname -m)
    
    case "$os" in
        linux) ;;
        darwin)
            log_error "macOS is not currently supported (requires Sysbox which is Linux-only)"
            exit 1
            ;;
        *)
            log_error "Unsupported operating system: $os"
            exit 1
            ;;
    esac
    
    case "$arch" in
        x86_64|amd64) arch="amd64" ;;
        aarch64|arm64) arch="arm64" ;;
        *)
            log_error "Unsupported architecture: $arch"
            exit 1
            ;;
    esac
    
    echo "${os}-${arch}"
}

# Check prerequisites
check_prerequisites() {
    log_info "Checking prerequisites..."
    
    local missing=()
    
    # Docker
    if ! command -v docker &>/dev/null; then
        missing+=("docker")
    elif ! docker info &>/dev/null; then
        log_error "Docker is installed but not running or accessible"
        echo "Make sure Docker is running and your user is in the docker group"
        exit 1
    fi
    
    # curl or wget
    if ! command -v curl &>/dev/null && ! command -v wget &>/dev/null; then
        missing+=("curl or wget")
    fi
    
    # openssl (for CA generation)
    if ! command -v openssl &>/dev/null; then
        missing+=("openssl")
    fi
    
    if [[ ${#missing[@]} -gt 0 ]]; then
        log_error "Missing required tools: ${missing[*]}"
        echo ""
        echo "Install them with:"
        echo "  Ubuntu/Debian: sudo apt-get install ${missing[*]}"
        echo "  Fedora/RHEL:   sudo dnf install ${missing[*]}"
        exit 1
    fi
    
    log_success "All prerequisites met"
}

# Download file helper
download() {
    local url="$1"
    local dest="$2"
    
    if command -v curl &>/dev/null; then
        curl -fsSL "$url" -o "$dest"
    else
        wget -q "$url" -O "$dest"
    fi
}

# Main installation
install_polis() {
    echo ""
    echo "╔═══════════════════════════════════════════════════════════════╗"
    echo "║                    Polis Installer                            ║"
    echo "║         Secure AI Workspace with Traffic Inspection           ║"
    echo "╚═══════════════════════════════════════════════════════════════╝"
    echo ""
    
    detect_platform
    check_prerequisites
    
    # Create install directory
    log_info "Installing to ${INSTALL_DIR}..."
    mkdir -p "${INSTALL_DIR}"
    cd "${INSTALL_DIR}"
    
    # Check if git is available for cloning
    if command -v git &>/dev/null; then
        log_info "Cloning Polis repository..."
        
        # Try HTTPS first (works for public repos)
        if git clone --depth 1 --branch "${BRANCH}" "https://github.com/${REPO_OWNER}/${REPO_NAME}.git" . 2>/dev/null; then
            log_success "Repository cloned successfully"
        # Try SSH (works if user has SSH keys configured)
        elif git clone --depth 1 --branch "${BRANCH}" "git@github.com:${REPO_OWNER}/${REPO_NAME}.git" . 2>/dev/null; then
            log_success "Repository cloned via SSH"
        else
            log_error "Failed to clone repository"
            echo ""
            echo "Make sure:"
            echo "  - The repository exists: https://github.com/${REPO_OWNER}/${REPO_NAME}"
            echo "  - The branch exists: ${BRANCH}"
            echo "  - The repository is public, or you have access"
            echo ""
            echo "For private repos, clone manually:"
            echo "  git clone git@github.com:${REPO_OWNER}/${REPO_NAME}.git ${INSTALL_DIR}"
            exit 1
        fi
    else
        # Fallback to downloading individual files (only works for public repos)
        log_info "Git not found, downloading files directly..."
        download_files
    fi
    
    # Make scripts executable
    chmod +x tools/polis.sh scripts/*.sh 2>/dev/null || true
    
    # Create symlink for easy access
    log_info "Creating polis command..."
    mkdir -p "$HOME/.local/bin"
    ln -sf "${INSTALL_DIR}/tools/polis.sh" "$HOME/.local/bin/polis"
    
    # Check if ~/.local/bin is in PATH
    if [[ ":$PATH:" != *":$HOME/.local/bin:"* ]]; then
        log_warn "~/.local/bin is not in your PATH"
        echo ""
        echo "Add it by running:"
        echo '  echo '\''export PATH="$HOME/.local/bin:$PATH"'\'' >> ~/.bashrc && source ~/.bashrc'
        echo ""
    fi
    
    log_success "Polis installed successfully!"
    echo ""
    echo "═══════════════════════════════════════════════════════════════"
    echo ""
    echo "Next steps:"
    echo ""
    echo "  1. Configure your API key:"
    echo "     cp ${INSTALL_DIR}/config/openclaw.env.example ${INSTALL_DIR}/.env"
    echo "     nano ${INSTALL_DIR}/.env"
    echo "     # Add ANTHROPIC_API_KEY, OPENAI_API_KEY, or OPENROUTER_API_KEY"
    echo ""
    echo "  2. Initialize Polis:"
    echo "     cd ${INSTALL_DIR} && ./tools/polis.sh init"
    echo ""
    echo "  The init command will:"
    echo "    - Check Docker compatibility"
    echo "    - Install Sysbox runtime"
    echo "    - Build/pull container images"
    echo "    - Start all services"
    echo "    - Help you pair your first device"
    echo ""
    echo "Documentation: https://github.com/${REPO_OWNER}/${REPO_NAME}"
    echo ""
}

# Download files directly (fallback for when git is not available)
download_files() {
    local base_url="https://raw.githubusercontent.com/${REPO_OWNER}/${REPO_NAME}/${BRANCH}"
    
    # URL-encode the branch name (replace + with %2B, / with %2F for safety)
    local encoded_branch
    encoded_branch=$(echo "$BRANCH" | sed 's/+/%2B/g')
    base_url="https://raw.githubusercontent.com/${REPO_OWNER}/${REPO_NAME}/${encoded_branch}"
    
    # Create directory structure
    mkdir -p tools scripts config deploy certs/ca build/workspace/scripts config/seccomp
    
    # Download essential files
    download "${base_url}/tools/polis.sh" "tools/polis.sh" || exit 1
    chmod +x tools/polis.sh
    
    download "${base_url}/deploy/docker-compose.yml" "deploy/docker-compose.yml" || exit 1
    download "${base_url}/config/g3proxy.yaml" "config/g3proxy.yaml" || exit 1
    download "${base_url}/config/g3fcgen.yaml" "config/g3fcgen.yaml" || exit 1
    download "${base_url}/config/c-icap.conf" "config/c-icap.conf" || exit 1
    download "${base_url}/config/squidclamav.conf" "config/squidclamav.conf" || exit 1
    download "${base_url}/config/freshclam.conf" "config/freshclam.conf" || exit 1
    download "${base_url}/config/polis-init.service" "config/polis-init.service" || exit 1
    download "${base_url}/config/openclaw.service" "config/openclaw.service" || exit 1
    download "${base_url}/config/openclaw.env.example" "config/openclaw.env.example" || exit 1
    
    # Download seccomp profiles
    download "${base_url}/config/seccomp/gateway.json" "config/seccomp/gateway.json" || exit 1
    download "${base_url}/config/seccomp/icap.json" "config/seccomp/icap.json" || exit 1
    
    # Download scripts
    download "${base_url}/scripts/workspace-init.sh" "scripts/workspace-init.sh" || exit 1
    download "${base_url}/scripts/g3proxy-init.sh" "scripts/g3proxy-init.sh" || exit 1
    download "${base_url}/scripts/health-check.sh" "scripts/health-check.sh" || exit 1
    chmod +x scripts/*.sh
    
    log_success "Files downloaded"
}

# Run installer
install_polis
