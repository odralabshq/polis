#!/bin/bash
# polis-dev.sh - Automated Polis Development Environment Setup
# Creates a Multipass VM with Docker + Sysbox and mounts your local Polis directory

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m' # No Color

log_info() { echo -e "${BLUE}[INFO]${NC} $*"; }
log_success() { echo -e "${GREEN}[OK]${NC} $*"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }
log_error() { echo -e "${RED}[ERROR]${NC} $*" >&2; }
log_step() { echo -e "${CYAN}[STEP]${NC} $*"; }

# Detect script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# Default VM settings
VM_NAME="${POLIS_DEV_VM_NAME:-polis-dev}"
VM_CPUS="${POLIS_DEV_CPUS:-4}"
VM_MEMORY="${POLIS_DEV_MEMORY:-8G}"
VM_DISK="${POLIS_DEV_DISK:-50G}"
UBUNTU_VERSION="24.04"

show_usage() {
    cat <<EOF
${BOLD}Polis Developer Environment Setup${NC}

Usage: $0 [command] [options]

Commands:
  create        Create a new development VM
  start         Start the development VM
  stop          Stop the development VM
  shell         Enter the development VM shell
  delete        Delete the development VM
  status        Show VM status
  mount         Mount local polis directory (auto-mounted on create)
  unmount       Unmount local polis directory
  rebuild       Rebuild Polis from source inside VM
  fix-perms     Fix file ownership issues in mounted directory

Options:
  --name=NAME       VM name (default: polis-dev)
  --cpus=N          Number of CPUs (default: 4)
  --memory=SIZE     Memory size (default: 8G)
  --disk=SIZE       Disk size (default: 50G)

Environment Variables:
  POLIS_DEV_VM_NAME    Override default VM name
  POLIS_DEV_CPUS       Override CPU count
  POLIS_DEV_MEMORY     Override memory size
  POLIS_DEV_DISK       Override disk size

Examples:
  $0 create                           # Create VM with defaults
  $0 create --cpus=8 --memory=16G     # Create with custom resources
  $0 shell                            # Access VM shell
  $0 rebuild                          # Rebuild Polis in VM
  $0 fix-perms                        # Fix file permissions after Docker builds
EOF
}

# Parse arguments
for arg in "$@"; do
    case "$arg" in
        --name=*)
            VM_NAME="${arg#*=}"
            ;;
        --cpus=*)
            VM_CPUS="${arg#*=}"
            ;;
        --memory=*)
            VM_MEMORY="${arg#*=}"
            ;;
        --disk=*)
            VM_DISK="${arg#*=}"
            ;;
    esac
done

# Check if multipass is installed
check_multipass() {
    if ! command -v multipass &>/dev/null; then
        log_error "Multipass is not installed."
        echo ""
        echo "Install Multipass:"
        echo "  Windows:  winget install Canonical.Multipass"
        echo "  macOS:    brew install multipass"
        echo "  Linux:    sudo snap install multipass"
        echo ""
        echo "Visit: https://multipass.run/install"
        return 1
    fi
    return 0
}

# Check if VM exists
vm_exists() {
    multipass list | grep -q "^${VM_NAME}\s"
}

# Create development VM
create_vm() {
    echo ""
    log_step "Creating Polis development VM: ${VM_NAME}"
    echo ""
    
    if vm_exists; then
        log_warn "VM '${VM_NAME}' already exists."
        echo ""
        read -p "Delete and recreate? [y/N] " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            log_info "Aborting."
            return 1
        fi
        delete_vm
    fi
    
    log_info "VM Configuration:"
    echo "  Name:   ${VM_NAME}"
    echo "  CPUs:   ${VM_CPUS}"
    echo "  Memory: ${VM_MEMORY}"
    echo "  Disk:   ${VM_DISK}"
    echo ""
    
    log_info "Launching VM (this takes 5-10 minutes)..."
    multipass launch \
        --name "${VM_NAME}" \
        --cpus "${VM_CPUS}" \
        --memory "${VM_MEMORY}" \
        --disk "${VM_DISK}" \
        --cloud-init "${PROJECT_ROOT}/polis-dev.yaml" \
        "${UBUNTU_VERSION}"
    
    log_info "Waiting for cloud-init to complete..."
    multipass exec "${VM_NAME}" -- cloud-init status --wait
    
    log_success "VM created successfully!"
    echo ""
    
    # Mount project directory
    log_step "Mounting local polis directory..."
    multipass mount "${PROJECT_ROOT}" "${VM_NAME}:/home/ubuntu/polis"
    log_success "Mounted ${PROJECT_ROOT} -> ${VM_NAME}:/home/ubuntu/polis"
    echo ""
    
    # Show next steps
    log_success "Development VM is ready!"
    echo ""
    echo "Next steps:"
    echo "  1. Enter VM:        $0 shell"
    echo "  2. Configure keys:  cd ~/polis && nano .env"
    echo "  3. Build & start:   $0 rebuild"
    echo ""
    echo "Or manually:"
    echo "  multipass shell ${VM_NAME}"
    echo "  cd ~/polis"
    echo "  ./tools/polis.sh init --local"
}

# Start VM
start_vm() {
    if ! vm_exists; then
        log_error "VM '${VM_NAME}' does not exist. Create it first with: $0 create"
        return 1
    fi
    
    log_info "Starting VM '${VM_NAME}'..."
    multipass start "${VM_NAME}"
    log_success "VM started."
}

# Stop VM
stop_vm() {
    if ! vm_exists; then
        log_error "VM '${VM_NAME}' does not exist."
        return 1
    fi
    
    log_info "Stopping VM '${VM_NAME}'..."
    multipass stop "${VM_NAME}"
    log_success "VM stopped."
}

# Delete VM
delete_vm() {
    if ! vm_exists; then
        log_warn "VM '${VM_NAME}' does not exist."
        return 0
    fi
    
    log_info "Deleting VM '${VM_NAME}'..."
    multipass delete "${VM_NAME}"
    multipass purge
    log_success "VM deleted."
}

# Enter VM shell
enter_shell() {
    if ! vm_exists; then
        log_error "VM '${VM_NAME}' does not exist. Create it first with: $0 create"
        return 1
    fi
    
    log_info "Entering VM '${VM_NAME}'..."
    multipass shell "${VM_NAME}"
}

# Show VM status
show_status() {
    if ! vm_exists; then
        log_warn "VM '${VM_NAME}' does not exist."
        return 1
    fi
    
    multipass info "${VM_NAME}"
}

# Mount project directory
mount_project() {
    if ! vm_exists; then
        log_error "VM '${VM_NAME}' does not exist."
        return 1
    fi
    
    log_info "Mounting ${PROJECT_ROOT} -> ${VM_NAME}:/home/ubuntu/polis"
    multipass mount "${PROJECT_ROOT}" "${VM_NAME}:/home/ubuntu/polis"
    log_success "Mounted successfully."
}

# Unmount project directory
unmount_project() {
    if ! vm_exists; then
        log_error "VM '${VM_NAME}' does not exist."
        return 1
    fi
    
    log_info "Unmounting polis directory..."
    multipass unmount "${VM_NAME}:/home/ubuntu/polis"
    log_success "Unmounted successfully."
}

# Rebuild Polis from source
rebuild_polis() {
    if ! vm_exists; then
        log_error "VM '${VM_NAME}' does not exist."
        return 1
    fi
    
    log_info "Rebuilding Polis from source in VM..."
    echo ""
    
    multipass exec "${VM_NAME}" -- bash -c "
        cd ~/polis
        ./tools/polis.sh down 2>/dev/null || true
        ./tools/polis.sh init --local --no-cache
    "
    
    log_success "Rebuild complete!"
    echo ""
    echo "Get access token with: $0 shell"
    echo "Then run: ./tools/polis.sh openclaw init"
}

# Fix file permissions in mounted directory
fix_permissions() {
    if ! vm_exists; then
        log_error "VM '${VM_NAME}' does not exist."
        return 1
    fi
    
    log_info "Fixing file ownership in mounted directory..."
    echo ""
    
    # Change ownership of all files in ~/polis to ubuntu:ubuntu
    multipass exec "${VM_NAME}" -- sudo chown -R ubuntu:ubuntu /home/ubuntu/polis
    
    log_success "Permissions fixed!"
    echo ""
    log_info "All files in the mounted directory are now owned by the VM user."
    log_info "You should be able to edit them from your host without issues."
}

# Main command dispatcher
case "${1:-}" in
    create)
        check_multipass || exit 1
        create_vm
        ;;
    start)
        check_multipass || exit 1
        start_vm
        ;;
    stop)
        check_multipass || exit 1
        stop_vm
        ;;
    delete)
        check_multipass || exit 1
        delete_vm
        ;;
    shell)
        check_multipass || exit 1
        enter_shell
        ;;
    status)
        check_multipass || exit 1
        show_status
        ;;
    mount)
        check_multipass || exit 1
        mount_project
        ;;
    unmount)
        check_multipass || exit 1
        unmount_project
        ;;
    rebuild)
        check_multipass || exit 1
        rebuild_polis
        ;;
    fix-perms)
        check_multipass || exit 1
        fix_permissions
        ;;
    help|--help|-h)
        show_usage
        ;;
    *)
        show_usage
        exit 1
        ;;
esac
