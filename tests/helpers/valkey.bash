# Polis Valkey Test Helpers
# Extracted from mcp-agent.bats and valkey.bats (Rule of Three)

valkey_cli_as() {
    local user="$1" secret="$2"; shift 2
    local pass
    pass=$(docker exec "${VALKEY_CONTAINER}" cat "/run/secrets/${secret}" 2>/dev/null) || return 1
    docker exec "${VALKEY_CONTAINER}" valkey-cli \
        --tls --cert /etc/valkey/tls/client.crt \
        --key /etc/valkey/tls/client.key --cacert /etc/valkey/tls/ca.crt \
        --user "$user" --pass "$pass" --no-auth-warning "$@"
}

cleanup_valkey_key() {
    valkey_cli_as mcp-admin valkey_mcp_admin_password DEL "$1" 2>/dev/null || true
}
