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
