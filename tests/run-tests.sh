#!/bin/bash
# Polis Core Test Runner
# Usage: ./run-tests.sh [options] [test-category|test-file]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
BATS_DIR="${SCRIPT_DIR}/bats"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Default options
VERBOSE=""
TAP_OUTPUT=""
FILTER=""
JOBS=""

usage() {
    cat << EOF
Polis Core Test Suite Runner

Usage: $0 [options] [target]

Targets:
    unit            Run unit tests only
    integration     Run integration tests only
    e2e             Run end-to-end tests only
    all             Run all tests (default)
    <file.bats>     Run specific test file

Options:
    -v, --verbose       Verbose output
    -t, --tap           TAP output format (for CI)
    -f, --filter REGEX  Filter tests by name
    -j, --jobs N        Run N tests in parallel
    -h, --help          Show this help

Examples:
    $0                      # Run all tests
    $0 unit                 # Run unit tests
    $0 integration/network  # Run network integration tests
    $0 -v e2e               # Run e2e tests with verbose output
    $0 --tap all            # Run all tests with TAP output
EOF
    exit 0
}

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Install BATS if not present
install_bats() {
    if [[ ! -d "${BATS_DIR}/bats-core" ]]; then
        log_info "Installing BATS testing framework..."
        mkdir -p "${BATS_DIR}"
        
        # Clone bats-core
        git clone --depth 1 https://github.com/bats-core/bats-core.git "${BATS_DIR}/bats-core" 2>/dev/null || {
            log_error "Failed to clone bats-core"
            exit 1
        }
        
        # Clone bats-support
        git clone --depth 1 https://github.com/bats-core/bats-support.git "${BATS_DIR}/bats-support" 2>/dev/null || {
            log_error "Failed to clone bats-support"
            exit 1
        }
        
        # Clone bats-assert
        git clone --depth 1 https://github.com/bats-core/bats-assert.git "${BATS_DIR}/bats-assert" 2>/dev/null || {
            log_error "Failed to clone bats-assert"
            exit 1
        }
        
        # Clone bats-file
        git clone --depth 1 https://github.com/bats-core/bats-file.git "${BATS_DIR}/bats-file" 2>/dev/null || {
            log_error "Failed to clone bats-file"
            exit 1
        }
        
        log_info "BATS installed successfully"
    fi
}

# Check prerequisites
check_prerequisites() {
    # Check Docker
    if ! command -v docker &> /dev/null; then
        log_error "Docker is not installed"
        exit 1
    fi
    
    # Check Docker Compose
    if ! docker compose version &> /dev/null; then
        log_error "Docker Compose is not installed"
        exit 1
    fi
    
    # Check containers are running (warn only)
    if ! docker ps --format '{{.Names}}' | grep -q polis-v2; then
        log_warn "Polis containers not running. Run '../tools/polis.sh up' first for full test coverage."
    fi
}

# Parse arguments
parse_args() {
    local target="all"
    
    while [[ $# -gt 0 ]]; do
        case "$1" in
            -v|--verbose)
                VERBOSE="--verbose-run"
                shift
                ;;
            -t|--tap)
                TAP_OUTPUT="--formatter tap"
                shift
                ;;
            -f|--filter)
                FILTER="--filter $2"
                shift 2
                ;;
            -j|--jobs)
                JOBS="--jobs $2"
                shift 2
                ;;
            -h|--help)
                usage
                ;;
            unit|integration|e2e|all)
                target="$1"
                shift
                ;;
            *.bats)
                target="$1"
                shift
                ;;
            *)
                log_error "Unknown option: $1"
                usage
                ;;
        esac
    done
    
    echo "$target"
}

# Run tests
run_tests() {
    local target="$1"
    local bats_cmd="${BATS_DIR}/bats-core/bin/bats"
    local test_dirs=()
    
    # Determine test directories (use relative paths)
    case "$target" in
        all)
            test_dirs=("unit" "integration" "e2e")
            ;;
        unit)
            test_dirs=("unit")
            ;;
        integration)
            test_dirs=("integration")
            ;;
        e2e)
            test_dirs=("e2e")
            ;;
        *)
            # Specific file or directory
            if [[ -f "${SCRIPT_DIR}/${target}" ]]; then
                test_dirs=("${target}")
            elif [[ -f "${SCRIPT_DIR}/${target}.bats" ]]; then
                test_dirs=("${target}.bats")
            elif [[ -d "${SCRIPT_DIR}/${target}" ]]; then
                test_dirs=("${target}")
            else
                log_error "Test target not found: $target"
                exit 1
            fi
            ;;
    esac
    
    # Build bats command
    local cmd="${bats_cmd} --recursive ${VERBOSE} ${TAP_OUTPUT} ${FILTER} ${JOBS}"
    
    log_info "Running tests: ${test_dirs[*]}"
    echo ""
    
    # Export paths for helpers
    export BATS_LIB_PATH="${BATS_DIR}"
    export PROJECT_ROOT
    export SCRIPT_DIR
    
    # Run from SCRIPT_DIR for relative paths in output
    cd "${SCRIPT_DIR}"
    ${cmd} "${test_dirs[@]}"
}

# Main
main() {
    cd "${SCRIPT_DIR}"
    
    install_bats
    check_prerequisites
    
    local target
    target=$(parse_args "$@")
    
    echo ""
    echo "=========================================="
    echo "  Polis Core Test Suite"
    echo "=========================================="
    echo ""
    
    run_tests "$target"
    
    local exit_code=$?
    
    echo ""
    if [[ $exit_code -eq 0 ]]; then
        log_info "All tests passed!"
    else
        log_error "Some tests failed (exit code: $exit_code)"
    fi
    
    exit $exit_code
}

main "$@"
