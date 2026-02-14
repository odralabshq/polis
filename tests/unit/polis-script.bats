#!/usr/bin/env bats
# Polis Management Script Tests — Manifest-driven Plugin System

setup() {
    load '../helpers/common.bash'
    load '../bats/bats-file/load.bash'
    POLIS_SCRIPT="${PROJECT_ROOT}/cli/polis.sh"
}

# ── Script basics ──────────────────────────────────────────────────────────

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

# ── Manifest-driven functions ──────────────────────────────────────────────

@test "polis-script: has check_yq function" {
    run grep -q 'check_yq()' "${POLIS_SCRIPT}"
    assert_success
}

@test "polis-script: has audit_log function" {
    run grep -q 'audit_log()' "${POLIS_SCRIPT}"
    assert_success
}

@test "polis-script: has load_agent_yaml function" {
    run grep -q 'load_agent_yaml()' "${POLIS_SCRIPT}"
    assert_success
}

@test "polis-script: has validate_manifest_security function" {
    run grep -q 'validate_manifest_security()' "${POLIS_SCRIPT}"
    assert_success
}

@test "polis-script: has generate_compose_override function" {
    run grep -q 'generate_compose_override()' "${POLIS_SCRIPT}"
    assert_success
}

@test "polis-script: has generate_systemd_unit function" {
    run grep -q 'generate_systemd_unit()' "${POLIS_SCRIPT}"
    assert_success
}

@test "polis-script: has generate_agent_env function" {
    run grep -q 'generate_agent_env()' "${POLIS_SCRIPT}"
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

@test "polis-script: has build_compose_flags function" {
    run grep -q 'build_compose_flags()' "${POLIS_SCRIPT}"
    assert_success
}

@test "polis-script: has dispatch_agent_command function" {
    run grep -q 'dispatch_agent_command()' "${POLIS_SCRIPT}"
    assert_success
}

# ── Old functions removed ──────────────────────────────────────────────────

@test "polis-script: load_agent_conf removed" {
    run grep -q 'load_agent_conf()' "${POLIS_SCRIPT}"
    assert_failure
}

@test "polis-script: no references to agent.conf" {
    run grep -q 'agent\.conf' "${POLIS_SCRIPT}"
    assert_failure
}

# ── Manifest-driven discovery ──────────────────────────────────────────────

@test "polis-script: discover_agents looks for agent.yaml" {
    run grep -A5 'discover_agents()' "${POLIS_SCRIPT}"
    assert_success
    assert_output --partial 'agent.yaml'
}

@test "polis-script: dynamic dispatch checks agent.yaml" {
    run grep 'agent\.yaml' "${POLIS_SCRIPT}"
    assert_success
}

# ── Security validation checks ─────────────────────────────────────────────

@test "polis-script: validates metadata.name regex" {
    run grep 'a-z0-9.*a-z0-9' "${POLIS_SCRIPT}"
    assert_success
}

@test "polis-script: rejects root user" {
    run grep -q '"root"' "${POLIS_SCRIPT}"
    assert_success
}

@test "polis-script: checks shell metacharacters" {
    run grep -q 'metacharacters' "${POLIS_SCRIPT}"
    assert_success
}

@test "polis-script: checks path traversal" {
    run grep -q 'path traversal' "${POLIS_SCRIPT}"
    assert_success
}

@test "polis-script: checks readWritePaths prefixes" {
    run grep -q '/home/polis/' "${POLIS_SCRIPT}"
    assert_success
}

@test "polis-script: checks envFile under /home/polis/" {
    run grep -q 'envFile must be under /home/polis/' "${POLIS_SCRIPT}"
    assert_success
}

# ── Generation outputs ─────────────────────────────────────────────────────

@test "polis-script: generates into .generated/ directory" {
    run grep -q '\.generated/' "${POLIS_SCRIPT}"
    assert_success
}

@test "polis-script: generates SHA-256 hash file" {
    run grep -q 'sha256sum' "${POLIS_SCRIPT}"
    assert_success
}

@test "polis-script: injects CA trust environment variables" {
    run grep -q 'NODE_EXTRA_CA_CERTS' "${POLIS_SCRIPT}"
    assert_success
}

@test "polis-script: injects BindReadOnlyPaths for PrivateTmp" {
    run grep -q 'BindReadOnlyPaths' "${POLIS_SCRIPT}"
    assert_success
}

# ── Reserved ports ─────────────────────────────────────────────────────────

@test "polis-script: defines reserved platform ports" {
    run grep -q 'RESERVED_PORTS' "${POLIS_SCRIPT}"
    assert_success
}

# ── Required commands ──────────────────────────────────────────────────────

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

# ── Manifest files ─────────────────────────────────────────────────────────

@test "polis-script: openclaw agent.yaml exists" {
    assert_file_exist "${PROJECT_ROOT}/agents/openclaw/agent.yaml"
}

@test "polis-script: template agent.yaml exists" {
    assert_file_exist "${PROJECT_ROOT}/agents/_template/agent.yaml"
}

@test "polis-script: old openclaw agent.conf removed" {
    run test -f "${PROJECT_ROOT}/agents/openclaw/agent.conf"
    assert_failure
}

@test "polis-script: old openclaw compose.override.yaml removed" {
    run test -f "${PROJECT_ROOT}/agents/openclaw/compose.override.yaml"
    assert_failure
}

@test "polis-script: old openclaw .service removed" {
    run test -f "${PROJECT_ROOT}/agents/openclaw/config/openclaw.service"
    assert_failure
}

@test "polis-script: old openclaw health.sh removed" {
    run test -f "${PROJECT_ROOT}/agents/openclaw/scripts/health.sh"
    assert_failure
}

@test "polis-script: old template agent.conf removed" {
    run test -f "${PROJECT_ROOT}/agents/_template/agent.conf"
    assert_failure
}

@test "polis-script: old template compose.override.yaml removed" {
    run test -f "${PROJECT_ROOT}/agents/_template/compose.override.yaml"
    assert_failure
}

@test "polis-script: old template agent.service removed" {
    run test -f "${PROJECT_ROOT}/agents/_template/config/agent.service"
    assert_failure
}

@test "polis-script: old template health.sh removed" {
    run test -f "${PROJECT_ROOT}/agents/_template/scripts/health.sh"
    assert_failure
}

# ── .gitignore ─────────────────────────────────────────────────────────────

@test "polis-script: .gitignore includes .generated/" {
    run grep -q 'agents/\*/.generated/' "${PROJECT_ROOT}/.gitignore"
    assert_success
}

@test "polis-script: .gitignore includes audit log" {
    run grep -q 'polis-audit.log' "${PROJECT_ROOT}/.gitignore"
    assert_success
}

# ── init.sh integrity verification ─────────────────────────────────────────

@test "polis-script: init.sh has SHA-256 integrity check" {
    run grep -q 'sha256sum' "${PROJECT_ROOT}/services/workspace/scripts/init.sh"
    assert_success
}

@test "polis-script: init.sh has batched daemon-reload" {
    # Should only have one actual daemon-reload command (batched, not per-agent)
    local count
    count=$(grep -c 'systemctl daemon-reload' "${PROJECT_ROOT}/services/workspace/scripts/init.sh")
    [ "$count" -eq 1 ]
}

@test "polis-script: init.sh collects services into array" {
    run grep -q 'agent_services' "${PROJECT_ROOT}/services/workspace/scripts/init.sh"
    assert_success
}
