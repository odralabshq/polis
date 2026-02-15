#!/usr/bin/env bats
# bats file_tags=integration,container
# Integration tests for container image verification

setup_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    for ctr in "${ALL_CONTAINERS[@]}"; do
        local var="${ctr//-/_}_INSPECT"
        export "$var"="$(docker inspect "$ctr" 2>/dev/null || echo '[]')"
    done
}

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
}

_image() { jq -r '.[0].Config.Image' <<< "${!1}"; }

# Source: docker-compose.yml image fields

@test "resolver: uses polis-resolver-oss image" {
    require_container "$CTR_RESOLVER"
    run _image "polis_resolver_INSPECT"
    assert_output --partial "polis-resolver-oss"
}

@test "gate: uses polis-gate-oss image" {
    require_container "$CTR_GATE"
    run _image "polis_gate_INSPECT"
    assert_output --partial "polis-gate-oss"
}

@test "sentinel: uses polis-sentinel-oss image" {
    require_container "$CTR_SENTINEL"
    run _image "polis_sentinel_INSPECT"
    assert_output --partial "polis-sentinel-oss"
}

@test "scanner: uses polis-scanner-oss image" {
    require_container "$CTR_SCANNER"
    run _image "polis_scanner_INSPECT"
    assert_output --partial "polis-scanner-oss"
}

@test "state: uses valkey image" {
    require_container "$CTR_STATE"
    run _image "polis_state_INSPECT"
    assert_output --partial "valkey"
}

@test "toolbox: uses polis-toolbox-oss image" {
    require_container "$CTR_TOOLBOX"
    run _image "polis_toolbox_INSPECT"
    assert_output --partial "polis-toolbox-oss"
}

@test "workspace: uses polis-workspace-oss image" {
    require_container "$CTR_WORKSPACE"
    run _image "polis_workspace_INSPECT"
    assert_output --partial "polis-workspace-oss"
}
