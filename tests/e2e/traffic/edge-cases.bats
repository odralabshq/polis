#!/usr/bin/env bats
# bats file_tags=e2e,traffic
# E2E tests for traffic edge cases — delays, redirects, errors, large responses

setup_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    require_container "$CTR_WORKSPACE" "$CTR_GATE" "$CTR_HTTPBIN"
    # Wait for httpbin to accept connections via gate
    for _i in $(seq 1 10); do
        docker exec "$CTR_GATE" nc -z 10.20.1.100 8080 2>/dev/null && break
        sleep 1
    done
    approve_host "$HTTPBIN_HOST" 600
    approve_host "httpbin.org" 600
}

teardown_file() {
    true
}

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
}

PROXY="--proxy http://10.10.1.10:8080"

# ── Delays & redirects (local httpbin) ────────────────────────────────────

@test "e2e: slow response handled (2s delay)" {
    # Retry up to 3 times — the ICAP chain may 502 on the first slow request in CI
    local attempt
    for attempt in 1 2 3; do
        run docker exec "$CTR_WORKSPACE" \
            curl -sf -o /dev/null -w "%{http_code}" --connect-timeout 15 --max-time 30 \
            $PROXY "http://${HTTPBIN_HOST}/delay/2"
        [[ "$status" -eq 0 && "$output" == "200" ]] && return 0
        sleep 2
    done
    assert_success
    assert_output "200"
}

@test "e2e: HTTP redirects followed" {
    run docker exec "$CTR_WORKSPACE" \
        curl -sf -o /dev/null -w "%{http_code}" --connect-timeout 15 -L \
        $PROXY "http://${HTTPBIN_HOST}/redirect/1"
    assert_success
    assert_output "200"
}

@test "e2e: HTTPS redirects followed" {
    run_with_network_skip "httpbin.org" \
        docker exec "$CTR_WORKSPACE" \
        curl -sf -o /dev/null -w "%{http_code}" --connect-timeout 15 -L \
        https://httpbin.org/redirect/1
    assert_success
    assert_output "200"
}

# ── Error status codes (local httpbin) ────────────────────────────────────

@test "e2e: 404 responses passed through" {
    run docker exec "$CTR_WORKSPACE" \
        curl -s -o /dev/null -w "%{http_code}" --connect-timeout 15 \
        $PROXY "http://${HTTPBIN_HOST}/status/404"
    assert_output "404"
}

@test "e2e: 500 responses passed through" {
    run docker exec "$CTR_WORKSPACE" \
        curl -s -o /dev/null -w "%{http_code}" --connect-timeout 15 \
        $PROXY "http://${HTTPBIN_HOST}/status/500"
    assert_output "500"
}

# ── Response sizes (local httpbin) ────────────────────────────────────────

@test "e2e: large response (1KB) handled" {
    local size
    size=$(docker exec "$CTR_WORKSPACE" \
        bash -c "curl -sf --connect-timeout 15 $PROXY 'http://${HTTPBIN_HOST}/bytes/1024' | wc -c")
    [[ "$size" -ge 1000 ]] || fail "Expected ≥1000 bytes, got $size"
}

@test "e2e: streaming response works" {
    # Retry — ICAP chain may 502 on first streaming request in CI
    local attempt
    for attempt in 1 2 3; do
        run docker exec "$CTR_WORKSPACE" \
            curl -sf --connect-timeout 15 $PROXY "http://${HTTPBIN_HOST}/stream/5"
        if [[ "$status" -eq 0 ]]; then
            local line_count
            line_count=$(echo "$output" | wc -l)
            [[ "$line_count" -ge 5 ]] && return 0
        fi
        sleep 2
    done
    assert_success
    local line_count
    line_count=$(echo "$output" | wc -l)
    [[ "$line_count" -ge 5 ]] || fail "Expected ≥5 lines, got $line_count"
}

# ── Edge cases ────────────────────────────────────────────────────────────

@test "e2e: empty HTTP body handled" {
    # Retry — ICAP chain may 502 on empty POST in CI
    local attempt
    for attempt in 1 2 3; do
        run docker exec "$CTR_WORKSPACE" \
            curl -sf -o /dev/null -w "%{http_code}" --connect-timeout 15 \
            $PROXY -X POST "http://${HTTPBIN_HOST}/post"
        [[ "$status" -eq 0 && "$output" == "200" ]] && return 0
        sleep 2
    done
    assert_success
    assert_output "200"
}

@test "e2e: very long URL handled" {
    local long_path
    long_path=$(printf 'a%.0s' {1..200})
    run docker exec "$CTR_WORKSPACE" \
        curl -s -o /dev/null -w "%{http_code}" --connect-timeout 15 \
        $PROXY "http://${HTTPBIN_HOST}/${long_path}"
    [[ "$output" =~ ^[0-9]+$ ]] || fail "Expected HTTP status code, got: $output"
    [[ "$output" != "000" ]] || fail "Got 000 — connection failed"
}

@test "e2e: connection timeout handled" {
    # 192.0.2.1 is TEST-NET-1 (RFC 5737) — guaranteed non-routable
    run docker exec "$CTR_WORKSPACE" \
        curl -s -o /dev/null -w "%{http_code}" --connect-timeout 5 --max-time 10 \
        "http://192.0.2.1/"
    [[ "$status" -ne 0 || "$output" == "000" ]]
}

@test "e2e: DNS failure handled gracefully" {
    run docker exec "$CTR_WORKSPACE" \
        curl -s -o /dev/null -w "%{http_code}" --connect-timeout 10 \
        "http://nonexistent.invalid/"
    assert_failure
}

@test "e2e: direct IP access intercepted" {
    run_with_network_skip "1.1.1.1" \
        docker exec "$CTR_WORKSPACE" \
        curl -s -o /dev/null -w "%{http_code}" --connect-timeout 15 \
        "http://1.1.1.1/"
    [[ "$output" != "000" ]] || fail "Connection failed — traffic not intercepted"
}
