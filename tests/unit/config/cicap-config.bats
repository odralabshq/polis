#!/usr/bin/env bats
# bats file_tags=unit,config
# c-icap configuration validation

setup() {
    load "../../lib/test_helper.bash"
    CONFIG="$PROJECT_ROOT/services/sentinel/config/c-icap.conf"
}

@test "cicap config: file exists" {
    [ -f "$CONFIG" ]
}

@test "cicap config: port is 0.0.0.0:1344" {
    run grep "Port 0.0.0.0:1344" "$CONFIG"
    assert_success
}

@test "cicap config: StartServers is 3" {
    run grep "StartServers 3" "$CONFIG"
    assert_success
}

@test "cicap config: echo service loaded" {
    run grep "Service echo srv_echo.so" "$CONFIG"
    assert_success
}

@test "cicap config: squidclamav service loaded" {
    run grep "Service squidclamav squidclamav.so" "$CONFIG"
    assert_success
}

@test "cicap config: DLP module loaded" {
    run grep "Service polis_dlp /usr/local/lib/c_icap/srv_polis_dlp.so" "$CONFIG"
    assert_success
}

@test "cicap config: credcheck alias configured" {
    run grep "ServiceAlias credcheck polis_dlp" "$CONFIG"
    assert_success
}

@test "cicap config: approval modules loaded" {
    run grep "polis_approval_rewrite" "$CONFIG"
    assert_success
    run grep "polis_approval" "$CONFIG"
    assert_success
}

@test "cicap config: server log path set" {
    run grep "ServerLog /var/log/c-icap/server.log" "$CONFIG"
    assert_success
}

@test "cicap config: access log path set" {
    run grep "AccessLog /var/log/c-icap/access.log" "$CONFIG"
    assert_success
}
