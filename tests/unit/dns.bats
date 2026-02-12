#!/usr/bin/env bats
# DNS Unit Tests — static config validation (no containers needed)

setup() {
    load "../helpers/common.bash"
}

# =============================================================================
# File Existence
# =============================================================================

@test "dns-unit: Corefile exists" {
    [[ -f "${PROJECT_ROOT}/config/Corefile" ]]
}

@test "dns-unit: dns-blocklist.txt exists" {
    [[ -f "${PROJECT_ROOT}/config/dns-blocklist.txt" ]]
}

@test "dns-unit: blocklist.txt (HTTP) exists" {
    [[ -f "${PROJECT_ROOT}/config/blocklist.txt" ]]
}

@test "dns-unit: Dockerfile exists" {
    [[ -f "${PROJECT_ROOT}/build/dns/Dockerfile" ]]
}

@test "dns-unit: validate-blocklist.sh exists and is executable" {
    [[ -x "${PROJECT_ROOT}/scripts/validate-blocklist.sh" ]]
}

# =============================================================================
# Corefile Validation
# =============================================================================

@test "dns-unit: Corefile has blocklist plugin" {
    run grep 'blocklist' "${PROJECT_ROOT}/config/Corefile"
    assert_success
}

@test "dns-unit: Corefile has forward to upstream DNS" {
    run grep 'forward' "${PROJECT_ROOT}/config/Corefile"
    assert_success
}

# =============================================================================
# DNS Blocklist Content
# =============================================================================

@test "dns-unit: dns-blocklist.txt has exfiltration domains" {
    run grep 'webhook.site' "${PROJECT_ROOT}/config/dns-blocklist.txt"
    assert_success
}

@test "dns-unit: dns-blocklist.txt has tunneling domains" {
    run grep 'ngrok.io' "${PROJECT_ROOT}/config/dns-blocklist.txt"
    assert_success
}

@test "dns-unit: dns-blocklist.txt has typosquatting domains" {
    run grep 'githab.com' "${PROJECT_ROOT}/config/dns-blocklist.txt"
    assert_success
}

# =============================================================================
# HTTP Blocklist Content
# =============================================================================

@test "dns-unit: blocklist.txt has version header" {
    run grep 'Blocklist' "${PROJECT_ROOT}/config/blocklist.txt"
    assert_success
}

@test "dns-unit: blocklist.txt has exfiltration domains" {
    run grep 'webhook.site' "${PROJECT_ROOT}/config/blocklist.txt"
    assert_success
}

# =============================================================================
# Validate Script
# =============================================================================

@test "dns-unit: validate-blocklist.sh passes on dns-blocklist.txt" {
    run bash "${PROJECT_ROOT}/scripts/validate-blocklist.sh" "${PROJECT_ROOT}/config/dns-blocklist.txt"
    assert_success
    assert_output --partial "Valid:"
}

@test "dns-unit: validate-blocklist.sh passes on blocklist.txt" {
    run bash "${PROJECT_ROOT}/scripts/validate-blocklist.sh" "${PROJECT_ROOT}/config/blocklist.txt"
    assert_success
    assert_output --partial "Valid:"
}

@test "dns-unit: validate-blocklist.sh fails on missing file" {
    run bash "${PROJECT_ROOT}/scripts/validate-blocklist.sh" "/nonexistent"
    assert_failure
    assert_output --partial "CRITICAL"
}

@test "dns-unit: validate-blocklist.sh fails on empty file" {
    local tmp
    tmp=$(mktemp)
    echo "# only comments" > "$tmp"
    run bash "${PROJECT_ROOT}/scripts/validate-blocklist.sh" "$tmp"
    assert_failure
    assert_output --partial "CRITICAL"
    rm -f "$tmp"
}
