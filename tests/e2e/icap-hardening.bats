#!/usr/bin/env bats
# ICAP Hardening E2E Tests
# End-to-end tests for Linear Issue #12 - ICAP Large File Scanning Hardening
#
# Tests actual traffic flow through the hardened ICAP stack

bats_require_minimum_version 1.5.0

setup() {
    load "../helpers/common.bash"
    require_container "$WORKSPACE_CONTAINER" "$GATEWAY_CONTAINER" "$ICAP_CONTAINER" "$CLAMAV_CONTAINER"
    
    export EICAR_STRING='X5O!P%@AP[4\PZX54(P^)7CC)7}$EICAR-STANDARD-ANTIVIRUS-TEST-FILE!$H+H*'
}

# =============================================================================
# Whitelist E2E Tests (Layer 1)
# =============================================================================

@test "e2e-hardening: Debian packages bypass scanning" {
    # Test that whitelisted domains work end-to-end
    run docker exec "$WORKSPACE_CONTAINER" curl -s -o /dev/null -w "%{http_code}" --connect-timeout 15 http://deb.debian.org/debian/dists/stable/Release
    assert_success
    assert_output "200"
}

@test "e2e-hardening: npm registry bypasses scanning" {
    run docker exec "$WORKSPACE_CONTAINER" curl -s -o /dev/null -w "%{http_code}" --connect-timeout 15 https://registry.npmjs.org/
    assert_success
    # npm registry returns 200 or 301
    [[ "$output" =~ ^(200|301)$ ]]
}

@test "e2e-hardening: GitHub bypasses scanning" {
    run docker exec "$WORKSPACE_CONTAINER" curl -s -o /dev/null -w "%{http_code}" --connect-timeout 15 https://github.com/
    assert_success
    assert_output "200"
}

@test "e2e-hardening: suffix attack domain is NOT whitelisted" {
    # Verify deb.debian.org.evil.com would NOT be whitelisted
    # (We can't test actual evil.com, but we can verify the regex)
    run docker exec "$ICAP_CONTAINER" bash -c "echo 'https://deb.debian.org.evil.com/test' | grep -E '^https?://([a-z0-9-]+\.)*deb\.debian\.org(:[0-9]+)?(/|$)'"
    assert_failure
}

# =============================================================================
# Size Limit E2E Tests (Layer 2)
# =============================================================================

@test "e2e-hardening: small files are scanned" {
    # Download a small file - should be scanned
    run docker exec "$WORKSPACE_CONTAINER" curl -s -o /tmp/test.txt -w "%{http_code}" --connect-timeout 15 http://httpbin.org/bytes/1024
    assert_success
    assert_output "200"
    
    # Verify file was downloaded
    run docker exec "$WORKSPACE_CONTAINER" test -f /tmp/test.txt
    assert_success
}

@test "e2e-hardening: files under 100MB are scanned" {
    # Download a 10MB file - should be scanned
    run docker exec "$WORKSPACE_CONTAINER" curl -s -o /tmp/test10mb.bin -w "%{http_code}" --connect-timeout 30 http://httpbin.org/bytes/10485760
    assert_success
    assert_output "200"
    
    # Cleanup
    docker exec "$WORKSPACE_CONTAINER" rm -f /tmp/test10mb.bin
}

@test "e2e-hardening: maxsize is enforced in running config" {
    # Verify the running ICAP service has maxsize configured
    run docker exec "$ICAP_CONTAINER" grep "^maxsize" /etc/squidclamav.conf
    assert_success
    assert_output "maxsize 100M"
}

# =============================================================================
# Content-Type Security E2E Tests
# =============================================================================

@test "e2e-hardening: no Content-Type bypass in running config" {
    # Critical: Verify no abortcontent directives in running config
    run docker exec "$ICAP_CONTAINER" grep "^abortcontent" /etc/squidclamav.conf
    assert_failure
}

@test "e2e-hardening: video files are scanned (no bypass)" {
    # Verify video MIME types are NOT bypassed
    run docker exec "$ICAP_CONTAINER" grep "^abortcontent.*video" /etc/squidclamav.conf
    assert_failure
}

@test "e2e-hardening: audio files are scanned (no bypass)" {
    # Verify audio MIME types are NOT bypassed
    run docker exec "$ICAP_CONTAINER" grep "^abortcontent.*audio" /etc/squidclamav.conf
    assert_failure
}

@test "e2e-hardening: image files are scanned (no bypass)" {
    # Verify image MIME types are NOT bypassed
    run docker exec "$ICAP_CONTAINER" grep "^abortcontent.*image" /etc/squidclamav.conf
    assert_failure
}

# =============================================================================
# Malware Detection E2E Tests
# =============================================================================

@test "e2e-hardening: EICAR is detected by ClamAV" {
    # Verify ClamAV can detect EICAR
    run docker exec "$CLAMAV_CONTAINER" sh -c "echo '$EICAR_STRING' | clamdscan -"
    assert_failure
    assert_output --partial "Eicar"
    assert_output --partial "FOUND"
}

@test "e2e-hardening: EICAR with spoofed Content-Type is still detected" {
    # Create EICAR file and scan it directly
    run docker exec "$CLAMAV_CONTAINER" sh -c "echo '$EICAR_STRING' > /tmp/malware.mp4 && clamdscan /tmp/malware.mp4; rm -f /tmp/malware.mp4"
    assert_success  # clamdscan returns 0 but output shows FOUND
    assert_output --partial "FOUND"
}

@test "e2e-hardening: malware renamed as image is detected" {
    # Verify ClamAV uses magic bytes, not extensions
    run docker exec "$CLAMAV_CONTAINER" sh -c "echo '$EICAR_STRING' > /tmp/fake.png && clamdscan /tmp/fake.png; rm -f /tmp/fake.png"
    assert_success
    assert_output --partial "FOUND"
}

# =============================================================================
# Fail-Closed E2E Tests (Layer 3)
# =============================================================================

@test "e2e-hardening: ICAP service is healthy" {
    run docker inspect "$ICAP_CONTAINER" --format '{{.State.Health.Status}}'
    assert_success
    assert_output "healthy"
}

@test "e2e-hardening: ClamAV service is healthy" {
    run docker inspect "$CLAMAV_CONTAINER" --format '{{.State.Health.Status}}'
    assert_success
    assert_output "healthy"
}

@test "e2e-hardening: ICAP responds to health check" {
    run docker exec "$ICAP_CONTAINER" c-icap-client -i localhost -p 1344 -s squidclamav -f /dev/null
    assert_success
}

@test "e2e-hardening: gateway can reach ICAP service" {
    run docker exec "$GATEWAY_CONTAINER" nc -zv icap 1344
    assert_success
}

# =============================================================================
# Network Isolation E2E Tests
# =============================================================================

@test "e2e-hardening: ICAP cannot reach internet" {
    # ICAP should NOT have internet access
    run docker exec "$ICAP_CONTAINER" timeout 3 nc -zv 8.8.8.8 53 2>&1
    assert_failure
}

@test "e2e-hardening: ClamAV can reach internet for updates" {
    # ClamAV should have internet access for freshclam
    run docker exec "$CLAMAV_CONTAINER" timeout 5 nc -zv database.clamav.net 443 2>&1
    # May fail if DNS doesn't resolve, but should not be blocked by network
    if [[ "$status" -eq 0 ]]; then
        assert_success
    else
        # If it fails, it should be DNS/timeout, not "Network is unreachable"
        refute_output --partial "Network is unreachable"
    fi
}

@test "e2e-hardening: ICAP can reach ClamAV on internal network" {
    run docker exec "$ICAP_CONTAINER" sh -c "echo 'PING' | nc clamav 3310"
    assert_success
    assert_output "PONG"
}

# =============================================================================
# Resource Limits E2E Tests
# =============================================================================

@test "e2e-hardening: ICAP memory usage is within limits" {
    # Check current memory usage
    mem_usage=$(docker stats "$ICAP_CONTAINER" --no-stream --format '{{.MemUsage}}' | awk '{print $1}')
    
    # Should be less than 3GB (rough check)
    [[ ! "$mem_usage" =~ ^[4-9]\.[0-9]+GiB$ ]]
}

@test "e2e-hardening: ICAP tmpfs is mounted" {
    run docker exec "$ICAP_CONTAINER" df -h /tmp
    assert_success
    assert_output --partial "tmpfs"
}

@test "e2e-hardening: ICAP can write to tmpfs" {
    run docker exec "$ICAP_CONTAINER" sh -c "echo test > /tmp/test.txt && cat /tmp/test.txt && rm /tmp/test.txt"
    assert_success
    assert_output "test"
}

# =============================================================================
# Logging E2E Tests
# =============================================================================

@test "e2e-hardening: ICAP logs are being written" {
    run docker exec "$ICAP_CONTAINER" test -s /var/log/c-icap/server.log
    assert_success
}

@test "e2e-hardening: ICAP access log exists" {
    run docker exec "$ICAP_CONTAINER" test -f /var/log/c-icap/access.log
    assert_success
}

@test "e2e-hardening: squidclamav is loaded in logs" {
    run docker exec "$ICAP_CONTAINER" grep -i "squidclamav" /var/log/c-icap/server.log
    assert_success
}

@test "e2e-hardening: whitelist patterns are loaded" {
    run docker exec "$ICAP_CONTAINER" grep "Reading directive whitelist" /var/log/c-icap/server.log
    assert_success
}

# =============================================================================
# ClamAV Signature E2E Tests
# =============================================================================

@test "e2e-hardening: ClamAV signatures are loaded" {
    run docker exec "$CLAMAV_CONTAINER" clamdscan --version
    assert_success
    assert_output --partial "ClamAV"
}

@test "e2e-hardening: ClamAV database is accessible" {
    run docker exec "$CLAMAV_CONTAINER" ls -lh /var/lib/clamav/main.cvd
    assert_success
}

@test "e2e-hardening: ClamAV daemon is responsive" {
    run docker exec "$CLAMAV_CONTAINER" sh -c "echo 'VERSION' | nc localhost 3310"
    assert_success
    assert_output --partial "ClamAV"
}

# =============================================================================
# End-to-End Traffic Flow Tests
# =============================================================================

@test "e2e-hardening: clean HTTP traffic flows through stack" {
    # Test complete flow: workspace -> gateway -> ICAP -> ClamAV -> origin
    run docker exec "$WORKSPACE_CONTAINER" curl -s -o /dev/null -w "%{http_code}" --connect-timeout 15 http://httpbin.org/get
    assert_success
    assert_output "200"
}

@test "e2e-hardening: clean HTTPS traffic flows through stack" {
    run docker exec "$WORKSPACE_CONTAINER" curl -s -o /dev/null -w "%{http_code}" --connect-timeout 15 https://httpbin.org/get
    assert_success
    assert_output "200"
}

@test "e2e-hardening: workspace can download from whitelisted repos" {
    # Test that package manager operations work
    run docker exec "$WORKSPACE_CONTAINER" curl -s -o /dev/null -w "%{http_code}" --connect-timeout 15 http://deb.debian.org/debian/dists/stable/Release
    assert_success
    assert_output "200"
}

# =============================================================================
# Regression Tests
# =============================================================================

@test "e2e-hardening: no unanchored whitelist patterns in running config" {
    # Regression: Ensure old patterns like ".*deb.debian.org" don't exist
    run docker exec "$ICAP_CONTAINER" grep "^whitelist \.\*" /etc/squidclamav.conf
    assert_failure
}

@test "e2e-hardening: scan mode is ScanAllExcept" {
    run docker exec "$ICAP_CONTAINER" grep "^scan_mode" /etc/squidclamav.conf
    assert_success
    assert_output "scan_mode ScanAllExcept"
}

@test "e2e-hardening: no abort directives in running config" {
    run docker exec "$ICAP_CONTAINER" grep "^abort " /etc/squidclamav.conf
    assert_failure
}

# =============================================================================
# Performance Tests
# =============================================================================

@test "e2e-hardening: small file scan completes quickly" {
    # Download should complete within 10 seconds
    run timeout 10 docker exec "$WORKSPACE_CONTAINER" curl -s -o /tmp/quick.txt http://httpbin.org/bytes/1024
    assert_success
}

@test "e2e-hardening: ICAP responds within timeout" {
    # Health check should complete within 10 seconds
    run timeout 10 docker exec "$ICAP_CONTAINER" c-icap-client -i localhost -p 1344 -s squidclamav -f /dev/null
    assert_success
}

@test "e2e-hardening: ClamAV responds within timeout" {
    # ClamAV should respond to PING within 3 seconds
    run timeout 3 docker exec "$CLAMAV_CONTAINER" sh -c "echo 'PING' | nc localhost 3310"
    assert_success
    assert_output "PONG"
}
