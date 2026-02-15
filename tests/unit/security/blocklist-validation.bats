#!/usr/bin/env bats
# bats file_tags=unit,security
# Blocklist validation

setup() {
    load "../../lib/test_helper.bash"
    DNS_BLOCKLIST="$PROJECT_ROOT/services/resolver/config/blocklist.txt"
    URL_BLOCKLIST="$PROJECT_ROOT/services/sentinel/config/blocklist.txt"
    VALIDATE_SCRIPT="$PROJECT_ROOT/services/sentinel/scripts/validate-blocklist.sh"
}

@test "blocklist: DNS blocklist exists" {
    [ -f "$DNS_BLOCKLIST" ]
}

@test "blocklist: DNS blocklist has common blocked domains" {
    run grep "webhook.site" "$DNS_BLOCKLIST"
    assert_success
    run grep "ngrok.io" "$DNS_BLOCKLIST"
    assert_success
}

@test "blocklist: URL blocklist exists" {
    [ -f "$URL_BLOCKLIST" ]
}

@test "blocklist: validate-blocklist.sh passes on valid file" {
    run bash "$VALIDATE_SCRIPT" "$URL_BLOCKLIST"
    assert_success
}

@test "blocklist: validate-blocklist.sh fails on empty file" {
    local empty
    empty="$(mktemp)"
    run bash "$VALIDATE_SCRIPT" "$empty"
    assert_failure
    assert_output --partial "CRITICAL"
    rm -f "$empty"
}
