#!/usr/bin/env bats
# bats file_tags=e2e,scanning,security
# Verify the running scanner has safe ClamAV configuration and
# the on-disk squidclamav.conf has no scan-bypass directives.

# squidclamav.conf is no longer mounted into sentinel (squidclamav module removed),
# but the config file is kept for reference and future use. These tests validate
# the file on disk (unit-style) and the scanner's clamd.conf (e2e).

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
    SQUIDCLAMAV_CONF="${PROJECT_ROOT}/services/scanner/config/squidclamav.conf"
}

# =============================================================================
# squidclamav.conf on-disk validation (no bypass directives)
# =============================================================================

@test "e2e: no Content-Type bypass (abortcontent) in squidclamav config" {
    run grep "^abortcontent" "$SQUIDCLAMAV_CONF"
    assert_failure
}

@test "e2e: no abort directives in squidclamav config" {
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
    run sh -c "grep '^whitelist ' '$SQUIDCLAMAV_CONF' | grep -v '^whitelist \^'"
    assert_failure
}

@test "e2e: whitelist anchoring prevents suffix attack" {
    # deb.debian.org.evil.com must NOT match the anchored whitelist pattern
    run sh -c '
        pattern=$(grep "whitelist.*deb.*debian" "'"$SQUIDCLAMAV_CONF"'" | head -1 | cut -d" " -f2)
        echo "https://deb.debian.org.evil.com/test" | grep -E "$pattern"'
    assert_failure
}

# =============================================================================
# Scanner container ClamAV config (e2e)
# =============================================================================

@test "e2e: scanner clamd has StreamMaxLength set" {
    require_container "$CTR_SCANNER"
    run docker exec "$CTR_SCANNER" grep "^StreamMaxLength" /etc/clamav/clamd.conf
    assert_success
}

@test "e2e: scanner clamd runs as non-root" {
    require_container "$CTR_SCANNER"
    run docker exec "$CTR_SCANNER" grep "^User" /etc/clamav/clamd.conf
    assert_success
    assert_output "User clamav"
}
