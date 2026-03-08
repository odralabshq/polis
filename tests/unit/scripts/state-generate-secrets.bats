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

@test "generate-secrets: writes DOCKER_GID into project env" {
    run bash "$SCRIPT" "$TEST_DIR" "$TEST_DIR"
    assert_success

    run grep '^DOCKER_GID=' "$TEST_DIR/.env"
    assert_success
}

@test "generate-secrets: windows fallback uses DOCKER_GID=0 when socket is unavailable" {
    local mock_bin="$TEST_DIR/mock-bin"
    mkdir -p "$mock_bin"
    cat > "$mock_bin/uname" <<'EOF'
#!/usr/bin/env bash
echo "MINGW64_NT-10.0"
EOF
    chmod +x "$mock_bin/uname"

    run env PATH="$mock_bin:$PATH" bash "$SCRIPT" "$TEST_DIR" "$TEST_DIR"
    assert_success

    run grep '^DOCKER_GID=0$' "$TEST_DIR/.env"
    assert_success
}

@test "generate-secrets: WSL fallback uses DOCKER_GID=0" {
    run env WSL_DISTRO_NAME=Ubuntu WSL_INTEROP=/run/WSL/mock bash "$SCRIPT" "$TEST_DIR" "$TEST_DIR"
    assert_success

    run grep '^DOCKER_GID=0$' "$TEST_DIR/.env"
    assert_success
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

@test "generate-secrets: ACL includes control-plane auth tokens and bypass scan access" {
    run bash "$SCRIPT" "$TEST_DIR" "$TEST_DIR"
    assert_success

    run grep '^user cp-server .*~polis:auth:tokens:\*' "$TEST_DIR/valkey_users.acl"
    assert_success

    run grep '^user cp-server .*+INFO' "$TEST_DIR/valkey_users.acl"
    assert_success

    run grep '^user dlp-reader .*~polis:config:bypass:\* .*+SCAN' "$TEST_DIR/valkey_users.acl"
    assert_success
}

@test "generate-secrets: auth token files are generated only when auth is enabled" {
    run env POLIS_CP_AUTH_ENABLED=true bash "$SCRIPT" "$TEST_DIR" "$TEST_DIR"
    assert_success

    for f in cp_admin_token.txt cp_operator_token.txt cp_viewer_token.txt cp_agent_token.txt; do
        [ -f "$TEST_DIR/$f" ] || fail "$f was not generated"
    done

    run grep '^polis_admin_[a-f0-9]\{32\}$' "$TEST_DIR/cp_admin_token.txt"
    assert_success
    run grep '^polis_operator_[a-f0-9]\{32\}$' "$TEST_DIR/cp_operator_token.txt"
    assert_success
    run grep '^polis_viewer_[a-f0-9]\{32\}$' "$TEST_DIR/cp_viewer_token.txt"
    assert_success
    run grep '^polis_agent_[a-f0-9]\{32\}$' "$TEST_DIR/cp_agent_token.txt"
    assert_success
}

@test "generate-secrets: auth token rerun backfills missing files without rotating existing tokens" {
    run env POLIS_CP_AUTH_ENABLED=true bash "$SCRIPT" "$TEST_DIR" "$TEST_DIR"
    assert_success

    local admin_token
    admin_token="$(cat "$TEST_DIR/cp_admin_token.txt")"
    rm -f "$TEST_DIR/cp_agent_token.txt"

    run env POLIS_CP_AUTH_ENABLED=true bash "$SCRIPT" "$TEST_DIR" "$TEST_DIR"
    assert_success

    [ "$(cat "$TEST_DIR/cp_admin_token.txt")" = "$admin_token" ]
    [ -f "$TEST_DIR/cp_agent_token.txt" ]
}

@test "generate-secrets: empty placeholder directories are replaced with files" {
    mkdir -p "$TEST_DIR/valkey_password.txt"

    run bash "$SCRIPT" "$TEST_DIR" "$TEST_DIR"
    assert_success

    [ -f "$TEST_DIR/valkey_password.txt" ]
}
