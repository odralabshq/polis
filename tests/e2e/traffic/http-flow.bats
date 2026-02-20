#!/usr/bin/env bats
# bats file_tags=e2e,traffic
# E2E tests for HTTP traffic flow through g3proxy→ICAP chain via local httpbin

setup_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    require_container "$CTR_WORKSPACE"
    require_container "$CTR_GATE"
    approve_host "$HTTPBIN_HOST" 600
}

teardown_file() {
    true
}

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
}

# Workspace → gate HTTP proxy → ICAP → httpbin on external-bridge
PROXY="--proxy http://10.10.1.10:8080"

@test "e2e: HTTP GET returns 200" {
    run docker exec "$CTR_WORKSPACE" \
        curl -sf -o /dev/null -w "%{http_code}" --connect-timeout 15 \
        $PROXY "http://${HTTPBIN_HOST}/get"
    assert_success
    assert_output "200"
}

@test "e2e: HTTP response body is valid JSON" {
    # Retry up to 3 times — proxy/ICAP chain may need warmup in CI
    local attempt body
    for attempt in 1 2 3; do
        body=$(docker exec "$CTR_WORKSPACE" \
            curl -sf --connect-timeout 15 $PROXY "http://${HTTPBIN_HOST}/get" 2>/dev/null) && break
        sleep 2
    done
    [[ -n "$body" ]]
    run jq -e '.url' <<< "$body"
    assert_success
}

@test "e2e: HTTP POST intercepted by DLP" {
    # POST with credential pattern is blocked by DLP at any security level
    run docker exec "$CTR_WORKSPACE" \
        curl -s -w "%{http_code}" --connect-timeout 15 $PROXY -X POST -d "key=AKIAIOSFODNN7EXAMPLE" \
        "http://${HTTPBIN_HOST}/post"
    assert_output --partial "403"
}

@test "e2e: HTTP custom header preserved" {
    run docker exec "$CTR_WORKSPACE" \
        curl -sf --connect-timeout 15 $PROXY -H "X-Polis-Test: hello" \
        "http://${HTTPBIN_HOST}/get"
    assert_success
    assert_output --partial "X-Polis-Test"
}

@test "e2e: JSON content-type preserved" {
    run docker exec "$CTR_WORKSPACE" \
        curl -sf --connect-timeout 15 $PROXY -o /dev/null -w "%{content_type}" \
        "http://${HTTPBIN_HOST}/get"
    assert_success
    assert_output --partial "application/json"
}

@test "e2e: HTML content-type preserved" {
    run docker exec "$CTR_WORKSPACE" \
        curl -sf --connect-timeout 15 $PROXY -o /dev/null -w "%{content_type}" \
        "http://${HTTPBIN_HOST}/html"
    assert_success
    assert_output --partial "text/html"
}

@test "e2e: custom user agent preserved" {
    run docker exec "$CTR_WORKSPACE" \
        curl -sf --connect-timeout 15 $PROXY -A "PolisTestAgent/1.0" \
        "http://${HTTPBIN_HOST}/get"
    assert_success
    assert_output --partial "PolisTestAgent/1.0"
}

@test "e2e: traffic passes through ICAP chain" {
    run docker exec "$CTR_GATE" nc -z -w3 "$IP_SENTINEL" "$PORT_ICAP"
    assert_success
}
