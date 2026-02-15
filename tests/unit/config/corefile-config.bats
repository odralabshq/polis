#!/usr/bin/env bats
# bats file_tags=unit,config
# CoreDNS Corefile validation

setup() {
    load "../../lib/test_helper.bash"
    CONFIG="$PROJECT_ROOT/services/resolver/config/Corefile"
}

@test "corefile config: file exists" {
    [ -f "$CONFIG" ]
}

@test "corefile config: has blocklist plugin" {
    run grep "blocklist" "$CONFIG"
    assert_success
}

@test "corefile config: has forward directive" {
    run grep "forward" "$CONFIG"
    assert_success
}

@test "corefile config: AAAA records return NOERROR" {
    run grep "template ANY AAAA" "$CONFIG"
    assert_success
}
