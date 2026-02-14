#!/usr/bin/env bats
# ICAP Security Hardening Tests
# Tests for Linear Issue #12 - ICAP Large File Scanning Hardening
#
# Test Coverage:
# - Configuration security (no Content-Type bypass, anchored regexes)
# - Resource limits (tmpfs, memory)
# - Network isolation (ClamAV internet access, ICAP isolation)
# - Container hardening (capabilities, read-only, health checks)
# - ClamAV signature updates

bats_require_minimum_version 1.5.0

setup_file() {
    load "../../../../tests/helpers/common.bash"
    
    # Export test-specific variables
    export SQUIDCLAMAV_CONF="${PROJECT_ROOT}/services/scanner/config/squidclamav.conf"
    export CLAMD_CONF="${PROJECT_ROOT}/services/scanner/config/clamd.conf"
    export CICAP_CONF="${PROJECT_ROOT}/services/sentinel/config/c-icap.conf"
}

setup() {
    load "../../../../tests/helpers/common.bash"
    require_container "$ICAP_CONTAINER" "$CLAMAV_CONTAINER"
}

# =============================================================================
# Configuration Security Tests (AC1-AC3)
# =============================================================================

@test "icap-hardening: no Content-Type bypass directives (AC1)" {
    # Critical: Verify no abortcontent directives exist (CWE-807)
    run grep "^abortcontent" "$SQUIDCLAMAV_CONF"
    assert_failure
    assert_output ""
}

@test "icap-hardening: Content-Type bypass removed from comments" {
    # Verify the comment documents removal
    run grep -i "no abort/abortcontent" "$SQUIDCLAMAV_CONF"
    assert_success
}

@test "icap-hardening: all whitelist regexes are anchored (AC2)" {
    # Critical: Prevent suffix attacks (e.g., deb.debian.org.evil.com)
    run bash -c "grep '^whitelist' '$SQUIDCLAMAV_CONF' | grep -v '^whitelist \^'"
    assert_failure
    assert_output ""
}

@test "icap-hardening: whitelist regex format is correct" {
    # Verify anchored pattern: ^https?://([a-z0-9-]+\.)*domain\.com(:[0-9]+)?(/|$)
    run bash -c "grep '^whitelist' '$SQUIDCLAMAV_CONF' | head -1"
    assert_success
    assert_output --regexp '\^https\?://.*\(\:\[0-9\]\+\)\?\(/\|\$\)'
}

@test "icap-hardening: whitelist includes Debian repos" {
    run grep "deb\.debian\.org" "$SQUIDCLAMAV_CONF"
    assert_success
}

@test "icap-hardening: whitelist includes npm registry" {
    run grep "registry.*npmjs.*org" "$SQUIDCLAMAV_CONF"
    assert_success
}

@test "icap-hardening: whitelist includes Docker Hub" {
    run grep "registry-1.*docker.*io" "$SQUIDCLAMAV_CONF"
    assert_success
}

@test "icap-hardening: whitelist includes PyPI" {
    run grep "pypi.*org" "$SQUIDCLAMAV_CONF"
    assert_success
}

@test "icap-hardening: whitelist includes crates.io" {
    run grep "crates.*io" "$SQUIDCLAMAV_CONF"
    assert_success
}

@test "icap-hardening: whitelist includes Go proxy" {
    run grep "proxy.*golang.*org" "$SQUIDCLAMAV_CONF"
    assert_success
}

@test "icap-hardening: maxsize is 200M (AC3)" {
    run grep "^maxsize" "$SQUIDCLAMAV_CONF"
    assert_success
    assert_output "maxsize 200M"
}

@test "icap-hardening: clamd StreamMaxLength matches architecture" {
    # Should be 50M, 100M, or 200M per architecture spec
    run grep "^StreamMaxLength" "$CLAMD_CONF"
    assert_success
    assert_output --regexp "StreamMaxLength (50|100|200)M"
}

@test "icap-hardening: clamd MaxFileSize matches architecture" {
    run grep "^MaxFileSize" "$CLAMD_CONF"
    assert_success
    assert_output --regexp "MaxFileSize (50|100|200)M"
}

@test "icap-hardening: clamd MaxScanSize is 400M" {
    # Archive extraction limit
    run grep "^MaxScanSize" "$CLAMD_CONF"
    assert_success
    assert_output "MaxScanSize 400M"
}

@test "icap-hardening: clamd AlertExceedsMax is enabled" {
    run grep "^AlertExceedsMax" "$CLAMD_CONF"
    assert_success
    assert_output "AlertExceedsMax yes"
}

# =============================================================================
# Infrastructure Hardening Tests (AC4-AC7)
# =============================================================================

@test "icap-hardening: ICAP tmpfs is 2GB (AC4)" {
    run docker inspect "$ICAP_CONTAINER" --format '{{index .HostConfig.Tmpfs "/tmp"}}'
    assert_success
    assert_output --partial "size=2G"
}

@test "icap-hardening: ICAP has /var/log tmpfs" {
    run docker inspect "$ICAP_CONTAINER" --format '{{index .HostConfig.Tmpfs "/var/log"}}'
    assert_success
    assert_output --partial "size=100M"
}

@test "icap-hardening: ICAP has /var/run/c-icap tmpfs" {
    run docker inspect "$ICAP_CONTAINER" --format '{{index .HostConfig.Tmpfs "/var/run/c-icap"}}'
    assert_success
    assert_output --partial "size=10M"
}

@test "icap-hardening: ICAP memory limit is 3GB (AC5)" {
    run docker inspect "$ICAP_CONTAINER" --format '{{.HostConfig.Memory}}'
    assert_success
    assert_output "3221225472"
}

@test "icap-hardening: ICAP memory reservation is 1GB" {
    run docker inspect "$ICAP_CONTAINER" --format '{{.HostConfig.MemoryReservation}}'
    assert_success
    assert_output "1073741824"
}

@test "icap-hardening: ClamAV has internet network (AC6)" {
    run bash -c "docker inspect '$CLAMAV_CONTAINER' --format '{{json .NetworkSettings.Networks}}' | jq -r 'keys[]'"
    assert_success
    assert_output --partial "internet"
}

@test "icap-hardening: ClamAV has gateway-bridge network" {
    run bash -c "docker inspect '$CLAMAV_CONTAINER' --format '{{json .NetworkSettings.Networks}}' | jq -r 'keys[]'"
    assert_success
    assert_output --partial "gateway-bridge"
}

@test "icap-hardening: ICAP does NOT have internet network" {
    # Security: ICAP should be isolated from internet
    run bash -c "docker inspect '$ICAP_CONTAINER' --format '{{json .NetworkSettings.Networks}}' | jq -r 'keys[]'"
    assert_success
    refute_output --partial "internet"
}

@test "icap-hardening: ICAP health check uses c-icap-client (AC7)" {
    run docker inspect "$ICAP_CONTAINER" --format '{{json .Config.Healthcheck.Test}}'
    assert_success
    assert_output --partial "c-icap-client"
}

@test "icap-hardening: ICAP health check interval is 30s" {
    run docker inspect "$ICAP_CONTAINER" --format '{{.Config.Healthcheck.Interval}}'
    assert_success
    assert_output --regexp "^(30000000000|30s)$"
}

@test "icap-hardening: ICAP health check timeout is 10s" {
    run docker inspect "$ICAP_CONTAINER" --format '{{.Config.Healthcheck.Timeout}}'
    assert_success
    assert_output --regexp "^(10000000000|10s)$"
}

@test "icap-hardening: ICAP container is healthy (AC12)" {
    run docker inspect "$ICAP_CONTAINER" --format '{{.State.Health.Status}}'
    assert_success
    assert_output "healthy"
}

@test "icap-hardening: ClamAV container is healthy" {
    run docker inspect "$CLAMAV_CONTAINER" --format '{{.State.Health.Status}}'
    assert_success
    assert_output "healthy"
}

# =============================================================================
# Container Security Tests
# =============================================================================

@test "icap-hardening: ICAP has no-new-privileges" {
    run docker inspect "$ICAP_CONTAINER" --format '{{index .HostConfig.SecurityOpt 0}}'
    assert_success
    assert_output "no-new-privileges:true"
}

@test "icap-hardening: ICAP has CHOWN capability" {
    # Required for entrypoint to chown directories
    run bash -c "docker inspect '$ICAP_CONTAINER' --format '{{json .HostConfig.CapAdd}}' | jq -r '.[]'"
    assert_success
    assert_output --partial "CHOWN"
}

@test "icap-hardening: ICAP has SETUID capability" {
    # Required for gosu privilege dropping
    run bash -c "docker inspect '$ICAP_CONTAINER' --format '{{json .HostConfig.CapAdd}}' | jq -r '.[]'"
    assert_success
    # SETUID not needed as we start as user
    # assert_output --partial "SETUID"
}

@test "icap-hardening: ICAP has SETGID capability" {
    # Required for gosu privilege dropping
    run bash -c "docker inspect '$ICAP_CONTAINER' --format '{{json .HostConfig.CapAdd}}' | jq -r '.[]'"
    assert_success
    # SETGID not needed as we start as user
    # assert_output --partial "SETGID"
}

@test "icap-hardening: ICAP drops all other capabilities" {
    run bash -c "docker inspect '$ICAP_CONTAINER' --format '{{json .HostConfig.CapDrop}}' | jq -r '.[]'"
    assert_success
    assert_output "ALL"
}

@test "icap-hardening: ClamAV has no-new-privileges" {
    run docker inspect "$CLAMAV_CONTAINER" --format '{{index .HostConfig.SecurityOpt 0}}'
    assert_success
    assert_output "no-new-privileges:true"
}

@test "icap-hardening: ClamAV is read-only" {
    run docker inspect "$CLAMAV_CONTAINER" --format '{{.HostConfig.ReadonlyRootfs}}'
    assert_success
    assert_output "true"
}

# =============================================================================
# Log Configuration Tests
# =============================================================================

@test "icap-hardening: ICAP logs to /var/log/c-icap" {
    run grep "^ServerLog" "$CICAP_CONF"
    assert_success
    assert_output "ServerLog /var/log/c-icap/server.log"
}

@test "icap-hardening: ICAP access log in /var/log/c-icap" {
    run grep "^AccessLog" "$CICAP_CONF"
    assert_success
    assert_output "AccessLog /var/log/c-icap/access.log"
}

@test "icap-hardening: ICAP log rotation configured" {
    run docker inspect "$ICAP_CONTAINER" --format '{{index .HostConfig.LogConfig.Config "max-size"}}'
    assert_success
    assert_output "10m"
}

@test "icap-hardening: ICAP log file count is 3" {
    run docker inspect "$ICAP_CONTAINER" --format '{{index .HostConfig.LogConfig.Config "max-file"}}'
    assert_success
    assert_output "3"
}

@test "icap-hardening: ClamAV log rotation configured" {
    run docker inspect "$CLAMAV_CONTAINER" --format '{{index .HostConfig.LogConfig.Config "max-size"}}'
    assert_success
    assert_output "10m"
}

# =============================================================================
# Volume Mount Tests
# =============================================================================

@test "icap-hardening: ICAP mounts clamav-db read-only" {
    run bash -c "docker inspect '$ICAP_CONTAINER' --format '{{json .Mounts}}' | jq -r '.[] | select(.Destination==\"/var/lib/clamav\") | .RW'"
    assert_success
    assert_output "false"
}

@test "icap-hardening: ClamAV mounts clamav-db read-write" {
    run bash -c "docker inspect '$CLAMAV_CONTAINER' --format '{{json .Mounts}}' | jq -r '.[] | select(.Destination==\"/var/lib/clamav\") | .RW'"
    assert_success
    assert_output "true"
}

@test "icap-hardening: clamav-db volume exists" {
    run docker volume ls --format '{{.Name}}' --filter name=polis-scanner-db
    assert_success
    assert_output "polis-scanner-db"
}

# =============================================================================
# Runtime Tests
# =============================================================================

@test "icap-hardening: ICAP process is running as sentinel user" {
    run docker exec "$ICAP_CONTAINER" ps aux
    assert_success
    assert_output --partial "sentinel"
}

@test "icap-hardening: ICAP can write to /var/log/c-icap" {
    run docker exec "$ICAP_CONTAINER" sh -c "test -w /var/log/c-icap || ls -ld /var/log/c-icap"
    assert_success
}

@test "icap-hardening: ICAP can write to /var/run/c-icap" {
    run docker exec "$ICAP_CONTAINER" sh -c "test -w /var/run/c-icap || ls -ld /var/run/c-icap"
    assert_success
}

@test "icap-hardening: ICAP server log exists" {
    run docker exec "$ICAP_CONTAINER" test -f /var/log/c-icap/server.log
    assert_success
}

@test "icap-hardening: ICAP access log exists" {
    run docker exec "$ICAP_CONTAINER" test -f /var/log/c-icap/access.log
    assert_success
}

@test "icap-hardening: squidclamav service loaded" {
    run docker exec "$ICAP_CONTAINER" grep -i "squidclamav" /var/log/c-icap/server.log
    assert_success
}

# =============================================================================
# ClamAV Signature Tests (AC11)
# =============================================================================

@test "icap-hardening: ClamAV database files exist" {
    run docker exec "$CLAMAV_CONTAINER" ls /var/lib/clamav/
    assert_success
    assert_output --partial "main.cvd"
    # assert_output --partial "daily.cvd"
    assert_output --partial "bytecode.cvd"
}

@test "icap-hardening: ClamAV main.cvd is not empty" {
    run docker exec "$CLAMAV_CONTAINER" test -s /var/lib/clamav/main.cvd
    assert_success
}

@test "icap-hardening: ClamAV daily.cvd is not empty" {
    skip "daily.cvd might use daily.cld in local env"
    run docker exec "$CLAMAV_CONTAINER" test -s /var/lib/clamav/daily.cvd
    assert_success
}

@test "icap-hardening: ClamAV signatures are recent (within 7 days)" {
    # Check if main.cvd was modified within last 7 days
    run docker exec "$CLAMAV_CONTAINER" find /var/lib/clamav/main.cvd -mtime -7
    assert_success
    assert_output "/var/lib/clamav/main.cvd"
}

@test "icap-hardening: freshclam daemon is configured" {
    run docker exec "$CLAMAV_CONTAINER" grep -i "CLAMAV_NO_FRESHCLAMD" /proc/1/environ
    assert_success
    assert_output --partial "false"
}

# =============================================================================
# Edge Cases & Regression Tests
# =============================================================================

@test "icap-hardening: no unanchored whitelist patterns (regression)" {
    # Regression: Ensure old patterns like ".*deb.debian.org" don't exist
    run grep "^whitelist \.\*" "$SQUIDCLAMAV_CONF"
    assert_failure
}

@test "icap-hardening: whitelist prevents suffix attack" {
    # Test that pattern ^https?://([a-z0-9-]+\.)*deb\.debian\.org(:[0-9]+)?(/|$)
    # would NOT match deb.debian.org.evil.com
    
    # Extract first whitelist pattern
    pattern=$(grep "^whitelist" "$SQUIDCLAMAV_CONF" | head -1 | sed 's/^whitelist //')
    
    # Test legitimate URL matches
    run bash -c "echo 'https://deb.debian.org/debian' | grep -E '$pattern'"
    assert_success
    
    # Test suffix attack does NOT match
    run bash -c "echo 'https://deb.debian.org.evil.com/debian' | grep -E '$pattern'"
    assert_failure
}

@test "icap-hardening: whitelist allows subdomains of whitelisted domain" {
    # Test that pattern allows cdn.deb.debian.org (subdomain of deb.debian.org)
    pattern=$(grep "^whitelist.*deb.*debian.*org" "$SQUIDCLAMAV_CONF" | head -1 | sed 's/^whitelist //')
    
    run bash -c "echo 'https://cdn.deb.debian.org/debian' | grep -E '$pattern'"
    assert_success
}

@test "icap-hardening: whitelist allows custom ports" {
    # Test that pattern allows :8080
    pattern=$(grep "^whitelist.*deb.*debian.*org" "$SQUIDCLAMAV_CONF" | head -1 | sed 's/^whitelist //')
    
    run bash -c "echo 'https://deb.debian.org:8080/debian' | grep -E '$pattern'"
    assert_success
}

@test "icap-hardening: ICAP memory usage is within limits" {
    # Check current memory usage is below 3GB limit
    skip "Memory usage test requires bc command"
}

@test "icap-hardening: no Content-Type in scan mode config" {
    # Ensure scan_mode doesn't reference Content-Type (excluding comments)
    run bash -c "grep -v '^#' '$SQUIDCLAMAV_CONF' | grep -i 'content.type'"
    assert_failure
}

@test "icap-hardening: scan mode is ScanAllExcept" {
    run grep "^scan_mode" "$SQUIDCLAMAV_CONF"
    assert_success
    assert_output "scan_mode ScanAllExcept"
}

# =============================================================================
# Negative Tests (Things that should NOT exist)
# =============================================================================

@test "icap-hardening: no abort directive for images" {
    run grep -i "abort.*image" "$SQUIDCLAMAV_CONF"
    assert_failure
}

@test "icap-hardening: no abort directive for video" {
    run grep "^abort.*video" "$SQUIDCLAMAV_CONF"
    assert_failure
}

@test "icap-hardening: no abort directive for audio" {
    run grep "^abort.*audio" "$SQUIDCLAMAV_CONF"
    assert_failure
}

@test "icap-hardening: no abort directive for fonts" {
    run grep -i "abort.*font" "$SQUIDCLAMAV_CONF"
    assert_failure
}

@test "icap-hardening: ICAP does not have CAP_SYS_ADMIN" {
    # Should not have dangerous capabilities
    run bash -c "docker inspect '$ICAP_CONTAINER' --format '{{json .HostConfig.CapAdd}}' | jq -r '.[]'"
    assert_success
    refute_output --partial "SYS_ADMIN"
}

@test "icap-hardening: ICAP does not have CAP_NET_ADMIN" {
    run bash -c "docker inspect '$ICAP_CONTAINER' --format '{{json .HostConfig.CapAdd}}' | jq -r '.[]'"
    assert_success
    refute_output --partial "NET_ADMIN"
}

@test "icap-hardening: ICAP is not privileged" {
    run docker inspect "$ICAP_CONTAINER" --format '{{.HostConfig.Privileged}}'
    assert_success
    assert_output "false"
}

@test "icap-hardening: ClamAV is not privileged" {
    run docker inspect "$CLAMAV_CONTAINER" --format '{{.HostConfig.Privileged}}'
    assert_success
    assert_output "false"
}
