#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BATS="${SCRIPT_DIR}/bats/bats-core/bin/bats"

usage() {
    cat <<EOF
Usage: $0 [OPTIONS] <tier>

Tiers:
  unit           Run unit tests (~30s, no Docker)
  integration    Run integration tests (~3min, needs containers)
  e2e            Run E2E tests (~10min, needs containers + network)
  packer         Run Packer unit tests (~10s, no Docker)
  docker         Run Docker/Compose linting (~10s, no containers)
  all            Run all tiers (excludes packer, docker)

Options:
  --ci           CI mode (auto-start httpbin, reset test state)
  --filter-tags  Filter by bats file_tags (e.g. "security")
  -h, --help     Show this help
EOF
}

CI_MODE=false
FILTER_TAGS=""
TIER=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --ci)         CI_MODE=true; shift ;;
        --filter-tags) FILTER_TAGS="$2"; shift 2 ;;
        -h|--help)    usage; exit 0 ;;
        *)            TIER="$1"; shift ;;
    esac
done

[[ -n "$TIER" ]] || { usage; exit 1; }

BATS_ARGS=(--recursive --timing)
[[ -n "$FILTER_TAGS" ]] && BATS_ARGS+=(--filter-tags "$FILTER_TAGS")

run_tier() {
    local dir="$1"
    [[ -d "${SCRIPT_DIR}/${dir}" ]] || { echo "No tests in ${dir}"; return 0; }
    "$BATS" "${BATS_ARGS[@]}" "${SCRIPT_DIR}/${dir}"
}

if [[ "$CI_MODE" == "true" ]]; then
    # Reset test state before run
    source "${SCRIPT_DIR}/lib/constants.bash"
    source "${SCRIPT_DIR}/lib/guards.bash"
    reset_test_state 2>/dev/null || true
fi

case "$TIER" in
    unit)        run_tier "unit" ;;
    integration) run_tier "integration" ;;
    packer)      run_tier "unit/packer" ;;
    docker)      run_tier "unit/docker" ;;
    e2e)
        docker compose --profile test pull httpbin 2>/dev/null || true
        docker compose --profile test up -d httpbin
        trap 'docker compose --profile test rm -sf httpbin 2>/dev/null || true' EXIT
        run_tier "e2e"
        ;;
    all)
        docker compose --profile test pull httpbin 2>/dev/null || true
        docker compose --profile test up -d httpbin
        trap 'docker compose --profile test rm -sf httpbin 2>/dev/null || true' EXIT
        run_tier "unit"
        run_tier "integration"
        run_tier "e2e"
        ;;
    *) echo "Unknown tier: $TIER"; usage; exit 1 ;;
esac
