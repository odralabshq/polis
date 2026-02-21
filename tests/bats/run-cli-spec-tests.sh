#!/bin/bash
# =============================================================================
# Polis CLI Spec Test Runner
# =============================================================================
# Usage: ./run-cli-spec-tests.sh [--filter <pattern>] [--verbose]
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
BATS_DIR="${SCRIPT_DIR}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log_info() { echo -e "${BLUE}[INFO]${NC} $*"; }
log_ok()   { echo -e "${GREEN}[OK]${NC} $*"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }
log_error(){ echo -e "${RED}[ERROR]${NC} $*" >&2; }

# Parse arguments
FILTER=""
VERBOSE=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --filter)   FILTER="$2"; shift 2 ;;
        --filter=*) FILTER="${1#*=}"; shift ;;
        --verbose|-v) VERBOSE="--verbose-run"; shift ;;
        -h|--help)
            echo "Usage: $0 [--filter <pattern>] [--verbose]"
            echo ""
            echo "Options:"
            echo "  --filter <pattern>  Run only tests matching pattern"
            echo "  --verbose, -v       Show verbose output"
            echo ""
            echo "Examples:"
            echo "  $0                          # Run all tests"
            echo "  $0 --filter 'start:'        # Run only start tests"
            echo "  $0 --filter 'multipass:'    # Run only multipass verification tests"
            echo "  $0 --filter 'config'        # Run config-related tests"
            exit 0
            ;;
        *)
            log_error "Unknown option: $1"
            exit 1
            ;;
    esac
done

# Check prerequisites
check_prerequisites() {
    log_info "Checking prerequisites..."
    
    # Check bats
    if [[ ! -x "${BATS_DIR}/bats-core/bin/bats" ]]; then
        log_error "BATS not found at ${BATS_DIR}/bats-core/bin/bats"
        exit 1
    fi
    
    # Check polis binary
    if [[ ! -x "${HOME}/.polis/bin/polis" ]]; then
        log_error "polis binary not found. Run: ./scripts/install-dev.sh"
        exit 1
    fi
    
    # Check multipass
    if ! command -v multipass &>/dev/null; then
        log_error "multipass not found"
        exit 1
    fi
    
    # Check jq (needed for JSON parsing)
    if ! command -v jq &>/dev/null; then
        log_error "jq not found. Install with: sudo apt install jq"
        exit 1
    fi
    
    log_ok "Prerequisites OK"
}

# Setup environment
setup_env() {
    log_info "Setting up test environment..."
    
    # Find dev image
    DEV_IMAGE=$(find "${REPO_DIR}/packer/output" -name "*.qcow2" 2>/dev/null | sort | tail -1)
    if [[ -z "${DEV_IMAGE}" ]]; then
        log_error "No dev image found in ${REPO_DIR}/packer/output"
        log_error "Build with: just build-vm"
        exit 1
    fi
    export POLIS_DEV_IMAGE="${DEV_IMAGE}"
    
    # Find dev public key
    DEV_PUB_KEY="${REPO_DIR}/.secrets/polis-release.pub"
    if [[ -f "${DEV_PUB_KEY}" ]]; then
        export POLIS_DEV_PUB_KEY="${DEV_PUB_KEY}"
        export POLIS_VERIFYING_KEY_B64=$(base64 -w0 "${DEV_PUB_KEY}")
    else
        log_warn "Dev public key not found at ${DEV_PUB_KEY}"
    fi
    
    log_ok "Environment configured"
    log_info "  Image: ${DEV_IMAGE}"
}

# Run tests
run_tests() {
    log_info "Running CLI spec tests..."
    echo ""
    
    local bats_args=()
    bats_args+=("--tap")
    
    if [[ -n "${VERBOSE}" ]]; then
        bats_args+=("${VERBOSE}")
    fi
    
    if [[ -n "${FILTER}" ]]; then
        bats_args+=("--filter" "${FILTER}")
    fi
    
    "${BATS_DIR}/bats-core/bin/bats" "${bats_args[@]}" "${BATS_DIR}/cli-spec.bats"
}

# Main
main() {
    echo ""
    echo "╔═══════════════════════════════════════════════════════════════╗"
    echo "║           Polis CLI Specification Test Suite                  ║"
    echo "╚═══════════════════════════════════════════════════════════════╝"
    echo ""
    
    check_prerequisites
    setup_env
    echo ""
    run_tests
}

main "$@"
