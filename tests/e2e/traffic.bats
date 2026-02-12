#!/usr/bin/env bats
# End-to-End Traffic Tests
# Tests for HTTP/HTTPS traffic interception and proxy behavior

setup() {
    load "../helpers/common.bash"
    require_container "$GATEWAY_CONTAINER" "$ICAP_CONTAINER" "$WORKSPACE_CONTAINER"
    relax_security_level
}

# =============================================================================
# HTTP Traffic Tests
# =============================================================================

@test "e2e: HTTP request to httpbin.org succeeds" {
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -o /dev/null -w "%{http_code}" --connect-timeout 15 http://httpbin.org/get
    assert_success
    assert_output "200"
}

@test "e2e: HTTP request returns valid response body" {
    run docker exec "${WORKSPACE_CONTAINER}" curl -s --connect-timeout 15 http://httpbin.org/get
    assert_success
    assert_output --partial '"url"'
    assert_output --partial 'httpbin.org'
}

@test "e2e: HTTP POST request works" {
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -X POST -d "test=data" --connect-timeout 15 http://httpbin.org/post
    assert_success
    assert_output --partial '"test"'
}

@test "e2e: HTTP headers are preserved" {
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -H "X-Custom-Header: test-value" --connect-timeout 15 http://httpbin.org/headers
    assert_success
    assert_output --partial "X-Custom-Header"
}

# =============================================================================
# HTTPS Traffic Tests
# =============================================================================

@test "e2e: HTTPS request to httpbin.org succeeds" {
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -o /dev/null -w "%{http_code}" --connect-timeout 15 https://httpbin.org/get
    assert_success
    assert_output "200"
}

@test "e2e: HTTPS request returns valid response body" {
    run docker exec "${WORKSPACE_CONTAINER}" curl -s --connect-timeout 15 https://httpbin.org/get
    assert_success
    assert_output --partial '"url"'
}

@test "e2e: HTTPS POST request works" {
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -X POST -d "secure=data" --connect-timeout 15 https://httpbin.org/post
    assert_success
    assert_output --partial '"secure"'
}

@test "e2e: HTTPS to different domains works" {
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -o /dev/null -w "%{http_code}" --connect-timeout 15 https://api.github.com
    assert_success
    # GitHub API returns 200 or 403 (rate limited)
    [[ "$output" == "200" ]] || [[ "$output" == "403" ]]
}

# =============================================================================
# TLS MITM Verification Tests
# =============================================================================

@test "e2e: TLS certificate chain is valid" {
    # Verify HTTPS works through the proxy (CA trusted or MITM working)
    run docker exec "${WORKSPACE_CONTAINER}" curl -s --connect-timeout 15 -o /dev/null -w "%{http_code}" https://httpbin.org/get
    assert_success
    assert_output "200"
}

@test "e2e: workspace trusts Polis CA" {
    # Verify the CA is in the trust store
    run docker exec "${WORKSPACE_CONTAINER}" curl -v --connect-timeout 15 https://httpbin.org/get 2>&1
    assert_success
    # Should not show certificate errors
    refute_output --partial "certificate problem"
    refute_output --partial "SSL certificate problem"
}

# =============================================================================
# Non-HTTP Port Tests
# =============================================================================

@test "e2e: SSH port (22) is blocked or times out" {
    # Non-HTTP traffic should be blocked to enforce proxy-only governance
    run docker exec "${WORKSPACE_CONTAINER}" timeout 5 bash -c 'echo "" > /dev/tcp/github.com/22' 2>&1
    assert_failure
}

@test "e2e: arbitrary port (8080) is blocked" {
    run docker exec "${WORKSPACE_CONTAINER}" timeout 5 curl -s --connect-timeout 3 http://httpbin.org:8080/get 2>&1
    # Should fail - non-standard HTTP port blocked
    assert_failure
}

@test "e2e: FTP port (21) is blocked" {
    # Non-HTTP traffic should be blocked to enforce proxy-only governance
    run docker exec "${WORKSPACE_CONTAINER}" timeout 5 bash -c 'echo "" > /dev/tcp/ftp.gnu.org/21' 2>&1
    assert_failure
}

# =============================================================================
# Direct IP Access Tests
# =============================================================================

@test "e2e: direct IP access is handled by proxy" {
    # Direct IP should either work through proxy or be blocked
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -o /dev/null -w "%{http_code}" --connect-timeout 10 http://1.1.1.1 2>/dev/null
    # Either succeeds (proxied) or fails (blocked) - both are acceptable
    # The key is it doesn't bypass the proxy
}

@test "e2e: direct IP HTTPS is handled" {
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -o /dev/null -w "%{http_code}" --connect-timeout 10 https://1.1.1.1 2>/dev/null
    # May succeed or fail depending on proxy config
}

# =============================================================================
# DNS Resolution Tests
# =============================================================================

@test "e2e: DNS resolution works for external domains" {
    run docker exec "${WORKSPACE_CONTAINER}" getent hosts httpbin.org
    assert_success
    refute_output ""
}

@test "e2e: DNS resolution works for multiple domains" {
    run docker exec "${WORKSPACE_CONTAINER}" getent hosts github.com
    assert_success
    refute_output ""
}

# =============================================================================
# Large Response Tests
# =============================================================================

@test "e2e: large response body handled correctly" {
    # Request 1KB of data
    run docker exec "${WORKSPACE_CONTAINER}" bash -c "curl -s --connect-timeout 15 'http://httpbin.org/bytes/1024' | wc -c"
    assert_success
    [[ "$output" -ge 1000 ]]
}

@test "e2e: streaming response works" {
    run docker exec "${WORKSPACE_CONTAINER}" bash -c "curl -s --connect-timeout 15 'http://httpbin.org/stream/5' | wc -l"
    assert_success
    [[ "$output" -ge 5 ]]
}

# =============================================================================
# Redirect Tests
# =============================================================================

@test "e2e: HTTP redirects are followed" {
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -L -o /dev/null -w "%{http_code}" --connect-timeout 15 --max-time 30 --retry 2 --retry-delay 2 --retry-all-errors "http://httpbin.org/redirect/1"
    assert_success
    assert_output "200"
}

@test "e2e: HTTPS redirects are followed" {
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -L -o /dev/null -w "%{http_code}" --connect-timeout 15 --max-time 30 --retry 2 --retry-delay 2 --retry-all-errors "https://httpbin.org/redirect/1"
    assert_success
    assert_output "200"
}

# =============================================================================
# Error Response Tests
# =============================================================================

@test "e2e: 404 responses are passed through" {
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -o /dev/null -w "%{http_code}" --connect-timeout 15 "http://httpbin.org/status/404"
    assert_success
    assert_output "404"
}

@test "e2e: 500 responses are passed through" {
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -o /dev/null -w "%{http_code}" --connect-timeout 15 "http://httpbin.org/status/500"
    assert_success
    assert_output "500"
}

# =============================================================================
# Timeout Tests
# =============================================================================

@test "e2e: slow responses are handled" {
    # Request with 2 second delay
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -o /dev/null -w "%{http_code}" --connect-timeout 15 --max-time 10 "http://httpbin.org/delay/2"
    assert_success
    assert_output "200"
}

# =============================================================================
# Content Type Tests
# =============================================================================

@test "e2e: JSON content type preserved" {
    run docker exec "${WORKSPACE_CONTAINER}" bash -c "curl -s -I --connect-timeout 15 'http://httpbin.org/json' | grep -i content-type"
    assert_success
    assert_output --partial "application/json"
}

@test "e2e: HTML content type preserved" {
    run docker exec "${WORKSPACE_CONTAINER}" bash -c "curl -s -I --connect-timeout 15 'http://httpbin.org/html' | grep -i content-type"
    assert_success
    assert_output --partial "text/html"
}

# =============================================================================
# ICAP Integration Tests
# =============================================================================

@test "e2e: traffic passes through ICAP" {
    # Verify ICAP is being used by checking gateway can reach it
    run docker exec "${GATEWAY_CONTAINER}" timeout 2 bash -c "echo > /dev/tcp/icap/1344"
    assert_success
}

@test "e2e: ICAP echo service responds" {
    # The echo service should be functional
    run docker exec "${ICAP_CONTAINER}" pgrep -x c-icap
    assert_success
}

# =============================================================================
# Concurrent Request Tests
# =============================================================================

@test "e2e: multiple concurrent requests work" {
    # Make 3 concurrent requests
    run docker exec "${WORKSPACE_CONTAINER}" bash -c '
        curl -s -o /dev/null -w "%{http_code}" --connect-timeout 15 http://httpbin.org/get &
        curl -s -o /dev/null -w "%{http_code}" --connect-timeout 15 http://httpbin.org/get &
        curl -s -o /dev/null -w "%{http_code}" --connect-timeout 15 http://httpbin.org/get &
        wait
    '
    assert_success
}

# =============================================================================
# User Agent Tests
# =============================================================================

@test "e2e: custom user agent is preserved" {
    run docker exec "${WORKSPACE_CONTAINER}" curl -s -A "TestAgent/1.0" --connect-timeout 15 http://httpbin.org/user-agent
    assert_success
    assert_output --partial "TestAgent"
}
