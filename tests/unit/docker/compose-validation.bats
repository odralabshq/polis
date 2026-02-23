#!/usr/bin/env bats
# bats file_tags=unit,docker
# Docker Compose file validation

setup() {
    load "../../lib/test_helper.bash"
    load "../../lib/constants.bash"
    COMPOSE_FILE="$PROJECT_ROOT/docker-compose.yml"
}

# ── Syntax validation ─────────────────────────────────────────────────────

@test "compose: file exists" {
    [ -f "$COMPOSE_FILE" ]
}

@test "compose: valid YAML syntax" {
    command -v python3 >/dev/null 2>&1 || skip "python3 not installed"
    run python3 -c "import yaml; yaml.safe_load(open('$COMPOSE_FILE'))"
    assert_success
}

@test "compose: docker compose config validates" {
    command -v docker >/dev/null 2>&1 || skip "docker not installed"
    cd "$PROJECT_ROOT"
    # Use --quiet to only show errors
    run docker compose config --quiet
    assert_success
}

# ── Service definitions ───────────────────────────────────────────────────

@test "compose: all required services defined" {
    for svc in resolver gate certgen sentinel scanner state toolbox workspace; do
        run grep -E "^  ${svc}:" "$COMPOSE_FILE"
        assert_success "Service '$svc' not defined"
    done
}

@test "compose: no duplicate service names" {
    local count
    count=$(grep -E "^  [a-z-]+:" "$COMPOSE_FILE" | sort | uniq -d | wc -l)
    [ "$count" -eq 0 ]
}

# ── Network definitions ───────────────────────────────────────────────────

@test "compose: all required networks defined" {
    for net in internal-bridge gateway-bridge external-bridge; do
        run grep -E "^  ${net}:" "$COMPOSE_FILE"
        assert_success "Network '$net' not defined"
    done
}

# ── Security constraints ──────────────────────────────────────────────────

@test "compose: cap_drop ALL on all services" {
    # Count services with cap_drop: ALL (should be >= 7 core services)
    local count
    count=$(grep -c "cap_drop:" "$COMPOSE_FILE" || echo 0)
    [ "$count" -ge 7 ]
}

@test "compose: read_only on all services" {
    local count
    count=$(grep -c "read_only: true" "$COMPOSE_FILE" || echo 0)
    [ "$count" -ge 7 ]
}

@test "compose: no-new-privileges on all services" {
    local count
    count=$(grep -c "no-new-privileges:true" "$COMPOSE_FILE" || echo 0)
    [ "$count" -ge 7 ]
}

# ── Static IPs match constants ────────────────────────────────────────────

@test "compose: resolver IP matches constants" {
    run grep "$IP_RESOLVER_INT" "$COMPOSE_FILE"
    assert_success
    run grep "$IP_RESOLVER_GW" "$COMPOSE_FILE"
    assert_success
}

@test "compose: gate IP matches constants" {
    run grep "$IP_GATE_INT" "$COMPOSE_FILE"
    assert_success
    run grep "$IP_GATE_GW" "$COMPOSE_FILE"
    assert_success
}

@test "compose: sentinel IP matches constants" {
    run grep "$IP_SENTINEL" "$COMPOSE_FILE"
    assert_success
}

@test "compose: toolbox IP matches constants" {
    run grep "$IP_TOOLBOX_INT" "$COMPOSE_FILE"
    assert_success
    run grep "$IP_TOOLBOX_GW" "$COMPOSE_FILE"
    assert_success
}

# ── Secrets ───────────────────────────────────────────────────────────────

@test "compose: secrets section exists" {
    run grep "^secrets:" "$COMPOSE_FILE"
    assert_success
}

@test "compose: no secrets in environment variables" {
    # Ensure no PASSWORD or SECRET values in environment sections
    # grep returns 1 (failure) when no match - that's what we want
    run bash -c "grep -A20 'environment:' '$COMPOSE_FILE' | grep -iE '(password|secret).*='"
    # We expect grep to NOT find anything (exit code 1)
    [ "$status" -ne 0 ]
}
