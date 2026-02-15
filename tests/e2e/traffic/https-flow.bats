#!/usr/bin/env bats
# bats file_tags=e2e,traffic
# E2E tests for HTTPS/TLS interception through the proxy chain (external domains)

setup_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    require_container "$CTR_WORKSPACE"
    relax_security_level
}

teardown_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    restore_security_level
}

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
}

# HTTPS tests use TPROXY (external destinations are intercepted)

@test "e2e: HTTPS GET returns 200" {
    run_with_network_skip "httpbin.org" \
        docker exec "$CTR_WORKSPACE" \
        curl -sf -o /dev/null -w "%{http_code}" --connect-timeout 15 \
        https://httpbin.org/get
    assert_success
    assert_output "200"
}

@test "e2e: HTTPS response body valid JSON" {
    run_with_network_skip "httpbin.org" \
        docker exec "$CTR_WORKSPACE" \
        curl -sf --connect-timeout 15 https://httpbin.org/get
    assert_success
    run jq -e '.url' <<< "$output"
    assert_success
}

@test "e2e: HTTPS POST intercepted by DLP" {
    # POST with credential pattern is blocked by DLP at any security level
    run_with_network_skip "httpbin.org" \
        docker exec "$CTR_WORKSPACE" \
        curl -s -w "%{http_code}" --connect-timeout 15 -X POST -d "key=AKIAIOSFODNN7EXAMPLE" \
        https://httpbin.org/post
    assert_output --partial "403"
}

@test "e2e: HTTPS to different domains works" {
    run_with_network_skip "api.github.com" \
        docker exec "$CTR_WORKSPACE" \
        curl -sf -o /dev/null -w "%{http_code}" --connect-timeout 15 \
        https://api.github.com/
    assert_success
    assert_output --regexp "^(200|403)$"
}

@test "e2e: workspace trusts Polis CA" {
    # curl without -k succeeds — workspace has Polis CA installed
    run_with_network_skip "httpbin.org" \
        docker exec "$CTR_WORKSPACE" \
        curl -sf -o /dev/null -w "%{http_code}" --connect-timeout 15 \
        https://httpbin.org/get
    assert_success
    assert_output "200"
}

@test "e2e: HTTPS response headers received" {
    run_with_network_skip "httpbin.org" \
        docker exec "$CTR_WORKSPACE" \
        curl -sf --connect-timeout 15 -D - -o /dev/null \
        https://httpbin.org/get
    assert_success
    assert_output --partial "HTTP/"
}
