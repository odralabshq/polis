#!/usr/bin/env bats
# bats file_tags=unit,dlp
# Unit tests for DLP pattern configuration (polis_dlp.conf)

setup() {
    load "../../lib/test_helper.bash"
    DLP_CONF="${PROJECT_ROOT}/services/sentinel/config/polis_dlp.conf"
}

@test "dlp-config: DLP config file exists" {
    [[ -f "$DLP_CONF" ]]
}

@test "dlp-config: has at least 5 credential patterns" {
    local count
    count=$(grep -c '^pattern\.' "$DLP_CONF")
    [[ "$count" -ge 5 ]]
}

@test "dlp-config: has at least 4 allow rules" {
    local count
    count=$(grep -c '^allow\.' "$DLP_CONF")
    [[ "$count" -ge 4 ]]
}

@test "dlp-config: has at least 3 action rules" {
    local count
    count=$(grep -c '^action\.' "$DLP_CONF")
    [[ "$count" -ge 3 ]]
}

@test "dlp-config: AWS allow rules do not contain wildcard amazonaws.com" {
    # Regression guard for DLP bypass via attacker-controlled S3 path-style URLs.
    # AWS-bound credentials must go through HITL approval, not a blanket wildcard.
    run grep -E '^allow\.aws_(access|secret)' "$DLP_CONF"
    refute_output --partial 'amazonaws.com'
}
