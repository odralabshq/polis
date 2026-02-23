#!/usr/bin/env bats
# bats file_tags=integration,security
# Runtime seccomp enforcement — verify profiles are applied to containers

setup_file() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    export SENTINEL_INSPECT="$(docker inspect "$CTR_SENTINEL" 2>/dev/null || echo '[]')"
}

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    load "../../lib/guards.bash"
}

@test "sentinel: has custom seccomp profile applied" {
    require_container "$CTR_SENTINEL"
    run jq -r '.[0].HostConfig.SecurityOpt[]' <<< "$SENTINEL_INSPECT"
    assert_success
    assert_output --partial "seccomp="
}

@test "sentinel: seccomp profile is not unconfined" {
    require_container "$CTR_SENTINEL"
    run jq -r '.[0].HostConfig.SecurityOpt[]' <<< "$SENTINEL_INSPECT"
    refute_output --partial "unconfined"
}
