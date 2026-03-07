#!/usr/bin/env bats
# bats file_tags=unit,scripts
# State generate-secrets.sh validation (runs in temp dir)

setup() {
    load "../../lib/test_helper.bash"
    SCRIPT="$PROJECT_ROOT/services/state/scripts/generate-secrets.sh"
    TEST_DIR="$(mktemp -d)"
}

teardown() {
    rm -rf "$TEST_DIR"
}

@test "generate-secrets: all passwords are 32 characters" {
    run bash "$SCRIPT" "$TEST_DIR" "$TEST_DIR"
    assert_success
    for f in "$TEST_DIR"/valkey_*_password.txt; do
        [ -f "$f" ] || continue
        local len
        len=$(tr -d '[:space:]' < "$f" | wc -c)
        [ "$len" -eq 32 ] || fail "$(basename "$f") has $len chars, expected 32"
    done
}

@test "generate-secrets: all passwords are mutually unique" {
    bash "$SCRIPT" "$TEST_DIR" "$TEST_DIR"
    local passwords=()
    for f in "$TEST_DIR"/valkey_*_password.txt; do
        [ -f "$f" ] || continue
        passwords+=("$(cat "$f")")
    done
    local unique
    unique=$(printf '%s\n' "${passwords[@]}" | sort -u | wc -l)
    [ "$unique" -eq "${#passwords[@]}" ] || fail "Duplicate passwords found"
}

@test "generate-secrets: ACL file is generated" {
    bash "$SCRIPT" "$TEST_DIR" "$TEST_DIR"
    [ -f "$TEST_DIR/valkey_users.acl" ]
}

@test "generate-secrets: password files have correct permissions" {
    bash "$SCRIPT" "$TEST_DIR" "$TEST_DIR"
    for f in "$TEST_DIR"/valkey_*_password.txt; do
        [ -f "$f" ] || continue
        local perms
        perms=$(stat -c '%a' "$f")
        [ "$perms" = "644" ] || [ "$perms" = "600" ] || fail "$(basename "$f") has perms $perms"
    done
}

@test "generate-secrets: rerun backfills control-plane secret without rotating existing secrets" {
    bash "$SCRIPT" "$TEST_DIR" "$TEST_DIR"

    local healthcheck_password
    local agent_password
    healthcheck_password="$(cat "$TEST_DIR/valkey_password.txt")"
    agent_password="$(cat "$TEST_DIR/valkey_mcp_agent_password.txt")"

    rm -f "$TEST_DIR/valkey_cp_server_password.txt"
    grep -v '^user cp-server ' "$TEST_DIR/valkey_users.acl" > "$TEST_DIR/valkey_users.acl.tmp"
    mv "$TEST_DIR/valkey_users.acl.tmp" "$TEST_DIR/valkey_users.acl"

    run bash "$SCRIPT" "$TEST_DIR" "$TEST_DIR"
    assert_success

    [ "$(cat "$TEST_DIR/valkey_password.txt")" = "$healthcheck_password" ]
    [ "$(cat "$TEST_DIR/valkey_mcp_agent_password.txt")" = "$agent_password" ]
    [ -f "$TEST_DIR/valkey_cp_server_password.txt" ]

    run grep '^user cp-server ' "$TEST_DIR/valkey_users.acl"
    assert_success
}

@test "generate-secrets: empty placeholder directories are replaced with files" {
    mkdir -p "$TEST_DIR/valkey_password.txt"

    run bash "$SCRIPT" "$TEST_DIR" "$TEST_DIR"
    assert_success

    [ -f "$TEST_DIR/valkey_password.txt" ]
}
