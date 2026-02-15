#!/usr/bin/env bats
# bats file_tags=e2e,scanning,security
# Verify the running sentinel has no scan-bypass directives in squidclamav.conf

# Source: services/scanner/config/squidclamav.conf
# Mounted: sentinel:/etc/squidclamav.conf (docker-compose.yml)

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    require_container "$CTR_SENTINEL"
}

# =============================================================================
# No bypass directives
# =============================================================================

@test "e2e: no Content-Type bypass (abortcontent) in running config" {
    run docker exec "$CTR_SENTINEL" grep "^abortcontent" /etc/squidclamav.conf
    assert_failure
}

@test "e2e: no abort directives in running config" {
    run docker exec "$CTR_SENTINEL" grep "^abort" /etc/squidclamav.conf
    assert_failure
}

@test "e2e: video files are scanned (no bypass)" {
    run docker exec "$CTR_SENTINEL" grep -E "abort.*video" /etc/squidclamav.conf
    assert_failure
}

@test "e2e: audio files are scanned (no bypass)" {
    run docker exec "$CTR_SENTINEL" grep -E "abort.*audio" /etc/squidclamav.conf
    assert_failure
}

@test "e2e: image files are scanned (no bypass)" {
    run docker exec "$CTR_SENTINEL" grep -E "abort.*image" /etc/squidclamav.conf
    assert_failure
}

# =============================================================================
# Scan mode
# =============================================================================

@test "e2e: scan mode is ScanAllExcept" {
    run docker exec "$CTR_SENTINEL" grep "^scan_mode" /etc/squidclamav.conf
    assert_success
    assert_output "scan_mode ScanAllExcept"
}

# =============================================================================
# Whitelist anchoring
# =============================================================================

@test "e2e: no unanchored whitelist patterns" {
    # Every whitelist regex must start with ^ to prevent suffix attacks
    run docker exec "$CTR_SENTINEL" sh -c \
        "grep '^whitelist ' /etc/squidclamav.conf | grep -v '^whitelist \^'"
    assert_failure
}

@test "e2e: whitelist anchoring prevents suffix attack" {
    # deb.debian.org.evil.com must NOT match the anchored whitelist pattern
    # Source: whitelist ^https?://([a-z0-9-]+\.)*deb\.debian\.org(:[0-9]+)?(/|$)
    run docker exec "$CTR_SENTINEL" sh -c '
        pattern=$(grep "whitelist.*deb.*debian" /etc/squidclamav.conf | head -1 | cut -d" " -f2)
        echo "https://deb.debian.org.evil.com/test" | grep -E "$pattern"'
    assert_failure
}
