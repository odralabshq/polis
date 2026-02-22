#!/usr/bin/env bats
# bats file_tags=e2e,traffic
# E2E tests for concurrent HTTP requests through the proxy chain

setup_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    require_container "$CTR_WORKSPACE"
    approve_host "$HTTPBIN_HOST" 600
}

teardown_file() {
    true
}

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
}

@test "e2e: 3 concurrent HTTP requests succeed" {
    # Retry — ICAP chain may 502 transiently under concurrent load in CI
    local attempt
    for attempt in 1 2 3; do
        run docker exec "$CTR_WORKSPACE" sh -c "
            for i in 1 2 3; do
                curl -sf -o /dev/null -w '%{http_code}\n' --connect-timeout 15 \
                    --proxy http://10.10.1.10:8080 'http://${HTTPBIN_HOST}/get' &
            done
            wait"
        local count
        count=$(echo "$output" | grep -c "200" || true)
        [[ "$status" -eq 0 && "$count" -eq 3 ]] && return 0
        sleep 2
    done
    assert_success
    count=$(echo "$output" | grep -c "200" || true)
    [[ "$count" -eq 3 ]] || fail "Expected 3x 200, got: $output"
}

@test "e2e: mixed HTTP concurrent requests succeed" {
    # Retry — ICAP chain may 502 transiently under concurrent load in CI
    local attempt
    for attempt in 1 2 3; do
        run docker exec "$CTR_WORKSPACE" sh -c "
            curl -sf -o /dev/null -w '%{http_code}\n' --connect-timeout 15 \
                --proxy http://10.10.1.10:8080 'http://${HTTPBIN_HOST}/get' &
            curl -sf -o /dev/null -w '%{http_code}\n' --connect-timeout 15 \
                --proxy http://10.10.1.10:8080 'http://${HTTPBIN_HOST}/headers' &
            curl -sf -o /dev/null -w '%{http_code}\n' --connect-timeout 15 \
                --proxy http://10.10.1.10:8080 'http://${HTTPBIN_HOST}/ip' &
            wait"
        local count
        count=$(echo "$output" | grep -c "200" || true)
        [[ "$status" -eq 0 && "$count" -eq 3 ]] && return 0
        sleep 2
    done
    assert_success
    count=$(echo "$output" | grep -c "200" || true)
    [[ "$count" -eq 3 ]] || fail "Expected 3x 200, got: $output"
}
