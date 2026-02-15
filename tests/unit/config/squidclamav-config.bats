#!/usr/bin/env bats
# bats file_tags=unit,config
# SquidClamav configuration validation
# Source: services/scanner/config/squidclamav.conf

setup() {
    load "../../lib/test_helper.bash"
    CONFIG="$PROJECT_ROOT/services/scanner/config/squidclamav.conf"
}

@test "squidclamav config: file exists" {
    [ -f "$CONFIG" ]
}

@test "squidclamav config: no abortcontent directives" {
    run grep "^abortcontent" "$CONFIG"
    assert_failure
}

@test "squidclamav config: no abort directives" {
    run grep "^abort " "$CONFIG"
    assert_failure
}

@test "squidclamav config: all whitelists are anchored" {
    # Every whitelist line's pattern must start with ^
    run grep "^whitelist " "$CONFIG"
    assert_success
    run bash -c "grep '^whitelist ' '$CONFIG' | grep -v 'whitelist \^'"
    assert_failure
}

@test "squidclamav config: maxsize is 200M" {
    run grep "^maxsize 200M" "$CONFIG"
    assert_success
}

@test "squidclamav config: scan mode is ScanAllExcept" {
    run grep "^scan_mode ScanAllExcept" "$CONFIG"
    assert_success
}

@test "squidclamav config: ClamAV host is scanner" {
    run grep "^clamd_ip scanner" "$CONFIG"
    assert_success
}

@test "squidclamav config: ClamAV port is 3310" {
    run grep "^clamd_port 3310" "$CONFIG"
    assert_success
}
