# Polis Core Test Suite Setup
# This file runs once before all tests in the suite

# Export common variables
export PROJECT_ROOT="${PROJECT_ROOT:-$(cd "$(dirname "${BATS_TEST_FILENAME}")/.." && pwd)}"
export GATEWAY_CONTAINER="polis-gateway"
export ICAP_CONTAINER="polis-icap"
export WORKSPACE_CONTAINER="polis-workspace"

setup_suite() {
    echo "# Setting up test suite..." >&3
    
    # Check if containers are running
    local containers_running=true
    
    if ! docker ps --format '{{.Names}}' | grep -q "${GATEWAY_CONTAINER}"; then
        containers_running=false
    fi
    if ! docker ps --format '{{.Names}}' | grep -q "${ICAP_CONTAINER}"; then
        containers_running=false
    fi
    if ! docker ps --format '{{.Names}}' | grep -q "${WORKSPACE_CONTAINER}"; then
        containers_running=false
    fi
    
    if [[ "$containers_running" == "false" ]]; then
        echo "# WARNING: Not all containers are running. Some tests will be skipped." >&3
        echo "# Run '../tools/polis.sh up' to start containers for full test coverage." >&3
    else
        echo "# All containers running. Waiting for health checks..." >&3
        
        # Wait for containers to be healthy (max 60 seconds)
        local timeout=60
        local elapsed=0
        
        while [[ $elapsed -lt $timeout ]]; do
            local gateway_health icap_health workspace_health
            gateway_health=$(docker inspect --format '{{.State.Health.Status}}' "${GATEWAY_CONTAINER}" 2>/dev/null || echo "unknown")
            icap_health=$(docker inspect --format '{{.State.Health.Status}}' "${ICAP_CONTAINER}" 2>/dev/null || echo "unknown")
            workspace_health=$(docker inspect --format '{{.State.Health.Status}}' "${WORKSPACE_CONTAINER}" 2>/dev/null || echo "unknown")
            
            if [[ "$gateway_health" == "healthy" ]] && [[ "$icap_health" == "healthy" ]] && [[ "$workspace_health" == "healthy" ]]; then
                echo "# All containers healthy!" >&3
                break
            fi
            
            sleep 2
            elapsed=$((elapsed + 2))
        done
        
        if [[ $elapsed -ge $timeout ]]; then
            echo "# WARNING: Timeout waiting for containers to be healthy" >&3
        fi
    fi
    
    echo "# Suite setup complete" >&3
}

teardown_suite() {
    echo "# Tearing down test suite..." >&3
    # No cleanup needed - we don't stop containers after tests
    echo "# Suite teardown complete" >&3
}
