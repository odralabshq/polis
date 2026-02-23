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
    # Read config from host mount (minimal sentinel image lacks grep)
    SQUIDCLAMAV_CONF="$PROJECT_ROOT/services/scanner/config/squidclamav.conf"
}

# =============================================================================
# No bypass directives
# =============================================================================

@test "e2e: no Content-Type bypass (abortcontent) in running config" {
    run grep "^abortcontent" "$SQUIDCLAMAV_CONF"
    assert_failure
}

@test "e2e: no abort directives in running config" {
    run grep "^abort" "$SQUIDCLAMAV_CONF"
    assert_failure
}

@test "e2e: video files are scanned (no bypass)" {
    run grep -E "abort.*video" "$SQUIDCLAMAV_CONF"
    assert_failure
}

@test "e2e: audio files are scanned (no bypass)" {
    run grep -E "abort.*audio" "$SQUIDCLAMAV_CONF"
    assert_failure
}

@test "e2e: image files are scanned (no bypass)" {
    run grep -E "abort.*image" "$SQUIDCLAMAV_CONF"
    assert_failure
}

# =============================================================================
# Scan mode
# =============================================================================

@test "e2e: scan mode is ScanAllExcept" {
    run grep "^scan_mode" "$SQUIDCLAMAV_CONF"
    assert_success
    assert_output "scan_mode ScanAllExcept"
}

# =============================================================================
# Whitelist anchoring
# =============================================================================

@test "e2e: no unanchored whitelist patterns" {
    # Every whitelist regex must start with ^ to prevent suffix attacks
    run bash -c "grep '^whitelist ' $SQUIDCLAMAV_CONF | grep -v '^whitelist \^'"
    assert_failure
}

@test "e2e: whitelist anchoring prevents suffix attack" {
    # deb.debian.org.evil.com must NOT match the anchored whitelist pattern
    # Source: whitelist ^https?://([a-z0-9-]+\.)*deb\.debian\.org(:[0-9]+)?(/|$)
    local pattern
    pattern=$(grep "whitelist.*deb.*debian" "$SQUIDCLAMAV_CONF" | head -1 | cut -d" " -f2)
    run bash -c "echo 'https://deb.debian.org.evil.com/test' | grep -E '$pattern'"
    assert_failure
}
