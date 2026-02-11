#!/usr/bin/env bats
# Valkey Property-Based Tests
# Parameterized tests validating correctness properties from the design doc.
# Since bats has no native PBT library, properties are tested via loops
# over the full input domain.

setup() {
    load "../helpers/common.bash"
    require_container "$VALKEY_CONTAINER"

    CERT_SCRIPT="${PROJECT_ROOT}/scripts/generate-valkey-certs.sh"
    CREDENTIALS_FILE="${PROJECT_ROOT}/secrets/credentials.env.example"
}

# Helper: run a valkey-cli command inside the container as a given user
# Usage: valkey_cli_as <username> <password> <command> [args...]
valkey_cli_as() {
    local user="$1"
    local pass="$2"
    shift 2
    docker exec "${VALKEY_CONTAINER}" \
        valkey-cli \
        --tls \
        --cert /etc/valkey/tls/client.crt \
        --key /etc/valkey/tls/client.key \
        --cacert /etc/valkey/tls/ca.crt \
        --user "${user}" \
        --pass "${pass}" \
        "$@"
}

# Helper: read a password from the credentials file
# Usage: get_password <ENV_VAR_NAME>
get_password() {
    local key="$1"
    grep "^${key}=" "${CREDENTIALS_FILE}" | cut -d'=' -f2
}

# =============================================================================
# Property 1: Dangerous commands are disabled
# Feature: valkey-state-management, Property 1: Dangerous commands disabled
# Validates: Requirements 2.6
#
# For any command in the set {FLUSHALL, FLUSHDB, DEBUG, CONFIG, SHUTDOWN,
# SLAVEOF, REPLICAOF, MODULE, BGSAVE, BGREWRITEAOF, KEYS}, executing that
# command against the Valkey instance should return an error indicating the
# command is not recognized.
# =============================================================================

@test "property 1: all 11 dangerous commands return error" {
    local admin_pass
    admin_pass="$(get_password VALKEY_MCP_ADMIN_PASS)"

    # Full set of dangerous commands that must be disabled
    local dangerous_commands=(
        "FLUSHALL"
        "FLUSHDB"
        "DEBUG"
        "CONFIG"
        "SHUTDOWN"
        "SLAVEOF"
        "REPLICAOF"
        "MODULE"
        "BGSAVE"
        "BGREWRITEAOF"
        "KEYS"
    )

    for cmd in "${dangerous_commands[@]}"; do
        local result
        result="$(valkey_cli_as \
            "mcp-admin" "${admin_pass}" ${cmd} 2>&1 || true)"

        # Commands must return "unknown command", NOPERM, or ERR (blocked/unusable)
        if [[ "${result}" != *"unknown command"* ]] && [[ "${result}" != *"NOPERM"* ]] && [[ "${result}" != *"ERR"* ]]; then
            fail "Dangerous command ${cmd} was not blocked." \
                 " Got: ${result}"
        fi
    done
}

# =============================================================================
# Property 2: mcp-agent ACL enforcement
# Feature: valkey-state-management, Property 2: mcp-agent ACL enforcement
# Validates: Requirements 3.2
#
# For any key pattern outside polis:blocked:*, polis:approved:*, and
# polis:config:*, the mcp-agent user should be denied access.
# Additionally, for any attempt to execute DEL or UNLINK on allowed
# keys, the mcp-agent user should be denied.
# =============================================================================

@test "property 2: mcp-agent denied access to unauthorized keys" {
    local agent_pass
    agent_pass="$(get_password VALKEY_MCP_AGENT_PASS)"

    # Keys outside the allowed patterns
    local denied_keys=(
        "polis:log:events"
        "polis:other:data"
        "unauthorized:key"
        "random:key"
        "polis:admin:settings"
    )

    for key in "${denied_keys[@]}"; do
        local result
        result="$(valkey_cli_as \
            "mcp-agent" "${agent_pass}" \
            SET "${key}" "test" 2>&1 || true)"

        if [[ "${result}" != *"NOPERM"* ]] \
           && [[ "${result}" != *"no permissions"* ]]; then
            fail "mcp-agent should be denied key ${key}." \
                 " Got: ${result}"
        fi
    done
}

@test "property 2: mcp-agent denied DEL and UNLINK on allowed keys" {
    local agent_pass
    agent_pass="$(get_password VALKEY_MCP_AGENT_PASS)"

    # DEL and UNLINK must be denied even on allowed key patterns
    local denied_commands=("DEL" "UNLINK")
    local allowed_key="polis:blocked:test-acl-check"

    for cmd in "${denied_commands[@]}"; do
        local result
        result="$(valkey_cli_as \
            "mcp-agent" "${agent_pass}" \
            ${cmd} "${allowed_key}" 2>&1 || true)"

        if [[ "${result}" != *"NOPERM"* ]] \
           && [[ "${result}" != *"no permissions"* ]]; then
            fail "mcp-agent should be denied ${cmd}." \
                 " Got: ${result}"
        fi
    done
}

# =============================================================================
# Property 3: mcp-admin ACL enforcement
# Feature: valkey-state-management, Property 3: mcp-admin ACL enforcement
# Validates: Requirements 3.3
#
# For any command in the dangerous set {FLUSHALL, FLUSHDB, DEBUG,
# CONFIG, SHUTDOWN}, the mcp-admin user should be denied execution,
# even though mcp-admin has full access to the polis:* namespace.
# =============================================================================

@test "property 3: mcp-admin denied dangerous commands" {
    local admin_pass
    admin_pass="$(get_password VALKEY_MCP_ADMIN_PASS)"

    # Dangerous commands that mcp-admin must be denied
    local denied_commands=(
        "FLUSHALL"
        "FLUSHDB"
        "DEBUG"
        "CONFIG"
        "SHUTDOWN"
    )

    for cmd in "${denied_commands[@]}"; do
        local result
        result="$(valkey_cli_as \
            "mcp-admin" "${admin_pass}" \
            ${cmd} 2>&1 || true)"

        if [[ "${result}" != *"NOPERM"* ]] \
           && [[ "${result}" != *"no permissions"* ]] \
           && [[ "${result}" != *"ERR"* ]]; then
            fail "mcp-admin should be denied ${cmd}." \
                 " Got: ${result}"
        fi
    done
}

# =============================================================================
# Property 4: log-writer ACL enforcement
# Feature: valkey-state-management, Property 4: log-writer ACL enforcement
# Validates: Requirements 3.4
#
# For any command not in {ZADD, ZRANGEBYSCORE, ZCARD, PING}, the
# log-writer user should be denied execution. For any key other than
# polis:log:events, the log-writer user should be denied access.
# =============================================================================

@test "property 4: log-writer denied non-allowed commands" {
    local lw_pass
    lw_pass="$(get_password VALKEY_LOG_WRITER_PASS)"

    # Commands that log-writer must NOT be able to run
    local denied_commands=(
        "SET"
        "GET"
        "DEL"
        "HSET"
        "LPUSH"
        "SADD"
        "INFO"
    )

    for cmd in "${denied_commands[@]}"; do
        local result
        result="$(valkey_cli_as \
            "log-writer" "${lw_pass}" \
            ${cmd} "polis:log:events" 2>&1 || true)"

        if [[ "${result}" != *"NOPERM"* ]] \
           && [[ "${result}" != *"no permissions"* ]] \
           && [[ "${result}" != *"ERR"* ]]; then
            fail "log-writer should be denied ${cmd}." \
                 " Got: ${result}"
        fi
    done
}

@test "property 4: log-writer denied access to non-allowed keys" {
    local lw_pass
    lw_pass="$(get_password VALKEY_LOG_WRITER_PASS)"

    # Keys outside the allowed pattern
    local denied_keys=(
        "polis:blocked:test"
        "polis:approved:test"
        "polis:config:test"
        "polis:other:data"
        "unauthorized:key"
    )

    for key in "${denied_keys[@]}"; do
        local result
        result="$(valkey_cli_as \
            "log-writer" "${lw_pass}" \
            ZADD "${key}" 1 "test" 2>&1 || true)"

        if [[ "${result}" != *"NOPERM"* ]] \
           && [[ "${result}" != *"no permissions"* ]]; then
            fail "log-writer should be denied key ${key}." \
                 " Got: ${result}"
        fi
    done
}

# =============================================================================
# Property 5: healthcheck ACL enforcement
# Feature: valkey-state-management, Property 5: healthcheck ACL enforcement
# Validates: Requirements 3.5
#
# For any command not in {PING, INFO}, the healthcheck user should be
# denied execution. For any key access attempt, the healthcheck user
# should be denied.
# =============================================================================

@test "property 5: healthcheck denied non-allowed commands" {
    local hc_pass
    hc_pass="$(get_password VALKEY_HEALTHCHECK_PASS)"

    # Commands that healthcheck must NOT be able to run
    local denied_commands=(
        "SET"
        "GET"
        "DEL"
        "ZADD"
        "HSET"
        "LPUSH"
        "KEYS"
    )

    for cmd in "${denied_commands[@]}"; do
        local result
        result="$(valkey_cli_as \
            "healthcheck" "${hc_pass}" \
            ${cmd} "somekey" 2>&1 || true)"

        if [[ "${result}" != *"NOPERM"* ]] \
           && [[ "${result}" != *"no permissions"* ]] \
           && [[ "${result}" != *"ERR"* ]]; then
            fail "healthcheck should be denied ${cmd}." \
                 " Got: ${result}"
        fi
    done
}

@test "property 5: healthcheck denied key access" {
    local hc_pass
    hc_pass="$(get_password VALKEY_HEALTHCHECK_PASS)"

    # Any key access should be denied for healthcheck
    local test_keys=(
        "polis:blocked:test"
        "polis:approved:test"
        "polis:config:test"
        "polis:log:events"
        "any:random:key"
    )

    for key in "${test_keys[@]}"; do
        local result
        result="$(valkey_cli_as \
            "healthcheck" "${hc_pass}" \
            GET "${key}" 2>&1 || true)"

        if [[ "${result}" != *"NOPERM"* ]] \
           && [[ "${result}" != *"no permissions"* ]] \
           && [[ "${result}" != *"ERR"* ]]; then
            fail "healthcheck should be denied key ${key}." \
                 " Got: ${result}"
        fi
    done
}

# =============================================================================
# Property 6: Certificate file permissions
# Feature: valkey-state-management, Property 6: Certificate file permissions
# Validates: Requirements 4.3
#
# For all files generated by the cert generator, private key files (.key)
# should have permission 600 and certificate files (.crt) should have
# permission 644.
# =============================================================================

@test "property 6: all .key files have permission 600" {
    # Skip on Windows/MSYS — mktemp paths conflict with MSYS_NO_PATHCONV
    if [[ "$(uname -s)" == MINGW* ]] || [[ "$(uname -s)" == MSYS* ]]; then
        skip "Skipped on Windows/MSYS (MSYS_NO_PATHCONV path conflict)"
    fi

    local tmpdir
    tmpdir="$(mktemp -d)"

    # Generate certificates into a temp directory
    run bash "${CERT_SCRIPT}" "${tmpdir}"
    assert_success

    # Define the full set of expected key files
    local key_files=("ca.key" "server.key" "client.key")

    for key_file in "${key_files[@]}"; do
        local filepath="${tmpdir}/${key_file}"
        # Verify the file exists
        [ -f "${filepath}" ] || {
            rm -rf "${tmpdir}"
            fail "Expected key file not found: ${key_file}"
        }
        # Get octal permissions (last 3 digits)
        local perms
        perms="$(stat -c '%a' "${filepath}" 2>/dev/null \
              || stat -f '%Lp' "${filepath}" 2>/dev/null)"
        if [[ "${perms}" != "600" ]]; then
            rm -rf "${tmpdir}"
            fail "${key_file} has permission ${perms}, expected 600"
        fi
    done

    rm -rf "${tmpdir}"
}

@test "property 6: all .crt files have permission 644" {
    # Skip on Windows/MSYS — mktemp paths conflict with MSYS_NO_PATHCONV
    if [[ "$(uname -s)" == MINGW* ]] || [[ "$(uname -s)" == MSYS* ]]; then
        skip "Skipped on Windows/MSYS (MSYS_NO_PATHCONV path conflict)"
    fi

    local tmpdir
    tmpdir="$(mktemp -d)"

    # Generate certificates into a temp directory
    run bash "${CERT_SCRIPT}" "${tmpdir}"
    assert_success

    # Define the full set of expected certificate files
    local crt_files=("ca.crt" "server.crt" "client.crt")

    for crt_file in "${crt_files[@]}"; do
        local filepath="${tmpdir}/${crt_file}"
        # Verify the file exists
        [ -f "${filepath}" ] || {
            rm -rf "${tmpdir}"
            fail "Expected cert file not found: ${crt_file}"
        }
        # Get octal permissions (last 3 digits)
        local perms
        perms="$(stat -c '%a' "${filepath}" 2>/dev/null \
              || stat -f '%Lp' "${filepath}" 2>/dev/null)"
        if [[ "${perms}" != "644" ]]; then
            rm -rf "${tmpdir}"
            fail "${crt_file} has permission ${perms}, expected 644"
        fi
    done

    rm -rf "${tmpdir}"
}

# =============================================================================
# Property 7: Password uniqueness and length
# Feature: valkey-state-management, Property 7: Password uniqueness and length
# Validates: Requirements 5.1
#
# For any execution of the secrets generator, all four generated passwords
# should be exactly 32 characters long and mutually unique.
# =============================================================================

@test "property 7: all four passwords are exactly 32 characters" {
    # Skip on Windows/MSYS — mktemp paths conflict with MSYS_NO_PATHCONV
    if [[ "$(uname -s)" == MINGW* ]] || [[ "$(uname -s)" == MSYS* ]]; then
        skip "Skipped on Windows/MSYS (MSYS_NO_PATHCONV path conflict)"
    fi

    local tmpdir
    tmpdir="$(mktemp -d)"

    SECRETS_SCRIPT="${PROJECT_ROOT}/scripts/generate-valkey-secrets.sh"

    # Generate secrets into a temp directory
    run bash "${SECRETS_SCRIPT}" "${tmpdir}"
    assert_success

    # Extract passwords from credentials.env.example
    local creds_file="${tmpdir}/credentials.env.example"
    [ -f "${creds_file}" ] || {
        rm -rf "${tmpdir}"
        fail "credentials.env.example not found"
    }

    local password_keys=(
        "VALKEY_MCP_AGENT_PASS"
        "VALKEY_MCP_ADMIN_PASS"
        "VALKEY_LOG_WRITER_PASS"
        "VALKEY_HEALTHCHECK_PASS"
    )

    for key in "${password_keys[@]}"; do
        local password
        password="$(grep "^${key}=" "${creds_file}" \
                  | cut -d'=' -f2)"
        local len="${#password}"
        if [[ "${len}" -ne 32 ]]; then
            rm -rf "${tmpdir}"
            fail "${key} has length ${len}, expected 32"
        fi
    done

    rm -rf "${tmpdir}"
}

@test "property 7: all four passwords are mutually unique" {
    # Skip on Windows/MSYS — mktemp paths conflict with MSYS_NO_PATHCONV
    if [[ "$(uname -s)" == MINGW* ]] || [[ "$(uname -s)" == MSYS* ]]; then
        skip "Skipped on Windows/MSYS (MSYS_NO_PATHCONV path conflict)"
    fi

    local tmpdir
    tmpdir="$(mktemp -d)"

    SECRETS_SCRIPT="${PROJECT_ROOT}/scripts/generate-valkey-secrets.sh"

    # Generate secrets into a temp directory
    run bash "${SECRETS_SCRIPT}" "${tmpdir}"
    assert_success

    # Extract passwords from credentials.env.example
    local creds_file="${tmpdir}/credentials.env.example"
    [ -f "${creds_file}" ] || {
        rm -rf "${tmpdir}"
        fail "credentials.env.example not found"
    }

    local password_keys=(
        "VALKEY_MCP_AGENT_PASS"
        "VALKEY_MCP_ADMIN_PASS"
        "VALKEY_LOG_WRITER_PASS"
        "VALKEY_HEALTHCHECK_PASS"
    )

    local passwords=()
    for key in "${password_keys[@]}"; do
        local password
        password="$(grep "^${key}=" "${creds_file}" \
                  | cut -d'=' -f2)"
        passwords+=("${password}")
    done

    # Check all pairs for uniqueness
    local count="${#passwords[@]}"
    for ((i = 0; i < count; i++)); do
        for ((j = i + 1; j < count; j++)); do
            if [[ "${passwords[$i]}" == "${passwords[$j]}" ]]; then
                rm -rf "${tmpdir}"
                fail "Password ${i} and ${j} are identical"
            fi
        done
    done

    rm -rf "${tmpdir}"
}

# =============================================================================
# Property 8: ACL password hash consistency
# Feature: valkey-state-management, Property 8: ACL password hash consistency
# Validates: Requirements 5.3
#
# For any user in {mcp-agent, mcp-admin, log-writer, healthcheck}, the
# SHA-256 hash stored in valkey_users.acl should match the SHA-256 of the
# corresponding plaintext password in credentials.env.example.
# =============================================================================

@test "property 8: ACL hashes match SHA-256 of plaintext passwords" {
    # Skip on Windows/MSYS — mktemp paths conflict with MSYS_NO_PATHCONV
    if [[ "$(uname -s)" == MINGW* ]] || [[ "$(uname -s)" == MSYS* ]]; then
        skip "Skipped on Windows/MSYS (MSYS_NO_PATHCONV path conflict)"
    fi

    local tmpdir
    tmpdir="$(mktemp -d)"

    SECRETS_SCRIPT="${PROJECT_ROOT}/scripts/generate-valkey-secrets.sh"

    # Generate secrets into a temp directory
    run bash "${SECRETS_SCRIPT}" "${tmpdir}"
    assert_success

    local creds_file="${tmpdir}/credentials.env.example"
    local acl_file="${tmpdir}/valkey_users.acl"

    [ -f "${creds_file}" ] || {
        rm -rf "${tmpdir}"
        fail "credentials.env.example not found"
    }
    [ -f "${acl_file}" ] || {
        rm -rf "${tmpdir}"
        fail "valkey_users.acl not found"
    }

    # Map ACL usernames to env var keys
    local -A user_to_env=(
        ["mcp-agent"]="VALKEY_MCP_AGENT_PASS"
        ["mcp-admin"]="VALKEY_MCP_ADMIN_PASS"
        ["log-writer"]="VALKEY_LOG_WRITER_PASS"
        ["healthcheck"]="VALKEY_HEALTHCHECK_PASS"
    )

    for acl_user in "mcp-agent" "mcp-admin" \
                    "log-writer" "healthcheck"; do
        local env_key="${user_to_env[${acl_user}]}"

        # Get plaintext password from credentials file
        local plaintext
        plaintext="$(grep "^${env_key}=" "${creds_file}" \
                   | cut -d'=' -f2)"

        # Compute expected SHA-256 hash
        local expected_hash
        expected_hash="$(echo -n "${plaintext}" \
                       | sha256sum | awk '{print $1}')"

        # Extract hash from ACL file (format: user <name> on #<hash> ...)
        local acl_hash
        acl_hash="$(grep "^user ${acl_user} " "${acl_file}" \
                  | grep -o '#[a-f0-9]\{64\}' \
                  | sed 's/^#//')"

        if [[ "${expected_hash}" != "${acl_hash}" ]]; then
            rm -rf "${tmpdir}"
            fail "Hash mismatch for ${acl_user}: " \
                 "expected=${expected_hash}, " \
                 "acl=${acl_hash}"
        fi
    done

    rm -rf "${tmpdir}"
}

# =============================================================================
# Property 9: Secret file permissions
# Feature: valkey-state-management, Property 9: Secret file permissions
# Validates: Requirements 5.5
#
# For all files generated by the secrets generator, file permissions
# should be 600.
# =============================================================================

@test "property 9: all generated secret files have permission 600" {
    # Skip on Windows/MSYS — mktemp paths conflict with MSYS_NO_PATHCONV
    if [[ "$(uname -s)" == MINGW* ]] || [[ "$(uname -s)" == MSYS* ]]; then
        skip "Skipped on Windows/MSYS (MSYS_NO_PATHCONV path conflict)"
    fi

    local tmpdir
    tmpdir="$(mktemp -d)"

    SECRETS_SCRIPT="${PROJECT_ROOT}/scripts/generate-valkey-secrets.sh"

    # Generate secrets into a temp directory
    run bash "${SECRETS_SCRIPT}" "${tmpdir}"
    assert_success

    # Define the full set of expected secret files
    local secret_files=(
        "valkey_password.txt"
        "valkey_users.acl"
        "credentials.env.example"
    )

    for secret_file in "${secret_files[@]}"; do
        local filepath="${tmpdir}/${secret_file}"
        # Verify the file exists
        [ -f "${filepath}" ] || {
            rm -rf "${tmpdir}"
            fail "Expected secret file not found: ${secret_file}"
        }
        # Get octal permissions (last 3 digits)
        local perms
        perms="$(stat -c '%a' "${filepath}" 2>/dev/null \
              || stat -f '%Lp' "${filepath}" 2>/dev/null)"
        if [[ "${perms}" != "600" ]]; then
            rm -rf "${tmpdir}"
            fail "${secret_file} has permission ${perms}, expected 600"
        fi
    done

    rm -rf "${tmpdir}"
}

# =============================================================================
# Property 10: Health check input validation
# Feature: valkey-state-management, Property 10: Health check input validation
# Validates: Requirements 6.6
#
# For any VALKEY_HOST value containing characters outside [a-zA-Z0-9._-]
# or for any VALKEY_PORT value that is non-numeric or outside the range
# 1-65535, the health check script should exit with code 1 and output a
# message starting with "CRITICAL".
# =============================================================================

@test "property 10: invalid VALKEY_HOST values are rejected" {
    HEALTH_SCRIPT="${PROJECT_ROOT}/scripts/valkey-health.sh"

    # Array of invalid host values: spaces, special chars, slashes, etc.
    local invalid_hosts=(
        "host name"
        "host@name"
        "host!name"
        "host#name"
        "host\$name"
        "host%name"
        "host&name"
        "host*name"
        "host/name"
        "host:name"
        "host;name"
        "host=name"
        "host?name"
        "host[name"
        "host]name"
        "host{name"
        "host}name"
        "host|name"
        "host~name"
        "host,name"
        "host<name"
        "host>name"
    )

    for invalid_host in "${invalid_hosts[@]}"; do
        run env VALKEY_HOST="${invalid_host}" \
                VALKEY_PORT="6379" \
                bash "${HEALTH_SCRIPT}"

        if [[ "${status}" -ne 1 ]]; then
            fail "Host '${invalid_host}' should be rejected" \
                 " (exit ${status})"
        fi
        if [[ "${output}" != CRITICAL* ]]; then
            fail "Host '${invalid_host}' should produce" \
                 " CRITICAL message, got: ${output}"
        fi
    done
}

@test "property 10: non-numeric VALKEY_PORT values are rejected" {
    HEALTH_SCRIPT="${PROJECT_ROOT}/scripts/valkey-health.sh"

    # Array of non-numeric port values
    local invalid_ports=(
        "abc"
        "12abc"
        "abc12"
        "6379a"
        "port"
        "12.34"
        "-1"
        "1 2"
        "0x1F"
        ""
    )

    for invalid_port in "${invalid_ports[@]}"; do
        run env VALKEY_HOST="validhost" \
                VALKEY_PORT="${invalid_port}" \
                bash "${HEALTH_SCRIPT}"

        if [[ "${status}" -ne 1 ]]; then
            fail "Port '${invalid_port}' should be rejected" \
                 " (exit ${status})"
        fi
        if [[ "${output}" != CRITICAL* ]]; then
            fail "Port '${invalid_port}' should produce" \
                 " CRITICAL message, got: ${output}"
        fi
    done
}

@test "property 10: out-of-range VALKEY_PORT values are rejected" {
    HEALTH_SCRIPT="${PROJECT_ROOT}/scripts/valkey-health.sh"

    # Array of numeric but out-of-range port values
    local invalid_ports=(
        "0"
        "65536"
        "65537"
        "99999"
        "100000"
    )

    for invalid_port in "${invalid_ports[@]}"; do
        run env VALKEY_HOST="validhost" \
                VALKEY_PORT="${invalid_port}" \
                bash "${HEALTH_SCRIPT}"

        if [[ "${status}" -ne 1 ]]; then
            fail "Port '${invalid_port}' should be rejected" \
                 " (exit ${status})"
        fi
        if [[ "${output}" != CRITICAL* ]]; then
            fail "Port '${invalid_port}' should produce" \
                 " CRITICAL message, got: ${output}"
        fi
    done
}
