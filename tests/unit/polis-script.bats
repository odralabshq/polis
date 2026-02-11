#!/usr/bin/env bats
# Polis Management Script Tests

load '../helpers/common'
load '../bats/bats-file/load'

setup() {
    POLIS_SCRIPT="${PROJECT_ROOT}/tools/polis.sh"
}

@test "polis-script: polis.sh exists" {
    assert_file_exist "${POLIS_SCRIPT}"
}

@test "polis-script: polis.sh is executable" {
    run test -x "${POLIS_SCRIPT}"
    assert_success
}

@test "polis-script: has --agent flag parsing" {
    run grep -q '\-\-agent=' "${POLIS_SCRIPT}"
    assert_success
}

@test "polis-script: has --profile backward compat" {
    run grep -q '\-\-profile=' "${POLIS_SCRIPT}"
    assert_success
}

@test "polis-script: has --no-cache flag" {
    run grep -q 'NO_CACHE=' "${POLIS_SCRIPT}"
    assert_success
}

@test "polis-script: has --local flag" {
    run grep -q 'LOCAL_BUILD=' "${POLIS_SCRIPT}"
    assert_success
}

@test "polis-script: has discover_agents function" {
    run grep -q 'discover_agents()' "${POLIS_SCRIPT}"
    assert_success
}

@test "polis-script: has validate_agent function" {
    run grep -q 'validate_agent()' "${POLIS_SCRIPT}"
    assert_success
}

@test "polis-script: has generate_dockerfile function" {
    run grep -q 'generate_dockerfile()' "${POLIS_SCRIPT}"
    assert_success
}

@test "polis-script: has build_compose_flags function" {
    run grep -q 'build_compose_flags()' "${POLIS_SCRIPT}"
    assert_success
}

@test "polis-script: has dispatch_agent_command function" {
    run grep -q 'dispatch_agent_command()' "${POLIS_SCRIPT}"
    assert_success
}

@test "polis-script: usage shows --agent option" {
    run grep -q '\-\-agent=' "${POLIS_SCRIPT}"
    assert_success
}

@test "polis-script: has all required commands" {
    local commands=("init" "up" "down" "start" "stop" "status" "logs" "build" "shell" "test")
    for cmd in "${commands[@]}"; do
        run grep -q "${cmd})" "${POLIS_SCRIPT}"
        assert_success
    done
}

@test "polis-script: has setup-ca command" {
    run grep -q 'setup-ca)' "${POLIS_SCRIPT}"
    assert_success
}

@test "polis-script: has setup-sysbox command" {
    run grep -q 'setup-sysbox)' "${POLIS_SCRIPT}"
    assert_success
}

@test "polis-script: shell command uses polis user" {
    run grep -A 2 'shell)' "${POLIS_SCRIPT}"
    assert_success
    assert_output --partial '-u polis'
}
