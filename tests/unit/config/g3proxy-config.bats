#!/usr/bin/env bats
# bats file_tags=unit,config
# g3proxy configuration validation

setup() {
    load "../../lib/test_helper.bash"
    CONFIG="$PROJECT_ROOT/services/gate/config/g3proxy.yaml"
}

@test "g3proxy config: file exists" {
    [ -f "$CONFIG" ]
}

@test "g3proxy config: has resolver section" {
    run grep "^resolver:" "$CONFIG"
    assert_success
}

@test "g3proxy config: resolver uses Docker DNS" {
    run grep "127.0.0.11" "$CONFIG"
    assert_success
}

@test "g3proxy config: has ICAP REQMOD configured" {
    run grep "icap_reqmod_service:" "$CONFIG"
    assert_success
    run grep "credcheck" "$CONFIG"
    assert_success
}

@test "g3proxy config: has ICAP RESPMOD configured" {
    run grep "icap_respmod_service:" "$CONFIG"
    assert_success
    run grep "sentinel_respmod" "$CONFIG"
    assert_success
}

@test "g3proxy config: TLS cert agent on port 2999" {
    run grep "query_peer_addr:.*:2999" "$CONFIG"
    assert_success
}

@test "g3proxy config: TPROXY listener on 18080" {
    run grep "listen:.*18080" "$CONFIG"
    assert_success
}

@test "g3proxy config: audit ratio is 1.0" {
    run grep "task_audit_ratio: 1.0" "$CONFIG"
    assert_success
}
