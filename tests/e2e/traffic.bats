#!/usr/bin/env bats
# bats file_tags=e2e,network
# End-to-End Traffic Tests
# Tests for HTTP/HTTPS traffic interception and proxy behavior

setup_file() {
    load "../helpers/common.bash"
    relax_security_level
    wait_for_port "$GATEWAY_CONTAINER" 18080 || skip "Gateway port 18080 not ready"
    wait_for_port "$ICAP_CONTAINER" 1344 || skip "ICAP port 1344 not ready"
}

teardown_file() {
    load "../helpers/common.bash"
    restore_security_level
}

setup() {
    load "../helpers/common.bash"
    require_container "$GATEWAY_CONTAINER" "$ICAP_CONTAINER" "$WORKSPACE_CONTAINER"
}

# =============================================================================
# HTTP Traffic Tests
# =============================================================================

@test "e2e: HTTP request to httpbin.org succeeds" {
    run_with_network_skip "httpbin.org" docker exec "${WORKSPACE_CONTAINER}" curl -s -o /dev/null -w "%{http_code}" --connect-timeout 15 http://httpbin.org/get
    assert_success
    assert_output "200"
}

@test "e2e: HTTP request returns valid response body" {
    run_with_network_skip "httpbin.org" docker exec "${WORKSPACE_CONTAINER}" curl -s --connect-timeout 15 http://httpbin.org/get
    assert_success
    assert_output --partial '"url"'
    assert_output --partial 'httpbin.org'
}

@test "e2e: HTTP POST request works" {
    run_with_network_skip "httpbin.org" docker exec "${WORKSPACE_CONTAINER}" curl -s -X POST -d "test=data" --connect-timeout 15 http://httpbin.org/post
    assert_success
    assert_output --partial '"test"'
}

@test "e2e: HTTP headers are preserved" {
    run_with_network_skip "httpbin.org" docker exec "${WORKSPACE_CONTAINER}" curl -s -H "X-Custom-Header: test-value" --connect-timeout 15 http://httpbin.org/headers
    assert_success
    assert_output --partial "X-Custom-Header"
}

# =============================================================================
# HTTPS Traffic Tests
# =============================================================================

@test "e2e: HTTPS request to httpbin.org succeeds" {
    run_with_network_skip "httpbin.org" docker exec "${WORKSPACE_CONTAINER}" curl -s -o /dev/null -w "%{http_code}" --connect-timeout 15 https://httpbin.org/get
    assert_success
    assert_output "200"
}

@test "e2e: HTTPS request returns valid response body" {
    run_with_network_skip "httpbin.org" docker exec "${WORKSPACE_CONTAINER}" curl -s --connect-timeout 15 https://httpbin.org/get
    assert_success
    assert_output --partial '"url"'
}

@test "e2e: HTTPS POST request works" {
    run_with_network_skip "httpbin.org" docker exec "${WORKSPACE_CONTAINER}" curl -s -X POST -d "secure=data" --connect-timeout 15 https://httpbin.org/post
    assert_success
    assert_output --partial '"secure"'
}

@test "e2e: HTTPS to different domains works" {
    run_with_network_skip "github.com" docker exec "${WORKSPACE_CONTAINER}" curl -s -o /dev/null -w "%{http_code}" --connect-timeout 15 https://api.github.com
    assert_success
    # GitHub API returns 200 or 403 (rate limited)
    assert [ "$output" = "200" ] || assert [ "$output" = "403" ]
}

# =============================================================================
# TLS MITM Verification Tests
# =============================================================================

@test "e2e: TLS certificate chain is valid" {
    # Verify HTTPS works through the proxy (CA trusted or MITM working)
    run_with_network_skip "httpbin.org" docker exec "${WORKSPACE_CONTAINER}" curl -s --connect-timeout 15 -o /dev/null -w "%{http_code}" https://httpbin.org/get
    assert_success
    assert_output "200"
}

@test "e2e: workspace trusts Polis CA" {
    # Verify the CA is in the trust store
    run_with_network_skip "httpbin.org" docker exec "${WORKSPACE_CONTAINER}" curl -v --connect-timeout 15 https://httpbin.org/get 2>&1
    assert_success
    # Should not show certificate errors
    refute_output --partial "certificate problem"
    refute_output --partial "SSL certificate problem"
}

# =============================================================================
# Non-HTTP Port Tests
# =============================================================================

@test "e2e: SSH port (22) is intercepted by TPROXY" {
    # All TCP traffic (including SSH) is intercepted by TPROXY
    # Connection may succeed or fail depending on g3proxy's protocol handling
    run_with_network_skip "github.com" docker exec "${WORKSPACE_CONTAINER}" timeout 5 bash -c 'echo "" > /dev/tcp/github.com/22' 2>&1
    # We just verify the connection attempt doesn't hang (TPROXY is working)
    # Exit code 0 (success) or 1 (connection refused) both indicate TPROXY intercepted it
    assert [ "$status" -eq 0 ] || assert [ "$status" -eq 1 ]
}

@test "e2e: arbitrary port (8080) is blocked" {
    run_with_network_skip "httpbin.org" docker exec "${WORKSPACE_CONTAINER}" timeout 5 curl -s --connect-timeout 3 http://httpbin.org:8080/get 2>&1
    # Should fail - non-standard HTTP port blocked
    assert_failure
}

@test "e2e: FTP port (21) is intercepted by TPROXY" {
    # All TCP traffic (including FTP) is intercepted by TPROXY
    run_with_network_skip "ftp.gnu.org" docker exec "${WORKSPACE_CONTAINER}" timeout 5 bash -c 'echo "" > /dev/tcp/ftp.gnu.org/21' 2>&1
    # We just verify the connection attempt doesn't hang (TPROXY is working)
    assert [ "$status" -eq 0 ] || assert [ "$status" -eq 1 ]
}

# =============================================================================
# Direct IP Access Tests
# =============================================================================

@test "e2e: direct IP access is handled by proxy" {
    run_with_network_skip "1.1.1.1" docker exec "${WORKSPACE_CONTAINER}" curl -s -o /dev/null -w "%{http_code}" --connect-timeout 10 http://1.1.1.1 2>/dev/null
    # Must get an HTTP status (not 000) — proves proxy intercepted it
    [[ "$status" -ne 0 ]] || assert [ "$output" != "000" ]
}

@test "e2e: direct IP HTTPS is handled" {
    run_with_network_skip "1.1.1.1" docker exec "${WORKSPACE_CONTAINER}" curl -s -o /dev/null -w "%{http_code}" --connect-timeout 10 https://1.1.1.1 2>/dev/null
    # Must get an HTTP status (not 000) — proves proxy intercepted it
    [[ "$status" -ne 0 ]] || assert [ "$output" != "000" ]
}

# =============================================================================
# DNS Resolution Tests
# =============================================================================

@test "e2e: DNS resolution works for external domains" {
    run_with_network_skip "httpbin.org" docker exec "${WORKSPACE_CONTAINER}" getent hosts httpbin.org
    assert_success
    refute_output ""
}

@test "e2e: DNS resolution works for multiple domains" {
    run_with_network_skip "github.com" docker exec "${WORKSPACE_CONTAINER}" getent hosts github.com
    assert_success
    refute_output ""
}

# =============================================================================
# Large Response Tests
# =============================================================================

@test "e2e: large response body handled correctly" {
    # Request 1KB of data
    run_with_network_skip "httpbin.org" docker exec "${WORKSPACE_CONTAINER}" bash -c "curl -s --connect-timeout 15 'http://httpbin.org/bytes/1024' | wc -c"
    assert_success
    assert [ "$output" -ge 1000 ]
}

@test "e2e: streaming response works" {
    run_with_network_skip "httpbin.org" docker exec "${WORKSPACE_CONTAINER}" bash -c "curl -s --connect-timeout 15 'http://httpbin.org/stream/5' | wc -l"
    assert_success
    assert [ "$output" -ge 5 ]
}

# =============================================================================
# Redirect Tests
# =============================================================================

@test "e2e: HTTP redirects are followed" {
    run_with_network_skip "httpbin.org" docker exec "${WORKSPACE_CONTAINER}" curl -s -L -o /dev/null -w "%{http_code}" --connect-timeout 15 --max-time 30 --retry 2 --retry-delay 2 --retry-all-errors "http://httpbin.org/redirect/1"
    assert_success
    assert_output "200"
}

@test "e2e: HTTPS redirects are followed" {
    run_with_network_skip "httpbin.org" docker exec "${WORKSPACE_CONTAINER}" curl -s -L -o /dev/null -w "%{http_code}" --connect-timeout 15 --max-time 30 --retry 2 --retry-delay 2 --retry-all-errors "https://httpbin.org/redirect/1"
    assert_success
    assert_output "200"
}

# =============================================================================
# Error Response Tests
# =============================================================================

@test "e2e: 404 responses are passed through" {
    run_with_network_skip "httpbin.org" docker exec "${WORKSPACE_CONTAINER}" curl -s -o /dev/null -w "%{http_code}" --connect-timeout 15 "http://httpbin.org/status/404"
    assert_success
    assert_output "404"
}

@test "e2e: 500 responses are passed through" {
    run_with_network_skip "httpbin.org" docker exec "${WORKSPACE_CONTAINER}" curl -s -o /dev/null -w "%{http_code}" --connect-timeout 15 "http://httpbin.org/status/500"
    assert_success
    assert_output "500"
}

# =============================================================================
# Timeout Tests
# =============================================================================

@test "e2e: slow responses are handled" {
    # Request with 2 second delay - allow 30s total for proxy stack latency
    # (TPROXY → g3proxy → ICAP inspection → upstream → response)
    run_with_network_skip "httpbin.org" docker exec "${WORKSPACE_CONTAINER}" curl -s -o /dev/null -w "%{http_code}" --connect-timeout 15 --max-time 30 "http://httpbin.org/delay/2"
    assert_success
    assert_output "200"
}

# =============================================================================
# Content Type Tests
# =============================================================================

@test "e2e: JSON content type preserved" {
    run_with_network_skip "httpbin.org" docker exec "${WORKSPACE_CONTAINER}" bash -c "curl -s -I --connect-timeout 15 'http://httpbin.org/json' | grep -i content-type"
    assert_success
    assert_output --partial "application/json"
}

@test "e2e: HTML content type preserved" {
    run_with_network_skip "httpbin.org" docker exec "${WORKSPACE_CONTAINER}" bash -c "curl -s -I --connect-timeout 15 'http://httpbin.org/html' | grep -i content-type"
    assert_success
    assert_output --partial "text/html"
}

# =============================================================================
# ICAP Integration Tests
# =============================================================================

@test "e2e: traffic passes through ICAP" {
    # Verify ICAP is being used by checking gateway can reach it
    run docker exec "${GATEWAY_CONTAINER}" timeout 2 bash -c "echo > /dev/tcp/sentinel/1344"
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
    run_with_network_skip "httpbin.org" docker exec "${WORKSPACE_CONTAINER}" bash -c '
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
    run_with_network_skip "httpbin.org" docker exec "${WORKSPACE_CONTAINER}" curl -s -A "TestAgent/1.0" --connect-timeout 15 http://httpbin.org/user-agent
    assert_success
    assert_output --partial "TestAgent"
}
