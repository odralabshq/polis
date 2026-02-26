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
# Requires: python3, sha256sum
# Exit codes: 0 = success, 1 = validation error, 2 = missing dependency
set -euo pipefail

AGENT_NAME="${1:?Usage: generate-agent.sh <agent-name> [agents-base-dir]}"
BASE_DIR="${2:-agents}"
AGENT_DIR="${BASE_DIR}/${AGENT_NAME}"
MANIFEST="${AGENT_DIR}/agent.yaml"
OUT_DIR="${AGENT_DIR}/.generated"

# ---------------------------------------------------------------------------
# Skip _template directory
# ---------------------------------------------------------------------------

if [[ "${AGENT_NAME}" == "_template" ]]; then
    echo "Skipping _template directory"
    exit 0
fi

# ---------------------------------------------------------------------------
# Dependency check
# ---------------------------------------------------------------------------

if ! command -v python3 &>/dev/null; then
    echo "Error: python3 is required" >&2
    exit 2
fi

if ! command -v sha256sum &>/dev/null; then
    echo "Error: sha256sum is required" >&2
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
# Parse manifest with python3 (no yq dependency)
# ---------------------------------------------------------------------------

# Use python3 to extract all fields from the YAML manifest at once.
# Outputs KEY=VALUE lines that are sourced into the shell.
_parse_manifest() {
    python3 - "${MANIFEST}" <<'PYEOF'
import sys, json

try:
    import yaml
    with open(sys.argv[1]) as f:
        data = yaml.safe_load(f)
except ImportError:
    # Fallback: minimal YAML parser for the subset used in agent.yaml manifests.
    # Handles block mappings, block sequences, and scalar values.
    import re

    def parse_yaml_simple(text):
        lines = text.splitlines()
        return _parse_block(lines, 0, 0)[0]

    def _unquote(s):
        s = s.strip()
        if len(s) >= 2 and ((s[0] == '"' and s[-1] == '"') or (s[0] == "'" and s[-1] == "'")):
            return s[1:-1]
        return s

    def _parse_block(lines, start, base_indent):
        result = None
        i = start
        while i < len(lines):
            raw = lines[i]
            stripped = raw.lstrip()
            if not stripped or stripped.startswith('#'):
                i += 1
                continue
            indent = len(raw) - len(raw.lstrip())
            if indent < base_indent:
                break
            if stripped.startswith('- '):
                if result is None:
                    result = []
                val = _unquote(stripped[2:].split('#')[0].strip())
                result.append(val)
                i += 1
            elif stripped == '-':
                if result is None:
                    result = []
                i += 1
                child, i = _parse_block(lines, i, indent + 2)
                result.append(child if child is not None else '')
            elif ':' in stripped:
                key_part, _, rest = stripped.partition(':')
                key = key_part.strip()
                rest = rest.strip()
                if result is None:
                    result = {}
                if not isinstance(result, dict):
                    result = {}
                if rest == '' or rest.startswith('#'):
                    i += 1
                    child, i = _parse_block(lines, i, indent + 2)
                    result[key] = child if child is not None else ''
                else:
                    result[key] = _unquote(rest.split('#')[0].strip())
                    i += 1
            else:
                i += 1
        return result, i

    with open(sys.argv[1]) as f:
        content = f.read()
    data = parse_yaml_simple(content)

if data is None:
    data = {}

def get(d, *keys, default=''):
    for k in keys:
        if not isinstance(d, dict):
            return default
        d = d.get(k, default)
        if d == default:
            return default
    return d if d is not None else default

meta   = data.get('metadata', {}) or {}
spec   = data.get('spec', {}) or {}
rt     = spec.get('runtime', {}) or {}
health = spec.get('health', {}) or {}
sec    = spec.get('security', {}) or {}
res    = spec.get('resources', {}) or {}
reqs   = spec.get('requirements', {}) or {}

def sh(v):
    """Escape a value for shell assignment (single-quote wrap)."""
    return "'" + str(v).replace("'", "'\\''") + "'"

fields = {
    'API_VERSION':        get(data, 'apiVersion'),
    'KIND':               get(data, 'kind'),
    'NAME':               get(meta, 'name'),
    'DISPLAY_NAME':       get(meta, 'displayName'),
    'PACKAGING':          get(spec, 'packaging'),
    'RUNTIME_CMD':        get(rt, 'command'),
    'RUNTIME_USER':       get(rt, 'user'),
    'RUNTIME_WORKDIR':    get(rt, 'workdir'),
    'RUNTIME_ENV_FILE':   get(rt, 'envFile'),
    'SPEC_INSTALL':       get(spec, 'install'),
    'SPEC_INIT':          get(spec, 'init'),
    'HEALTH_CMD':         get(health, 'command'),
    'HEALTH_INTERVAL':    get(health, 'interval', default='30s'),
    'HEALTH_TIMEOUT':     get(health, 'timeout', default='10s'),
    'HEALTH_RETRIES':     get(health, 'retries', default='3'),
    'HEALTH_START_PERIOD':get(health, 'startPeriod', default='60s'),
    'MEM_LIMIT':          get(res, 'memoryLimit'),
    'MEM_RESERVATION':    get(res, 'memoryReservation'),
    'PROTECT_SYSTEM':     get(sec, 'protectSystem', default='strict'),
    'PROTECT_HOME':       get(sec, 'protectHome', default='true'),
    'PRIVATE_TMP':        get(sec, 'privateTmp', default='true'),
    'MEM_MAX':            get(sec, 'memoryMax'),
    'CPU_QUOTA':          get(sec, 'cpuQuota'),
}

for k, v in fields.items():
    print(f"{k}={sh(v)}")

# Ports: emit as JSON array for shell to consume
ports = spec.get('ports', []) or []
ports_json = []
for p in ports:
    if isinstance(p, dict):
        ports_json.append({
            'container': str(p.get('container', '')),
            'hostEnv':   str(p.get('hostEnv', '')),
            'default':   str(p.get('default', '')),
        })
print(f"PORTS_JSON={sh(json.dumps(ports_json))}")

# Persistence
persist = spec.get('persistence', []) or []
persist_json = []
for p in persist:
    if isinstance(p, dict):
        persist_json.append({
            'name':          str(p.get('name', '')),
            'containerPath': str(p.get('containerPath', '')),
        })
print(f"PERSIST_JSON={sh(json.dumps(persist_json))}")

# Runtime env map
rt_env = rt.get('env', {}) or {}
rt_env_json = {}
if isinstance(rt_env, dict):
    rt_env_json = {str(k): str(v) for k, v in rt_env.items()}
print(f"RT_ENV_JSON={sh(json.dumps(rt_env_json))}")

# ReadWritePaths
rw_paths = sec.get('readWritePaths', []) or []
rw_json = [str(p) for p in rw_paths] if isinstance(rw_paths, list) else []
print(f"RW_PATHS_JSON={sh(json.dumps(rw_json))}")

# Requirements
env_one_of  = reqs.get('envOneOf', []) or []
env_optional = reqs.get('envOptional', []) or []
print(f"ENV_ONE_OF_JSON={sh(json.dumps([str(x) for x in env_one_of]))}")
print(f"ENV_OPTIONAL_JSON={sh(json.dumps([str(x) for x in env_optional]))}")
PYEOF
}

# Source the parsed fields into the current shell
eval "$(_parse_manifest)"

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

    # Check port conflicts using python3 to parse the JSON array
    while IFS= read -r port; do
        [[ -z "${port}" ]] && continue
        for platform_port in "${PLATFORM_PORTS[@]}"; do
            if [[ "${port}" == "${platform_port}" ]]; then
                echo "Error: Port ${port} conflicts with platform service" >&2
                errors=$((errors + 1))
            fi
        done
    done < <(python3 -c "
import json, sys
ports = json.loads(sys.argv[1])
for p in ports:
    print(p.get('default', ''))
" "${PORTS_JSON}")

    # Check readWritePaths
    while IFS= read -r rw_path; do
        [[ -z "${rw_path}" ]] && continue
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
    done < <(python3 -c "
import json, sys
paths = json.loads(sys.argv[1])
for p in paths:
    print(p)
" "${RW_PATHS_JSON}")

    return "${errors}"
}

# ---------------------------------------------------------------------------
# Generate compose.agent.yaml
# ---------------------------------------------------------------------------

gen_compose() {
    local out="${OUT_DIR}/compose.agent.yaml"

    # Build ports, volumes, and socat services via python3
    python3 - "${PORTS_JSON}" "${PERSIST_JSON}" "${NAME}" "${MANIFEST}" \
              "${HEALTH_CMD}" "${HEALTH_INTERVAL}" "${HEALTH_TIMEOUT}" \
              "${HEALTH_RETRIES}" "${HEALTH_START_PERIOD}" \
              "${MEM_LIMIT}" "${MEM_RESERVATION}" > "${out}" <<'PYEOF'
import sys, json

ports_json    = sys.argv[1]
persist_json  = sys.argv[2]
name          = sys.argv[3]
manifest_path = sys.argv[4]
health_cmd    = sys.argv[5]
health_interval    = sys.argv[6]
health_timeout     = sys.argv[7]
health_retries     = sys.argv[8]
health_start_period = sys.argv[9]
mem_limit     = sys.argv[10]
mem_reservation = sys.argv[11]

ports   = json.loads(ports_json)
persist = json.loads(persist_json)

lines = []
lines.append(f"# Generated from {manifest_path} - DO NOT EDIT")
lines.append("services:")
lines.append("  workspace:")
lines.append("    env_file:")
lines.append("      - .env")
lines.append("    volumes:")
lines.append(f"      - ./agents/{name}/:/opt/agents/{name}/:ro")
lines.append(f"      - ./agents/{name}/.generated/{name}.service:/etc/systemd/system/{name}.service:ro")
lines.append(f"      - ./agents/{name}/.generated/{name}.service.sha256:/etc/systemd/system/{name}.service.sha256:ro")

for p in persist:
    vol_name = p['name']
    container_path = p['containerPath']
    lines.append(f"      - polis-agent-{name}-{vol_name}:{container_path}")

healthcheck_test = (
    f"systemctl is-active polis-init.service && "
    f"systemctl is-active {name}.service && "
    f"{health_cmd} && "
    f"ip route | grep -q default"
)
lines.append("    healthcheck:")
lines.append(f'      test: ["CMD-SHELL", "{healthcheck_test}"]')
lines.append(f"      interval: {health_interval}")
lines.append(f"      timeout: {health_timeout}")
lines.append(f"      retries: {health_retries}")
lines.append(f"      start_period: {health_start_period}")

if mem_limit or mem_reservation:
    lines.append("    deploy:")
    lines.append("      resources:")
    if mem_limit:
        lines.append("        limits:")
        lines.append(f"          memory: {mem_limit}")
    if mem_reservation:
        lines.append("        reservations:")
        lines.append(f"          memory: {mem_reservation}")

# Socat proxy sidecars
if ports:
    lines.append("")
    for p in ports:
        container_port = p['container']
        host_env       = p['hostEnv']
        default_port   = p['default']
        lines.append(f"  {name}-proxy-{container_port}:")
        lines.append(f"    image: alpine/socat:latest")
        lines.append(f"    restart: unless-stopped")
        lines.append(f"    ports:")
        lines.append(f'      - "${{{host_env}:-{default_port}}}:{container_port}"')
        lines.append(f"    command: TCP-LISTEN:{container_port},fork,reuseaddr TCP:polis-workspace:{container_port}")
        lines.append(f"    networks:")
        lines.append(f"      - internal-bridge")
        lines.append(f"      - default")
        lines.append(f"    depends_on:")
        lines.append(f"      - workspace")

# Top-level volumes
if persist:
    lines.append("")
    lines.append("volumes:")
    for p in persist:
        vol_name = p['name']
        lines.append(f"  polis-agent-{name}-{vol_name}:")
        lines.append(f"    name: polis-agent-{name}-{vol_name}")

print('\n'.join(lines))
PYEOF
}

# ---------------------------------------------------------------------------
# Generate <name>.service (systemd unit)
# ---------------------------------------------------------------------------

gen_systemd() {
    local out="${OUT_DIR}/${NAME}.service"

    python3 - "${RT_ENV_JSON}" "${RW_PATHS_JSON}" \
              "${NAME}" "${DISPLAY_NAME}" "${RUNTIME_USER}" \
              "${RUNTIME_WORKDIR}" "${RUNTIME_ENV_FILE}" \
              "${SPEC_INIT}" "${RUNTIME_CMD}" \
              "${PROTECT_SYSTEM}" "${PROTECT_HOME}" "${PRIVATE_TMP}" \
              "${MEM_MAX}" "${CPU_QUOTA}" "${MANIFEST}" > "${out}" <<'PYEOF'
import sys, json

rt_env_json    = sys.argv[1]
rw_paths_json  = sys.argv[2]
name           = sys.argv[3]
display_name   = sys.argv[4]
runtime_user   = sys.argv[5]
runtime_workdir = sys.argv[6]
runtime_env_file = sys.argv[7]
spec_init      = sys.argv[8]
runtime_cmd    = sys.argv[9]
protect_system = sys.argv[10]
protect_home   = sys.argv[11]
private_tmp    = sys.argv[12]
mem_max        = sys.argv[13]
cpu_quota      = sys.argv[14]
manifest_path  = sys.argv[15]

rt_env   = json.loads(rt_env_json)
rw_paths = json.loads(rw_paths_json)

lines = []
lines.append(f"# Generated from {manifest_path} - DO NOT EDIT")
lines.append("[Unit]")
if display_name:
    lines.append(f"Description={display_name}")
lines.append("After=network-online.target polis-init.service")
lines.append("Wants=network-online.target")
lines.append("Requires=polis-init.service")
lines.append("StartLimitIntervalSec=300")
lines.append("StartLimitBurst=3")
lines.append("")
lines.append("[Service]")
lines.append("Type=simple")
lines.append(f"User={runtime_user}")
if runtime_workdir:
    lines.append(f"WorkingDirectory={runtime_workdir}")
lines.append("")
if runtime_env_file:
    lines.append(f"EnvironmentFile=-{runtime_env_file}")
lines.append("")
lines.append("Environment=NODE_EXTRA_CA_CERTS=/usr/local/share/ca-certificates/polis-ca.crt")
lines.append("Environment=SSL_CERT_FILE=/usr/local/share/ca-certificates/polis-ca.crt")
lines.append("Environment=REQUESTS_CA_BUNDLE=/usr/local/share/ca-certificates/polis-ca.crt")
for k, v in rt_env.items():
    lines.append(f'Environment="{k}={v}"')
lines.append("")
if spec_init:
    lines.append(f"ExecStartPre=+/bin/bash /opt/agents/{name}/{spec_init}")
lines.append(f"ExecStart={runtime_cmd}")
lines.append("")
lines.append("Restart=always")
lines.append("RestartSec=5")
lines.append("StartLimitBurst=3")
lines.append("")
lines.append("NoNewPrivileges=true")
lines.append(f"ProtectSystem={protect_system}")
lines.append(f"ProtectHome={protect_home}")
if rw_paths:
    lines.append(f"ReadWritePaths={' '.join(rw_paths)}")
lines.append(f"PrivateTmp={private_tmp}")
if mem_max:
    lines.append(f"MemoryMax={mem_max}")
if cpu_quota:
    lines.append(f"CPUQuota={cpu_quota}")
lines.append("")
lines.append("[Install]")
lines.append("WantedBy=multi-user.target")

print('\n'.join(lines))
PYEOF
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

    python3 - "${ENV_ONE_OF_JSON}" "${ENV_OPTIONAL_JSON}" "${env_file}" > "${out}" <<'PYEOF'
import sys, json, os

env_one_of_json  = sys.argv[1]
env_optional_json = sys.argv[2]
env_file         = sys.argv[3]

declared_keys = set(json.loads(env_one_of_json)) | set(json.loads(env_optional_json))

if not os.path.isfile(env_file):
    sys.exit(0)

with open(env_file) as f:
    for line in f:
        line = line.rstrip('\n')
        stripped = line.strip()
        if not stripped or stripped.startswith('#'):
            continue
        key = line.split('=', 1)[0]
        if key in declared_keys:
            print(line)
PYEOF

    echo "✓ Generated artifacts for '${NAME}' in .generated/"
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
