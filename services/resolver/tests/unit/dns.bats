#!/usr/bin/env bats
# DNS Unit Tests — static config validation (no containers needed)

setup() {
    load "../../../../tests/helpers/common.bash"
}

# =============================================================================
# File Existence
# =============================================================================

@test "dns-unit: Corefile exists" {
    [[ -f "${PROJECT_ROOT}/services/resolver/config/Corefile" ]]
}

@test "dns-unit: blocklist.txt exists" {
    [[ -f "${PROJECT_ROOT}/services/resolver/config/blocklist.txt" ]]
}

@test "dns-unit: Dockerfile exists" {
    [[ -f "${PROJECT_ROOT}/services/resolver/Dockerfile" ]]
}

@test "dns-unit: validate-blocklist.sh script exists" {
    [[ -x "${PROJECT_ROOT}/services/sentinel/scripts/validate-blocklist.sh" ]]
}

# =============================================================================
# Corefile Validation
# =============================================================================

@test "dns-unit: Corefile contains blocklist plugin" {
    run grep 'blocklist' "${PROJECT_ROOT}/services/resolver/config/Corefile"
    assert_success
}

@test "dns-unit: Corefile contains forward directive" {
    run grep 'forward' "${PROJECT_ROOT}/services/resolver/config/Corefile"
    assert_success
}

# =============================================================================
# Blocklist Content
# =============================================================================

@test "dns-unit: blocklist contains common blocked domains" {
    run grep 'webhook.site' "${PROJECT_ROOT}/services/resolver/config/blocklist.txt"
    assert_success
    run grep 'ngrok.io' "${PROJECT_ROOT}/services/resolver/config/blocklist.txt"
    assert_success
}

# =============================================================================
# Validate Script Logic
# =============================================================================

@test "dns-unit: validate-blocklist.sh passes on blocklist.txt" {
    run bash "${PROJECT_ROOT}/services/sentinel/scripts/validate-blocklist.sh" "${PROJECT_ROOT}/services/resolver/config/blocklist.txt"
    assert_success
    assert_output --partial "Valid:"
}

@test "dns-unit: validate-blocklist.sh fails on missing file" {
    run bash "${PROJECT_ROOT}/services/sentinel/scripts/validate-blocklist.sh" "/nonexistent"
    assert_failure
    assert_output --partial "CRITICAL"
}

@test "dns-unit: validate-blocklist.sh fails on empty file" {
    local tmp
    tmp=$(mktemp)
    echo "# only comments" > "$tmp"
    run bash "${PROJECT_ROOT}/services/sentinel/scripts/validate-blocklist.sh" "$tmp"
    assert_failure
    assert_output --partial "CRITICAL"
    rm -f "$tmp"
}
