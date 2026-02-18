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
    run docker exec "$CTR_WORKSPACE" sh -c "
        for i in 1 2 3; do
            curl -sf -o /dev/null -w '%{http_code}\n' --connect-timeout 15 \
                --proxy http://10.10.1.10:8080 'http://${HTTPBIN_HOST}/get' &
        done
        wait"
    assert_success
    local count
    count=$(echo "$output" | grep -c "200")
    [[ "$count" -eq 3 ]] || fail "Expected 3x 200, got: $output"
}

@test "e2e: mixed HTTP concurrent requests succeed" {
    run docker exec "$CTR_WORKSPACE" sh -c "
        curl -sf -o /dev/null -w '%{http_code}\n' --connect-timeout 15 \
            --proxy http://10.10.1.10:8080 'http://${HTTPBIN_HOST}/get' &
        curl -sf -o /dev/null -w '%{http_code}\n' --connect-timeout 15 \
            --proxy http://10.10.1.10:8080 'http://${HTTPBIN_HOST}/headers' &
        curl -sf -o /dev/null -w '%{http_code}\n' --connect-timeout 15 \
            --proxy http://10.10.1.10:8080 'http://${HTTPBIN_HOST}/ip' &
        wait"
    assert_success
    local count
    count=$(echo "$output" | grep -c "200")
    [[ "$count" -eq 3 ]] || fail "Expected 3x 200, got: $output"
}
