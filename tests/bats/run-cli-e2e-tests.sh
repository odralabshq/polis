#!/bin/bash
# =============================================================================
# Polis CLI E2E Test Runner
# =============================================================================
# Usage: ./run-cli-e2e-tests.sh [--filter <pattern>] [--verbose] [--suite <name>]
#
# Suites (run in order by default):
#   offline    — no VM needed (help, version, flags, error cases)
#   e2e        — original tests (doctor, exec, agent, security)
#   live       — needs running VM (status, connect, security CRUD, agent lifecycle)
#   lifecycle  — DESTRUCTIVE (stop, delete, start, update)
#
# Examples:
#   ./run-cli-e2e-tests.sh                        # Run offline + e2e + live (safe)
#   ./run-cli-e2e-tests.sh --suite all             # Run everything including lifecycle
#   ./run-cli-e2e-tests.sh --suite lifecycle        # Run only lifecycle tests
#   ./run-cli-e2e-tests.sh --filter 'security'     # Run matching tests across suites
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BATS_DIR="${SCRIPT_DIR}"
BATS_BIN="${BATS_DIR}/bats-core/bin/bats"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log_info()  { echo -e "${BLUE}[INFO]${NC} $*"; }
log_ok()    { echo -e "${GREEN}[OK]${NC} $*"; }
log_warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
log_error() { echo -e "${RED}[ERROR]${NC} $*" >&2; }

FILTER=""
VERBOSE=""
SUITE="safe"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --filter)     FILTER="$2"; shift 2 ;;
        --filter=*)   FILTER="${1#*=}"; shift ;;
        --verbose|-v) VERBOSE="--verbose-run"; shift ;;
        --suite)      SUITE="$2"; shift 2 ;;
        --suite=*)    SUITE="${1#*=}"; shift ;;
        -h|--help)
            echo "Usage: $0 [--filter <pattern>] [--verbose] [--suite <name>]"
            echo ""
            echo "Suites:"
            echo "  safe       Run offline + e2e + live (default, non-destructive)"
            echo "  all        Run all 4 test files including lifecycle (destructive)"
            echo "  offline    Only cli-offline.bats"
            echo "  e2e        Only cli-e2e.bats"
            echo "  live       Only cli-live.bats"
            echo "  lifecycle  Only cli-lifecycle.bats (DESTRUCTIVE)"
            echo ""
            echo "Options:"
            echo "  --filter <pattern>  Run only tests matching pattern"
            echo "  --verbose, -v       Show verbose output"
            exit 0
            ;;
        *)
            log_error "Unknown option: $1"
            exit 1
            ;;
    esac
done

check_prerequisites() {
    log_info "Checking prerequisites..."

    if [[ ! -x "${BATS_BIN}" ]]; then
        log_error "BATS not found at ${BATS_BIN}"
        exit 1
    fi

    if [[ ! -x "${HOME}/.polis/bin/polis" ]]; then
        log_error "polis binary not found at ~/.polis/bin/polis"
        exit 1
    fi

    if ! command -v jq &>/dev/null; then
        log_error "jq not found. Install with: sudo apt install jq"
        exit 1
    fi

    log_ok "Prerequisites OK"

    if command -v multipass &>/dev/null && multipass info polis &>/dev/null 2>&1; then
        local state
        state=$(multipass info polis --format json 2>/dev/null | jq -r '.info.polis.state // "Unknown"')
        log_info "Workspace state: ${state}"
        if [[ "${state}" != "Running" ]]; then
            log_warn "Workspace is not running — live tests will be skipped"
        fi
    else
        log_warn "No workspace found — live/lifecycle tests will be skipped"
    fi
}

run_bats_file() {
    local file="$1"
    local label="$2"

    if [[ ! -f "${file}" ]]; then
        log_error "Test file not found: ${file}"
        return 1
    fi

    echo ""
    log_info "━━━ ${label} ━━━"

    local bats_args=("--tap")
    [[ -n "${VERBOSE}" ]] && bats_args+=("${VERBOSE}")
    [[ -n "${FILTER}" ]]  && bats_args+=("--filter" "${FILTER}")

    "${BATS_BIN}" "${bats_args[@]}" "${file}"
}

build_file_list() {
    local files=()
    case "${SUITE}" in
        safe)
            files=(
                "${BATS_DIR}/cli-offline.bats"
                "${BATS_DIR}/cli-e2e.bats"
                "${BATS_DIR}/cli-live.bats"
            )
            ;;
        all)
            files=(
                "${BATS_DIR}/cli-offline.bats"
                "${BATS_DIR}/cli-e2e.bats"
                "${BATS_DIR}/cli-live.bats"
                "${BATS_DIR}/cli-lifecycle.bats"
            )
            ;;
        offline)   files=("${BATS_DIR}/cli-offline.bats") ;;
        e2e)       files=("${BATS_DIR}/cli-e2e.bats") ;;
        live)      files=("${BATS_DIR}/cli-live.bats") ;;
        lifecycle) files=("${BATS_DIR}/cli-lifecycle.bats") ;;
        *)
            log_error "Unknown suite: ${SUITE}"
            log_error "Valid suites: safe, all, offline, e2e, live, lifecycle"
            exit 1
            ;;
    esac
    printf '%s\n' "${files[@]}"
}

echo ""
echo "╔═══════════════════════════════════════════════════════════════╗"
echo "║              Polis CLI E2E Test Suite                        ║"
echo "╚═══════════════════════════════════════════════════════════════╝"
echo ""

check_prerequisites

FAILED=0
while IFS= read -r file; do
    label="$(basename "${file}" .bats)"
    if ! run_bats_file "${file}" "${label}"; then
        FAILED=1
    fi
done < <(build_file_list)

echo ""
if [[ "${FAILED}" -eq 0 ]]; then
    log_ok "All test suites passed"
else
    log_error "Some tests failed"
    exit 1
fi
