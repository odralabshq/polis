#!/usr/bin/env bats
# bats file_tags=unit,config
# Valkey configuration validation

setup() {
    load "../../lib/test_helper.bash"
    CONFIG="$PROJECT_ROOT/services/state/config/valkey.conf"
}

@test "valkey config: file exists" {
    [ -f "$CONFIG" ]
}

@test "valkey config: TLS port is 6379" {
    run grep "^tls-port 6379" "$CONFIG"
    assert_success
}

@test "valkey config: non-TLS port disabled" {
    run grep "^port 0" "$CONFIG"
    assert_success
}

@test "valkey config: TLS client auth enabled" {
    run grep "^tls-auth-clients yes" "$CONFIG"
    assert_success
}

@test "valkey config: protected mode on" {
    run grep "^protected-mode yes" "$CONFIG"
    assert_success
}

@test "valkey config: ACL file configured" {
    run grep "^aclfile /run/secrets/valkey_acl" "$CONFIG"
    assert_success
}

@test "valkey config: AOF enabled" {
    run grep "^appendonly yes" "$CONFIG"
    assert_success
}

@test "valkey config: max memory 256mb" {
    run grep "^maxmemory 256mb" "$CONFIG"
    assert_success
}

@test "valkey config: eviction policy volatile-lru" {
    run grep "^maxmemory-policy volatile-lru" "$CONFIG"
    assert_success
}

@test "valkey config: IO threads 4" {
    run grep "^io-threads 4" "$CONFIG"
    assert_success
}

@test "valkey config: max clients 100" {
    run grep "^maxclients 100" "$CONFIG"
    assert_success
}

@test "valkey config: data dir is /data" {
    run grep "^dir /data" "$CONFIG"
    assert_success
}
