# Polis Test Suite Setup
# Runs once before all tests in the suite.
# Automatically starts containers if not running.
#
# Environment:
#   POLIS_TEST_NO_START=1   — Skip auto-start (tests will be skipped if containers missing)
#   POLIS_TEST_TEARDOWN=1   — Tear down containers after tests (for CI)

export PROJECT_ROOT="${PROJECT_ROOT:-$(cd "$(dirname "${BATS_TEST_FILENAME}")/.." && pwd)}"
export COMPOSE_FILE="${PROJECT_ROOT}/docker-compose.yml"

# Container names (must match common.bash)
export DNS_CONTAINER="polis-dns"
export GATEWAY_CONTAINER="polis-gateway"
export ICAP_CONTAINER="polis-icap"
export WORKSPACE_CONTAINER="polis-workspace"

# Track whether we started containers (for teardown decision)
export _POLIS_STARTED_BY_TESTS="false"

_containers_running() {
    for c in "$DNS_CONTAINER" "$GATEWAY_CONTAINER" "$ICAP_CONTAINER" "$WORKSPACE_CONTAINER"; do
        docker ps --format '{{.Names}}' | grep -q "^${c}$" || return 1
    done
}

_wait_healthy() {
    local timeout="${1:-180}" elapsed=0
    while [[ $elapsed -lt $timeout ]]; do
        local ok=true
        for c in "$DNS_CONTAINER" "$GATEWAY_CONTAINER" "$ICAP_CONTAINER"; do
            local h
            h=$(docker inspect --format '{{.State.Health.Status}}' "$c" 2>/dev/null || echo "missing")
            [[ "$h" == "healthy" ]] || { ok=false; break; }
        done
        [[ "$ok" == "true" ]] && { echo "# All containers healthy (${elapsed}s)" >&3; return 0; }
        sleep 5
        elapsed=$((elapsed + 5))
    done
    echo "# WARNING: Health timeout after ${timeout}s" >&3
    return 1
}

setup_suite() {
    echo "# Polis test suite starting..." >&3

    if _containers_running; then
        echo "# Containers already running" >&3
        _wait_healthy 60
        return
    fi

    if [[ "${POLIS_TEST_NO_START:-0}" == "1" ]]; then
        echo "# Containers not running. POLIS_TEST_NO_START=1 set, skipping auto-start." >&3
        return
    fi

    echo "# Containers not running — starting..." >&3
    docker compose -f "$COMPOSE_FILE" up -d 2>&1 | sed 's/^/# /' >&3
    _POLIS_STARTED_BY_TESTS="true"
    export _POLIS_STARTED_BY_TESTS
    _wait_healthy 180
}

teardown_suite() {
    if [[ "${POLIS_TEST_TEARDOWN:-0}" == "1" && "$_POLIS_STARTED_BY_TESTS" == "true" ]]; then
        echo "# Tearing down containers..." >&3
        docker compose -f "$COMPOSE_FILE" down 2>&1 | sed 's/^/# /' >&3
    fi
    echo "# Suite complete" >&3
}
