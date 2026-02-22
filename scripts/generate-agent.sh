#!/usr/bin/env bash
# generate-agent.sh - Generate runtime artifacts from an agent manifest.
#
# Usage: generate-agent.sh <agent-name> [agents-base-dir]
#
# Reads:  <agents-base-dir>/<agent-name>/agent.yaml
#         .env (repo root, for filtering agent env vars)
#
# Writes to <agents-base-dir>/<agent-name>/.generated/:
#   compose.agent.yaml       Docker Compose overlay for workspace service
#   <name>.service           systemd unit for agent process inside workspace
#   <name>.service.sha256    SHA-256 integrity hash of the .service file
#   <name>.env               Filtered env vars (only those declared in manifest)
#
# Requires: yq v4+, sha256sum
# Exit codes: 0 = success, 1 = validation error, 2 = missing dependency
set -euo pipefail

AGENT_NAME="${1:?Usage: generate-agent.sh <agent-name> [agents-base-dir]}"
BASE_DIR="${2:-./agents}"
AGENT_DIR="${BASE_DIR}/${AGENT_NAME}"
MANIFEST="${AGENT_DIR}/agent.yaml"
OUT_DIR="${AGENT_DIR}/.generated"

# ---------------------------------------------------------------------------
# Dependency check
# ---------------------------------------------------------------------------

if ! command -v yq &>/dev/null || ! yq --version &>/dev/null 2>&1; then
    echo "Error: yq is required. Install from https://github.com/mikefarah/yq#install" >&2
    exit 2
fi

# ---------------------------------------------------------------------------
# Manifest existence
# ---------------------------------------------------------------------------

if [[ ! -f "${MANIFEST}" ]]; then
    echo "Error: ${MANIFEST} not found" >&2
    exit 1
fi

# ---------------------------------------------------------------------------
# Read all fields before validation
# ---------------------------------------------------------------------------

API_VERSION=$(yq '.apiVersion' "${MANIFEST}")
KIND=$(yq '.kind' "${MANIFEST}")
NAME=$(yq '.metadata.name' "${MANIFEST}")
DISPLAY_NAME=$(yq '.metadata.displayName // ""' "${MANIFEST}")
PACKAGING=$(yq '.spec.packaging' "${MANIFEST}")
RUNTIME_CMD=$(yq '.spec.runtime.command' "${MANIFEST}")
RUNTIME_USER=$(yq '.spec.runtime.user' "${MANIFEST}")
RUNTIME_WORKDIR=$(yq '.spec.runtime.workdir // ""' "${MANIFEST}")
RUNTIME_ENV_FILE=$(yq '.spec.runtime.envFile // ""' "${MANIFEST}")
SPEC_INSTALL=$(yq '.spec.install // ""' "${MANIFEST}")
SPEC_INIT=$(yq '.spec.init // ""' "${MANIFEST}")
HEALTH_CMD=$(yq '.spec.health.command // ""' "${MANIFEST}")
HEALTH_INTERVAL=$(yq '.spec.health.interval // "30s"' "${MANIFEST}")
HEALTH_TIMEOUT=$(yq '.spec.health.timeout // "10s"' "${MANIFEST}")
HEALTH_RETRIES=$(yq '.spec.health.retries // "3"' "${MANIFEST}")
HEALTH_START_PERIOD=$(yq '.spec.health.startPeriod // "60s"' "${MANIFEST}")
MEM_LIMIT=$(yq '.spec.resources.memoryLimit // ""' "${MANIFEST}")
MEM_RESERVATION=$(yq '.spec.resources.memoryReservation // ""' "${MANIFEST}")
PROTECT_SYSTEM=$(yq '.spec.security.protectSystem // "strict"' "${MANIFEST}")
PROTECT_HOME=$(yq '.spec.security.protectHome // "true"' "${MANIFEST}")
PRIVATE_TMP=$(yq '.spec.security.privateTmp // "true"' "${MANIFEST}")
MEM_MAX=$(yq '.spec.security.memoryMax // ""' "${MANIFEST}")
CPU_QUOTA=$(yq '.spec.security.cpuQuota // ""' "${MANIFEST}")

# ---------------------------------------------------------------------------
# Validation — collect ALL errors before any file generation
# ---------------------------------------------------------------------------

PLATFORM_PORTS=(53 1344 6379 8080 18080)
ALLOWED_RW_PREFIXES=("/home/polis/" "/tmp/" "/var/lib/" "/var/log/")

validate_manifest() {
    local errors=0

    if [[ "${API_VERSION}" != "polis.dev/v1" ]]; then
        echo "Error: Unsupported apiVersion. Expected polis.dev/v1" >&2
        errors=$((errors + 1))
    fi

    if [[ "${KIND}" != "AgentPlugin" ]]; then
        echo "Error: Unsupported kind. Expected AgentPlugin" >&2
        errors=$((errors + 1))
    fi

    if ! [[ "${NAME}" =~ ^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$ ]]; then
        echo "Error: metadata.name must be lowercase alphanumeric with hyphens" >&2
        errors=$((errors + 1))
    fi

    if [[ "${PACKAGING}" != "script" ]]; then
        echo "Error: Only 'script' packaging is supported" >&2
        errors=$((errors + 1))
    fi

    if [[ "${RUNTIME_CMD}" != /* ]]; then
        echo "Error: runtime.command must start with /" >&2
        errors=$((errors + 1))
    fi

    if [[ "${RUNTIME_CMD}" =~ [';|&`$()\\'] ]]; then
        echo "Error: runtime.command contains shell metacharacters" >&2
        errors=$((errors + 1))
    fi

    if [[ "${RUNTIME_USER}" == "root" ]]; then
        echo "Error: Agents must run as unprivileged user" >&2
        errors=$((errors + 1))
    fi

    if [[ -n "${SPEC_INSTALL}" && "${SPEC_INSTALL}" == *..* ]]; then
        echo "Error: Path escapes agent directory" >&2
        errors=$((errors + 1))
    fi

    if [[ -n "${SPEC_INIT}" && "${SPEC_INIT}" == *..* ]]; then
        echo "Error: Path escapes agent directory" >&2
        errors=$((errors + 1))
    fi

    local port_count
    port_count=$(yq '.spec.ports | length' "${MANIFEST}")
    for ((i=0; i<port_count; i++)); do
        local port
        port=$(yq ".spec.ports[${i}].default" "${MANIFEST}")
        for platform_port in "${PLATFORM_PORTS[@]}"; do
            if [[ "${port}" == "${platform_port}" ]]; then
                echo "Error: Port ${port} conflicts with platform service" >&2
                errors=$((errors + 1))
            fi
        done
    done

    local rw_count
    rw_count=$(yq '.spec.security.readWritePaths | length' "${MANIFEST}")
    for ((i=0; i<rw_count; i++)); do
        local rw_path
        rw_path=$(yq ".spec.security.readWritePaths[${i}]" "${MANIFEST}")
        local allowed=false
        for prefix in "${ALLOWED_RW_PREFIXES[@]}"; do
            if [[ "${rw_path}" == "${prefix}"* ]]; then
                allowed=true
                break
            fi
        done
        if [[ "${allowed}" == false ]]; then
            echo "Error: Path outside allowed prefixes: ${rw_path}" >&2
            errors=$((errors + 1))
        fi
    done

    return "${errors}"
}

# ---------------------------------------------------------------------------
# Generate compose.agent.yaml
# ---------------------------------------------------------------------------

gen_compose() {
    local out="${OUT_DIR}/compose.agent.yaml"

    # Build ports section lines
    local ports_yaml=""
    local port_count
    port_count=$(yq '.spec.ports | length' "${MANIFEST}")
    for ((i=0; i<port_count; i++)); do
        local container_port host_env default_port
        container_port=$(yq ".spec.ports[${i}].container" "${MANIFEST}")
        host_env=$(yq ".spec.ports[${i}].hostEnv" "${MANIFEST}")
        default_port=$(yq ".spec.ports[${i}].default" "${MANIFEST}")
        ports_yaml="${ports_yaml}      - \"\${${host_env}:-${default_port}}:${container_port}\"
"
    done

    # Build persistence volume mounts and top-level volumes section
    local volumes_mounts_yaml=""
    local vol_section_yaml=""
    local persist_count
    persist_count=$(yq '.spec.persistence | length' "${MANIFEST}")
    for ((i=0; i<persist_count; i++)); do
        local vol_name container_path
        vol_name=$(yq ".spec.persistence[${i}].name" "${MANIFEST}")
        container_path=$(yq ".spec.persistence[${i}].containerPath" "${MANIFEST}")
        volumes_mounts_yaml="${volumes_mounts_yaml}      - polis-agent-${NAME}-${vol_name}:${container_path}
"
        vol_section_yaml="${vol_section_yaml}  polis-agent-${NAME}-${vol_name}:
    name: polis-agent-${NAME}-${vol_name}
"
    done

    local healthcheck_test="systemctl is-active polis-init.service && systemctl is-active ${NAME}.service && ${HEALTH_CMD} && ip route | grep -q default"

    # Socat proxy runs as a VM-level systemd service (see gen_proxy below).
    # No container-level proxy needed.
    local socat_services_yaml=""

    {
        echo "# Generated from ${MANIFEST} - DO NOT EDIT"
        echo "services:"
        echo "  workspace:"
        if [[ -n "${ports_yaml}" ]]; then
            echo "    ports:"
            printf '%s' "${ports_yaml}"
        fi
        echo "    env_file:"
        echo "      - .env"
        echo "    volumes:"
        echo "      - ./agents/${NAME}/:/opt/agents/${NAME}/:ro"
        echo "      - ./agents/${NAME}/.generated/${NAME}.service:/etc/systemd/system/${NAME}.service:ro"
        echo "      - ./agents/${NAME}/.generated/${NAME}.service.sha256:/etc/systemd/system/${NAME}.service.sha256:ro"
        printf '%s' "${volumes_mounts_yaml}"
        echo "    healthcheck:"
        echo "      test: [\"CMD-SHELL\", \"${healthcheck_test}\"]"
        echo "      interval: ${HEALTH_INTERVAL}"
        echo "      timeout: ${HEALTH_TIMEOUT}"
        echo "      retries: ${HEALTH_RETRIES}"
        echo "      start_period: ${HEALTH_START_PERIOD}"
        if [[ -n "${MEM_LIMIT}" || -n "${MEM_RESERVATION}" ]]; then
            echo "    deploy:"
            echo "      resources:"
            if [[ -n "${MEM_LIMIT}" ]]; then
                echo "        limits:"
                echo "          memory: ${MEM_LIMIT}"
            fi
            if [[ -n "${MEM_RESERVATION}" ]]; then
                echo "        reservations:"
                echo "          memory: ${MEM_RESERVATION}"
            fi
        fi
        if [[ -n "${socat_services_yaml}" ]]; then
            echo ""
            printf '%s' "${socat_services_yaml}"
        fi
        if [[ -n "${vol_section_yaml}" ]]; then
            echo ""
            echo "volumes:"
            printf '%s' "${vol_section_yaml}"
        fi
    } > "${out}"
}

# ---------------------------------------------------------------------------
# Generate <name>.service (systemd unit)
# ---------------------------------------------------------------------------

gen_systemd() {
    local out="${OUT_DIR}/${NAME}.service"

    # Build Environment= lines from spec.runtime.env map
    local env_lines=""
    local env_count
    env_count=$(yq '.spec.runtime.env | length' "${MANIFEST}")
    if [[ "${env_count}" -gt 0 ]]; then
        while IFS= read -r line; do
            env_lines="${env_lines}Environment=\"${line}\"
"
        done < <(yq '.spec.runtime.env | to_entries | .[] | .key + "=" + .value' "${MANIFEST}")
    fi

    # Build ReadWritePaths value
    local rw_paths=""
    local rw_count
    rw_count=$(yq '.spec.security.readWritePaths | length' "${MANIFEST}")
    if [[ "${rw_count}" -gt 0 ]]; then
        rw_paths=$(yq '.spec.security.readWritePaths | join(" ")' "${MANIFEST}")
    fi

    {
        echo "# Generated from ${MANIFEST} - DO NOT EDIT"
        echo "[Unit]"
        if [[ -n "${DISPLAY_NAME}" ]]; then
            echo "Description=${DISPLAY_NAME}"
        fi
        echo "After=network-online.target polis-init.service"
        echo "Wants=network-online.target"
        echo "Requires=polis-init.service"
        echo "StartLimitIntervalSec=300"
        echo "StartLimitBurst=3"
        echo ""
        echo "[Service]"
        echo "Type=simple"
        echo "User=${RUNTIME_USER}"
        if [[ -n "${RUNTIME_WORKDIR}" ]]; then
            echo "WorkingDirectory=${RUNTIME_WORKDIR}"
        fi
        echo ""
        if [[ -n "${RUNTIME_ENV_FILE}" ]]; then
            echo "EnvironmentFile=-${RUNTIME_ENV_FILE}"
        fi
        echo ""
        echo "Environment=NODE_EXTRA_CA_CERTS=/etc/ssl/certs/polis-ca.crt"
        echo "Environment=SSL_CERT_FILE=/etc/ssl/certs/polis-ca.crt"
        echo "Environment=REQUESTS_CA_BUNDLE=/etc/ssl/certs/polis-ca.crt"
        if [[ -n "${env_lines}" ]]; then
            printf '%s' "${env_lines}"
        fi
        echo ""
        if [[ -n "${SPEC_INIT}" ]]; then
            echo "ExecStartPre=+/bin/bash /opt/agents/${NAME}/${SPEC_INIT}"
        fi
        echo "ExecStart=${RUNTIME_CMD}"
        echo ""
        echo "Restart=on-failure"
        echo "RestartSec=5"
        echo "StartLimitBurst=3"
        echo ""
        echo "NoNewPrivileges=true"
        echo "ProtectSystem=${PROTECT_SYSTEM}"
        echo "ProtectHome=${PROTECT_HOME}"
        if [[ -n "${rw_paths}" ]]; then
            echo "ReadWritePaths=${rw_paths}"
        fi
        echo "PrivateTmp=${PRIVATE_TMP}"
        if [[ -n "${MEM_MAX}" ]]; then
            echo "MemoryMax=${MEM_MAX}"
        fi
        if [[ -n "${CPU_QUOTA}" ]]; then
            echo "CPUQuota=${CPU_QUOTA}"
        fi
        echo ""
        echo "[Install]"
        echo "WantedBy=multi-user.target"
    } > "${out}"
}

# ---------------------------------------------------------------------------
# Generate <name>.service.sha256
# ---------------------------------------------------------------------------

gen_hash() {
    sha256sum "${OUT_DIR}/${NAME}.service" | awk '{print $1}' > "${OUT_DIR}/${NAME}.service.sha256"
}

# ---------------------------------------------------------------------------
# Generate <name>.env (filtered from repo root .env)
# ---------------------------------------------------------------------------

gen_env() {
    local out="${OUT_DIR}/${NAME}.env"
    local env_file=".env"

    # Collect all declared keys from manifest
    local declared_keys=()
    local one_of_count
    one_of_count=$(yq '.spec.requirements.envOneOf | length' "${MANIFEST}")
    for ((i=0; i<one_of_count; i++)); do
        declared_keys+=("$(yq ".spec.requirements.envOneOf[${i}]" "${MANIFEST}")")
    done
    local optional_count
    optional_count=$(yq '.spec.requirements.envOptional | length' "${MANIFEST}")
    for ((i=0; i<optional_count; i++)); do
        declared_keys+=("$(yq ".spec.requirements.envOptional[${i}]" "${MANIFEST}")")
    done

    # Write filtered env file
    : > "${out}"
    if [[ -f "${env_file}" ]]; then
        while IFS= read -r line || [[ -n "${line}" ]]; do
            # Skip comments and blank lines
            [[ "${line}" =~ ^[[:space:]]*# ]] && continue
            [[ -z "${line}" ]] && continue
            local key="${line%%=*}"
            for declared in "${declared_keys[@]}"; do
                if [[ "${key}" == "${declared}" ]]; then
                    echo "${line}" >> "${out}"
                    break
                fi
            done
        done < "${env_file}"
    fi

    echo "✓ Generated artifacts for '${NAME}' in .generated/"
}

# ---------------------------------------------------------------------------
# Generate VM-level socat proxy units (one per port)
# These run on the VM host (not in a container) to create real TCP listeners
# that Hyper-V's virtual switch can route to.
# ---------------------------------------------------------------------------

gen_proxy() {
    local port_count
    port_count=$(yq '.spec.ports | length' "${MANIFEST}")
    for ((i=0; i<port_count; i++)); do
        local container_port host_env default_port
        container_port=$(yq ".spec.ports[${i}].container" "${MANIFEST}")
        host_env=$(yq ".spec.ports[${i}].hostEnv" "${MANIFEST}")
        default_port=$(yq ".spec.ports[${i}].default" "${MANIFEST}")
        local out="${OUT_DIR}/${NAME}-proxy-${container_port}.service"
        {
            echo "# Generated from ${MANIFEST} - DO NOT EDIT"
            echo "# Install on the VM host: cp ${out} /etc/systemd/system/"
            echo "[Unit]"
            echo "Description=Polis socat proxy for ${NAME} port ${container_port}"
            echo "After=docker.service"
            echo "Requires=docker.service"
            echo ""
            echo "[Service]"
            echo "Type=simple"
            echo "Restart=always"
            echo "RestartSec=3"
            echo "ExecStart=/bin/sh -c 'IP=\$(docker inspect polis-workspace --format \"{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}\"); exec /usr/bin/socat TCP-LISTEN:\${${host_env}:-${default_port}},fork,reuseaddr TCP:\$IP:${container_port}'"
            echo ""
            echo "[Install]"
            echo "WantedBy=multi-user.target"
        } > "${out}"
    done
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

if ! validate_manifest; then
    exit 1
fi

mkdir -p "${OUT_DIR}"
gen_compose
gen_systemd
gen_hash
gen_env
gen_proxy
