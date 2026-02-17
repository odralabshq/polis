#!/bin/bash
# dev-vm.sh - Polis Development VM Management
# Consolidated from polis-dev.sh and setup-vm.sh

set -euo pipefail

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

log_info() { echo -e "${BLUE}[INFO]${NC} $*"; }
log_success() { echo -e "${GREEN}[OK]${NC} $*"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }
log_error() { echo -e "${RED}[ERROR]${NC} $*" >&2; }
log_step() { echo -e "${CYAN}[STEP]${NC} $*"; }

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# ============================================================================
# Input Validation
# ============================================================================

validate_name() {
    local name="$1"
    if [[ ! "${name}" =~ ^[a-zA-Z0-9_-]+$ ]] || [[ ${#name} -gt 63 ]]; then
        log_error "Invalid VM name '${name}' (alphanumeric, -, _ only; max 63 chars)"
        exit 1
    fi
}

validate_resource() {
    local value="$1" pattern="$2" label="$3"
    if [[ ! "${value}" =~ ${pattern} ]]; then
        log_error "Invalid ${label}: '${value}'"
        exit 1
    fi
}

# ============================================================================
# Defaults & Validation
# ============================================================================

VM_NAME="${POLIS_VM_NAME:-polis-dev}"
VM_CPUS="${POLIS_VM_CPUS:-4}"
VM_MEMORY="${POLIS_VM_MEMORY:-8G}"
VM_DISK="${POLIS_VM_DISK:-50G}"
UBUNTU_VERSION="24.04"

# Parse CLI overrides
for arg in "$@"; do
    case "$arg" in
        --name=*)   VM_NAME="${arg#*=}" ;;
        --cpus=*)   VM_CPUS="${arg#*=}" ;;
        --memory=*) VM_MEMORY="${arg#*=}" ;;
        --disk=*)   VM_DISK="${arg#*=}" ;;
    esac
done

# Validate all inputs
validate_name "${VM_NAME}"
validate_resource "${VM_CPUS}" '^[0-9]+$' "CPU count"
validate_resource "${VM_MEMORY}" '^[0-9]+[GMK]$' "memory"
validate_resource "${VM_DISK}" '^[0-9]+[GMK]$' "disk"

# ============================================================================
# Dependency Checks
# ============================================================================

check_multipass() {
    if ! command -v multipass &>/dev/null; then
        log_error "Multipass is not installed."
        echo ""
        echo "Install Multipass:"
        echo "  macOS:   brew install multipass"
        echo "  Linux:   sudo snap install multipass"
        echo "  Windows: winget install Canonical.Multipass"
        echo ""
        echo "Visit: https://multipass.run/install"
        exit 1
    fi
}

check_jq() {
    if ! command -v jq &>/dev/null; then
        log_error "jq is required for ssh-config command"
        exit 1
    fi
}

vm_exists() {
    multipass list 2>/dev/null | grep -q "^${VM_NAME}\s"
}

# ============================================================================
# Commands
# ============================================================================

cmd_create() {
    log_step "Creating Polis development VM: ${VM_NAME}"
    
    if vm_exists; then
        log_warn "VM '${VM_NAME}' already exists."
        read -p "Delete and recreate? [y/N] " -n 1 -r
        echo
        [[ ! $REPLY =~ ^[Yy]$ ]] && { log_info "Aborting."; exit 1; }
        cmd_delete
    fi
    
    log_info "VM Configuration: name=${VM_NAME} cpus=${VM_CPUS} memory=${VM_MEMORY} disk=${VM_DISK}"
    
    multipass launch \
        --name "${VM_NAME}" \
        --cpus "${VM_CPUS}" \
        --memory "${VM_MEMORY}" \
        --disk "${VM_DISK}" \
        --cloud-init "${PROJECT_ROOT}/polis-dev.yaml" \
        "${UBUNTU_VERSION}"
    
    log_info "Waiting for cloud-init..."
    multipass exec "${VM_NAME}" -- cloud-init status --wait
    
    log_step "Mounting project directory..."
    multipass mount "${PROJECT_ROOT}" "${VM_NAME}:/home/ubuntu/polis"
    
    log_success "VM ready! Run: $0 shell"
}

cmd_start() {
    vm_exists || { log_error "VM '${VM_NAME}' does not exist."; exit 1; }
    log_info "Starting VM '${VM_NAME}'..."
    multipass start "${VM_NAME}"
    log_success "VM started."
}

cmd_stop() {
    vm_exists || { log_error "VM '${VM_NAME}' does not exist."; exit 1; }
    log_info "Stopping VM '${VM_NAME}'..."
    multipass stop "${VM_NAME}"
    log_success "VM stopped."
}

cmd_delete() {
    vm_exists || { log_warn "VM '${VM_NAME}' does not exist."; return 0; }
    log_info "Deleting VM '${VM_NAME}'..."
    multipass delete "${VM_NAME}"
    multipass purge
    log_success "VM deleted."
}

cmd_shell() {
    vm_exists || { log_error "VM '${VM_NAME}' does not exist."; exit 1; }
    multipass shell "${VM_NAME}"
}

cmd_status() {
    vm_exists || { log_warn "VM '${VM_NAME}' does not exist."; exit 1; }
    multipass info "${VM_NAME}"
}

cmd_mount() {
    vm_exists || { log_error "VM '${VM_NAME}' does not exist."; exit 1; }
    multipass mount "${PROJECT_ROOT}" "${VM_NAME}:/home/ubuntu/polis"
    log_success "Mounted."
}

cmd_unmount() {
    vm_exists || { log_error "VM '${VM_NAME}' does not exist."; exit 1; }
    multipass unmount "${VM_NAME}:/home/ubuntu/polis"
    log_success "Unmounted."
}

cmd_rebuild() {
    vm_exists || { log_error "VM '${VM_NAME}' does not exist."; exit 1; }
    log_info "Rebuilding Polis..."
    multipass exec "${VM_NAME}" -- bash -c "
        cd ~/polis
        ./cli/polis.sh down 2>/dev/null || true
        ./cli/polis.sh init --local --no-cache
    "
    log_success "Rebuild complete!"
}

cmd_fix_perms() {
    vm_exists || { log_error "VM '${VM_NAME}' does not exist."; exit 1; }
    log_info "Fixing permissions..."
    multipass exec "${VM_NAME}" -- sudo chown -R ubuntu:ubuntu /home/ubuntu/polis
    log_success "Permissions fixed."
}

cmd_ssh_config() {
    check_jq
    vm_exists || { log_error "VM '${VM_NAME}' does not exist."; exit 1; }
    
    local ip
    ip=$(multipass info "${VM_NAME}" --format json | jq -r ".info.\"${VM_NAME}\".ipv4[0]")
    
    cat <<EOF
Host ${VM_NAME}
    HostName ${ip}
    User ubuntu
    StrictHostKeyChecking no
    UserKnownHostsFile /dev/null
EOF
}

show_usage() {
    cat <<EOF
${BOLD}Polis Development VM${NC}

Usage: $0 <command> [options]

Commands:
  create      Create development VM
  start       Start VM
  stop        Stop VM
  delete      Delete VM
  shell       Enter VM shell
  status      Show VM status
  mount       Mount project directory
  unmount     Unmount project directory
  rebuild     Rebuild Polis in VM
  fix-perms   Fix file ownership
  ssh-config  Print SSH config for VS Code

Options:
  --name=NAME     VM name (default: polis-dev)
  --cpus=N        CPU count (default: 4)
  --memory=SIZE   Memory (default: 8G)
  --disk=SIZE     Disk (default: 50G)

Environment:
  POLIS_VM_NAME   Override VM name
  POLIS_VM_CPUS   Override CPU count
  POLIS_VM_MEMORY Override memory
  POLIS_VM_DISK   Override disk
EOF
}

# ============================================================================
# Main
# ============================================================================

case "${1:-}" in
    help|--help|-h) show_usage; exit 0 ;;
esac

check_multipass

case "${1:-}" in
    create)    cmd_create ;;
    start)     cmd_start ;;
    stop)      cmd_stop ;;
    delete)    cmd_delete ;;
    shell)     cmd_shell ;;
    status)    cmd_status ;;
    mount)     cmd_mount ;;
    unmount)   cmd_unmount ;;
    rebuild)   cmd_rebuild ;;
    fix-perms) cmd_fix_perms ;;
    ssh-config) cmd_ssh_config ;;
    *) show_usage; exit 1 ;;
esac
