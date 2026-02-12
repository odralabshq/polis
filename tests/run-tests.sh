#!/bin/bash
# Polis Test Runner
#
# Usage:
#   ./run-tests.sh [options] [target]
#
# Targets:
#   unit            Static tests only (no containers needed)
#   integration     Container integration tests
#   e2e             Full end-to-end tests
#   all             Everything (default)
#   <file.bats>     Specific test file
#
# Options:
#   --ci            Full lifecycle: build → up → test → down
#   --up            Start containers and wait for healthy
#   --down          Stop containers
#   -v, --verbose   Verbose bats output
#   -t, --tap       TAP output (for CI parsers)
#   -f, --filter    Filter tests by name regex
#   -j, --jobs N    Parallel test jobs
#
# Examples:
#   ./run-tests.sh unit                    # No Docker needed at all
#   ./run-tests.sh integration             # Auto-starts containers if needed
#   ./run-tests.sh --ci all                # CI: start → test → stop
#   ./run-tests.sh --up                    # Just start containers
#   POLIS_TEST_NO_START=1 ./run-tests.sh   # Skip auto-start, skip missing

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
COMPOSE_FILE="${PROJECT_ROOT}/docker-compose.yml"
BATS_DIR="${SCRIPT_DIR}/bats"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Options
CI_MODE=""
DO_UP=""
DO_DOWN=""
BATS_OPTS=""
TARGET="all"

log_info()  { echo -e "${GREEN}[INFO]${NC} $1"; }
log_warn()  { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

usage() {
    sed -n '2,/^$/s/^# \?//p' "$0"
    exit 0
}

install_bats() {
    # Bats is now managed as git submodules - no installation needed
    # Submodules are cloned with: git clone --recursive
    # Or initialized with: git submodule update --init --recursive
    [[ -d "${BATS_DIR}/bats-core" ]] && return
    log_error "BATS submodules not initialized. Run: git submodule update --init --recursive"
    exit 1
}

compose_up() {
    log_info "Starting containers..."
    docker compose -f "$COMPOSE_FILE" up -d --build 2>&1
    log_info "Waiting for containers to be healthy..."
    local timeout=180 elapsed=0
    while [[ $elapsed -lt $timeout ]]; do
        local ok=true
        for c in polis-dns polis-gateway polis-icap; do
            local h
            h=$(docker inspect --format '{{.State.Health.Status}}' "$c" 2>/dev/null || echo "missing")
            [[ "$h" == "healthy" ]] || { ok=false; break; }
        done
        if [[ "$ok" == "true" ]]; then
            log_info "All containers healthy (${elapsed}s)"
            return 0
        fi
        sleep 5
        elapsed=$((elapsed + 5))
    done
    log_warn "Health timeout after ${timeout}s — some tests may fail"
}

compose_down() {
    log_info "Stopping containers..."
    docker compose -f "$COMPOSE_FILE" down 2>&1
}

parse_args() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --ci)       CI_MODE=1; shift ;;
            --up)       DO_UP=1; shift ;;
            --down)     DO_DOWN=1; shift ;;
            -v|--verbose) BATS_OPTS+=" --verbose-run"; shift ;;
            -t|--tap)     BATS_OPTS+=" --formatter tap"; shift ;;
            -f|--filter)  BATS_OPTS+=" --filter $2"; shift 2 ;;
            -j|--jobs)    BATS_OPTS+=" --jobs $2"; shift 2 ;;
            -h|--help)    usage ;;
            unit|integration|e2e|all) TARGET="$1"; shift ;;
            *.bats)       TARGET="$1"; shift ;;
            *)            log_error "Unknown: $1"; usage ;;
        esac
    done
}

run_tests() {
    local bats="${BATS_DIR}/bats-core/bin/bats"
    local dirs=()

    case "$TARGET" in
        all)          dirs=(unit integration e2e) ;;
        unit)         dirs=(unit) ;;
        integration)  dirs=(integration) ;;
        e2e)          dirs=(e2e) ;;
        *)
            if [[ -f "${SCRIPT_DIR}/${TARGET}" ]]; then
                dirs=("${TARGET}")
            elif [[ -f "${SCRIPT_DIR}/${TARGET}.bats" ]]; then
                dirs=("${TARGET}.bats")
            elif [[ -d "${SCRIPT_DIR}/${TARGET}" ]]; then
                dirs=("${TARGET}")
            else
                log_error "Not found: ${TARGET}"; exit 1
            fi ;;
    esac

    export BATS_LIB_PATH="${BATS_DIR}"
    export PROJECT_ROOT SCRIPT_DIR

    cd "${SCRIPT_DIR}"
    log_info "Running: ${dirs[*]}"
    echo ""
    # shellcheck disable=SC2086
    ${bats} --recursive ${BATS_OPTS} "${dirs[@]}"
}

main() {
    parse_args "$@"
    install_bats

    # --up only: start and exit
    if [[ -n "$DO_UP" && -z "$CI_MODE" ]]; then
        compose_up; exit 0
    fi

    # --down only: stop and exit
    if [[ -n "$DO_DOWN" && -z "$CI_MODE" ]]; then
        compose_down; exit 0
    fi

    # --ci: full lifecycle
    if [[ -n "$CI_MODE" ]]; then
        compose_up
        trap 'compose_down' EXIT
    fi

    echo ""
    echo "=========================================="
    echo "  Polis Test Suite"
    echo "=========================================="
    echo ""

    run_tests
}

main "$@"
