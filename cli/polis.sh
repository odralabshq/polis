#!/bin/bash
# Polis - Management Script

set -euo pipefail

# Define project root — resolve symlinks so this works when called via ~/.local/bin/polis
SOURCE="${BASH_SOURCE[0]}"
while [[ -L "$SOURCE" ]]; do
    DIR="$(cd "$(dirname "$SOURCE")" && pwd)"
    SOURCE="$(readlink "$SOURCE")"
    [[ "$SOURCE" != /* ]] && SOURCE="$DIR/$SOURCE"
done
SCRIPT_DIR="$(cd "$(dirname "$SOURCE")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${SCRIPT_DIR}"

# Compose file path
COMPOSE_FILE="${PROJECT_ROOT}/docker-compose.yml"
ENV_FILE="${PROJECT_ROOT}/.env"

# Parse flags
NO_CACHE=""
LOCAL_BUILD=""
AGENT_NAME=""
for arg in "$@"; do
    case "$arg" in
        --no-cache)
            NO_CACHE="--no-cache"
            ;;
        --local)
            LOCAL_BUILD="true"
            ;;
        --agent=*)
            AGENT_NAME="${arg#*=}"
            ;;
        --profile=*)
            # Backward compat: map --profile to --agent with deprecation warning
            AGENT_NAME="${arg#*=}"
            log_warn "--profile is deprecated. Use --agent=${AGENT_NAME} instead."
            ;;
    esac
done

# Sysbox version
SYSBOX_VERSION="0.6.7"



# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m' # No Color

log_info() { echo -e "${BLUE}[INFO]${NC} $*"; }
log_success() { echo -e "${GREEN}[OK]${NC} $*"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }
log_error() { echo -e "${RED}[ERROR]${NC} $*" >&2; }
log_step() { echo -e "${CYAN}[STEP]${NC} $*"; }

# Detect architecture
detect_arch() {
    local arch
    arch=$(uname -m)
    case "$arch" in
        x86_64)  echo "amd64" ;;
        aarch64) echo "arm64" ;;
        *)
            log_error "Unsupported architecture: $arch"
            return 1
            ;;
    esac
}


# Check Docker version compatibility
check_docker_version() {
    if ! command -v docker &>/dev/null; then
        log_error "Docker is not installed."
        echo ""
        echo "Install Docker first:"
        echo "  https://docs.docker.com/engine/install/"
        return 1
    fi
    
    if ! docker info &>/dev/null; then
        log_error "Docker is not running or not accessible."
        echo ""
        echo "Start Docker with:"
        echo "  sudo systemctl start docker"
        return 1
    fi
    
    local docker_version
    docker_version=$(docker version --format '{{.Server.Version}}' 2>/dev/null || echo "unknown")
    
    log_info "Docker version: $docker_version"
    
    return 0
}

# Check if sysbox is installed and configured
check_sysbox() {
    # Check if sysbox-runc binary exists
    if ! command -v sysbox-runc &>/dev/null; then
        return 1
    fi
    
    # Check if Docker knows about sysbox runtime
    if ! docker info 2>/dev/null | grep -qi "sysbox"; then
        return 1
    fi
    
    return 0
}

# Ensure sysbox services are running
ensure_sysbox_running() {
    # Check if systemctl is available
    if ! command -v systemctl &>/dev/null; then
        return 0
    fi
    
    local needs_restart=false
    
    # Check sysbox-mgr
    if ! systemctl is-active --quiet sysbox-mgr; then
        log_info "Starting sysbox-mgr service..."
        sudo systemctl start sysbox-mgr
        needs_restart=true
    fi
    
    # Check sysbox-fs
    if ! systemctl is-active --quiet sysbox-fs; then
        log_info "Starting sysbox-fs service..."
        sudo systemctl start sysbox-fs
        needs_restart=true
    fi
    
    # If we started sysbox services, give them a moment
    if [[ "$needs_restart" == "true" ]]; then
        sleep 2
        # Verify they started
        if ! systemctl is-active --quiet sysbox-mgr || ! systemctl is-active --quiet sysbox-fs; then
            log_error "Failed to start sysbox services"
            sudo systemctl status sysbox-mgr sysbox-fs --no-pager
            return 1
        fi
        log_success "Sysbox services started successfully."
    fi
    
    return 0
}

# Install sysbox runtime
install_sysbox() {
    echo "=== Installing Sysbox Runtime ==="
    
    local arch
    arch=$(detect_arch) || return 1
    
    # Check if already installed
    if check_sysbox; then
        log_success "Sysbox is already installed and configured."
        return 0
    fi
    
    # Check for Debian/Ubuntu
    if ! command -v apt-get &>/dev/null; then
        log_error "Sysbox installation requires apt-get (Debian/Ubuntu)."
        echo "For other distributions, please install Sysbox manually:"
        echo "  https://github.com/nestybox/sysbox/blob/master/docs/user-guide/install-package.md"
        return 1
    fi
    
    local deb_file="sysbox-ce_${SYSBOX_VERSION}-0.linux_${arch}.deb"
    local download_url="https://downloads.nestybox.com/sysbox/releases/v${SYSBOX_VERSION}/${deb_file}"
    local tmp_dir
    tmp_dir=$(mktemp -d)
    
    log_info "Downloading Sysbox v${SYSBOX_VERSION} for ${arch}..."
    if ! wget -q --show-progress -O "${tmp_dir}/${deb_file}" "${download_url}"; then
        log_error "Failed to download Sysbox from ${download_url}"
        rm -rf "${tmp_dir}"
        return 1
    fi
    
    log_info "Installing Sysbox (requires sudo)..."
    if ! sudo apt-get install -y jq "${tmp_dir}/${deb_file}"; then
        log_error "Failed to install Sysbox package"
        rm -rf "${tmp_dir}"
        return 1
    fi
    
    rm -rf "${tmp_dir}"
    log_success "Sysbox installed successfully."
    return 0
}

# Configure Docker to use sysbox runtime
configure_docker_sysbox() {
    echo "=== Configuring Docker for Sysbox ==="
    
    local daemon_json="/etc/docker/daemon.json"
    local sysbox_config='{"runtimes":{"sysbox-runc":{"path":"/usr/bin/sysbox-runc"}}}'
    
    # Check if Docker is running
    if ! docker info &>/dev/null; then
        log_error "Docker is not running. Please start Docker first."
        return 1
    fi
    
    # Check if sysbox is already configured
    if docker info 2>/dev/null | grep -qi "sysbox"; then
        log_success "Docker is already configured with Sysbox runtime."
        return 0
    fi
    
    log_info "Configuring Docker daemon (requires sudo)..."
    
    # Create /etc/docker if it doesn't exist
    sudo mkdir -p /etc/docker
    
    if [[ -f "$daemon_json" ]]; then
        # Merge with existing config
        log_info "Merging Sysbox config with existing daemon.json..."
        local existing_config
        existing_config=$(cat "$daemon_json")
        
        # Use jq to merge if available, otherwise warn and overwrite
        if command -v jq &>/dev/null; then
            local merged_config
            merged_config=$(echo "$existing_config" | jq --argjson sysbox '{"runtimes":{"sysbox-runc":{"path":"/usr/bin/sysbox-runc"}}}' '. * $sysbox')
            echo "$merged_config" | sudo tee "$daemon_json" > /dev/null
        else
            log_warn "jq not found. Backing up existing config and creating new one."
            sudo cp "$daemon_json" "${daemon_json}.backup.$(date +%Y%m%d%H%M%S)"
            echo "$sysbox_config" | sudo tee "$daemon_json" > /dev/null
        fi
    else
        echo "$sysbox_config" | sudo tee "$daemon_json" > /dev/null
    fi
    
    # Full stop/start cycle is required for Docker to pick up new runtimes
    # from daemon.json. A simple restart often doesn't work.
    log_info "Stopping Docker daemon..."
    if command -v systemctl &>/dev/null; then
        sudo systemctl stop docker docker.socket 2>/dev/null || true
    elif command -v service &>/dev/null; then
        sudo service docker stop 2>/dev/null || true
    fi
    
    # Ensure sysbox services are running before Docker starts
    log_info "Restarting Sysbox services..."
    if command -v systemctl &>/dev/null; then
        sudo systemctl restart sysbox-mgr sysbox-fs 2>/dev/null || true
        sleep 2
    fi
    
    log_info "Starting Docker daemon..."
    if command -v systemctl &>/dev/null; then
        sudo systemctl start docker
    elif command -v service &>/dev/null; then
        sudo service docker start
    else
        log_warn "Could not restart Docker automatically."
        echo "Please restart Docker manually, then re-run this command."
        return 1
    fi
    
    # Wait for Docker to come back up
    log_info "Waiting for Docker to start..."
    local retries=30
    while ! docker info &>/dev/null && [[ $retries -gt 0 ]]; do
        sleep 1
        ((retries--))
    done
    
    if ! docker info &>/dev/null; then
        log_error "Docker failed to start. Check 'journalctl -u docker' for details."
        return 1
    fi
    
    # Verify sysbox is available
    if docker info 2>/dev/null | grep -qi "sysbox"; then
        log_success "Docker configured with Sysbox runtime successfully."
        return 0
    else
        log_error "Sysbox runtime not detected after configuration."
        return 1
    fi
}

# Setup sysbox (install + configure)
setup_sysbox() {
    echo ""
    echo "=== Sysbox Runtime Setup ==="
    echo ""
    
    # Ensure sysbox services are running
    if command -v sysbox-runc &>/dev/null; then
        if ! ensure_sysbox_running; then
            return 1
        fi
    fi
    
    # Check if already fully configured
    if check_sysbox; then
        log_success "Sysbox is already installed and configured."
        docker info 2>/dev/null | grep -i "sysbox" | head -1
        return 0
    fi
    
    # Install sysbox
    if ! install_sysbox; then
        echo ""
        log_error "Sysbox installation failed."
        echo "The workspace container requires Sysbox for Docker-in-Docker support."
        echo ""
        echo "Manual installation:"
        echo "  https://github.com/nestybox/sysbox/blob/master/docs/user-guide/install-package.md"
        return 1
    fi
    
    # Configure Docker
    if ! configure_docker_sysbox; then
        echo ""
        log_error "Docker configuration for Sysbox failed."
        return 1
    fi
    
    echo ""
    log_success "Sysbox setup complete."
    return 0
}

# Generate CA certificate if not exists
generate_ca() {
    local CA_DIR="${PROJECT_ROOT}/certs/ca"
    local CA_KEY="${CA_DIR}/ca.key"
    local CA_PEM="${CA_DIR}/ca.pem"
    
    # Handle partial CA files (corruption/partial deletion)
    if [[ -f "$CA_KEY" ]] || [[ -f "$CA_PEM" ]]; then
        if [[ ! -f "$CA_KEY" ]] || [[ ! -f "$CA_PEM" ]]; then
            log_warn "Partial CA files found. Regenerating..."
            rm -f "$CA_KEY" "$CA_PEM"
        else
            log_success "CA certificate already exists."
            return 0
        fi
    fi
    
    echo "=== Generating new CA certificate ==="
    mkdir -p "$CA_DIR"
    
    # Generate CA private key (4096-bit RSA)
    openssl genrsa -out "$CA_KEY" 4096
    if [[ $? -ne 0 ]]; then
        log_error "Failed to generate CA private key"
        return 1
    fi
    
    # Generate self-signed CA certificate (10 years validity)
    openssl req -new -x509 -days 3650 -key "$CA_KEY" -out "$CA_PEM" \
        -subj "/C=US/ST=Local/L=Local/O=Polis/OU=Gateway/CN=Polis CA"
    if [[ $? -ne 0 ]]; then
        log_error "Failed to generate CA certificate"
        rm -f "$CA_KEY"
        return 1
    fi
    
    # Set permissions (readable for Docker bind mount)
    chmod 644 "$CA_KEY"
    chmod 644 "$CA_PEM"
    
    log_success "CA certificate generated successfully:"
    echo "  Private key: $CA_KEY"
    echo "  Certificate: $CA_PEM"
    echo ""
    log_warn "Keep ca.key secure and never commit it to version control!"
    return 0
}

# Setup Valkey TLS certificates and secrets (idempotent)
setup_valkey() {
    local VALKEY_CERTS_DIR="${PROJECT_ROOT}/certs/valkey"
    local VALKEY_SECRETS_DIR="${PROJECT_ROOT}/secrets"

    # --- Valkey TLS certificates ---
    if [[ -f "${VALKEY_CERTS_DIR}/ca.crt" ]] && [[ -f "${VALKEY_CERTS_DIR}/ca.key" ]] \
        && [[ -f "${VALKEY_CERTS_DIR}/server.crt" ]] && [[ -f "${VALKEY_CERTS_DIR}/server.key" ]] \
        && [[ -f "${VALKEY_CERTS_DIR}/client.crt" ]] && [[ -f "${VALKEY_CERTS_DIR}/client.key" ]]; then
        log_success "Valkey TLS certificates already exist."
    else
        echo "Generating Valkey TLS certificates..."
        if ! bash "${PROJECT_ROOT}/services/state/scripts/generate-certs.sh" \
            "${VALKEY_CERTS_DIR}"; then
            log_error "Failed to generate Valkey TLS certificates"
            return 1
        fi
        log_success "Valkey TLS certificates generated."
    fi

    # --- Valkey secrets (passwords + ACL) ---
    # Force regeneration if ACL has placeholder passwords (template from repo)
    local needs_regen=false
    if [[ -f "${VALKEY_SECRETS_DIR}/valkey_users.acl" ]]; then
        if grep -q '>password' "${VALKEY_SECRETS_DIR}/valkey_users.acl" 2>/dev/null; then
            log_warn "Valkey ACL contains placeholder passwords. Regenerating..."
            needs_regen=true
        fi
    fi

    if [[ "$needs_regen" == "false" ]] \
        && [[ -f "${VALKEY_SECRETS_DIR}/valkey_password.txt" ]] \
        && [[ -f "${VALKEY_SECRETS_DIR}/valkey_users.acl" ]]; then
        log_success "Valkey secrets already exist."
    else
        echo "Generating Valkey secrets..."
        if ! bash "${PROJECT_ROOT}/services/state/scripts/generate-secrets.sh" \
            "${VALKEY_SECRETS_DIR}" "${PROJECT_ROOT}"; then
            log_error "Failed to generate Valkey secrets"
            return 1
        fi
        log_success "Valkey secrets generated."
    fi

    return 0
}

# =============================================================================
# Agent Plugin System (Manifest-driven v1)
# =============================================================================

# Reserved platform ports — agents must not use these
RESERVED_PORTS="18080 1344 53 8080 6379"

# Check yq dependency
check_yq() {
    if ! command -v yq &>/dev/null; then
        log_error "yq v4+ is required. Install: https://github.com/mikefarah/yq#install"
        exit 1
    fi
    local yq_ver
    yq_ver=$(yq --version 2>/dev/null | grep -oP '\d+' | head -1)
    if [[ "${yq_ver:-0}" -lt 4 ]]; then
        log_error "yq v4+ is required (found v${yq_ver}). Install: https://github.com/mikefarah/yq#install"
        exit 1
    fi
}

# Structured audit log (JSON, one per line)
audit_log() {
    local event="$1" agent="${2:-}" result="${3:-ok}" detail="${4:-}"
    local ts
    ts=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
    printf '{"ts":"%s","event":"%s","agent":"%s","result":"%s","detail":"%s"}\n' \
        "$ts" "$event" "$agent" "$result" "$detail" \
        >> "${PROJECT_ROOT}/.polis-audit.log"
}

# Parse agent.yaml and export variables
load_agent_yaml() {
    local agent="$1"
    local manifest="${PROJECT_ROOT}/agents/${agent}/agent.yaml"
    if [[ ! -f "$manifest" ]]; then
        log_error "Manifest not found: ${manifest}"
        exit 1
    fi
    AGENT_NAME=$(yq '.metadata.name' "$manifest")
    AGENT_DISPLAY_NAME=$(yq '.metadata.displayName' "$manifest")
    AGENT_VERSION=$(yq '.metadata.version' "$manifest")
    AGENT_DESCRIPTION=$(yq '.metadata.description' "$manifest")
    AGENT_INSTALL=$(yq '.spec.install // ""' "$manifest")
    AGENT_COMMAND=$(yq '.spec.runtime.command' "$manifest")
    AGENT_HEALTH_CMD=$(yq '.spec.health.command' "$manifest")
    AGENT_INIT=$(yq '.spec.init // ""' "$manifest")
    AGENT_SERVICE_NAME="$AGENT_NAME"
    audit_log "manifest_parsed" "$agent"
}

# Security validation of manifest fields
validate_manifest_security() {
    local agent="$1"
    local manifest="${PROJECT_ROOT}/agents/${agent}/agent.yaml"
    local errors=()

    # metadata.name format
    local name
    name=$(yq '.metadata.name' "$manifest")
    if [[ ! "$name" =~ ^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$ ]]; then
        errors+=("metadata.name '${name}' is invalid. Must be lowercase alphanumeric with hyphens, 1-63 chars, no leading/trailing hyphen.")
    fi

    # runtime.command must start with absolute path
    local cmd
    cmd=$(yq '.spec.runtime.command' "$manifest")
    if [[ "$cmd" != /* ]]; then
        errors+=("runtime.command must start with an absolute path: ${cmd}")
    fi

    # Shell metacharacter check for runtime.command
    local meta_re='[;|&`$()]'
    if [[ "$cmd" =~ $meta_re ]]; then
        errors+=("runtime.command contains shell metacharacters (injection risk)")
    fi

    # Shell metacharacter check for health.command
    local hcmd
    hcmd=$(yq '.spec.health.command' "$manifest")
    if [[ "$hcmd" =~ $meta_re ]]; then
        errors+=("health.command contains shell metacharacters (injection risk)")
    fi

    # runtime.user must not be root
    local user
    user=$(yq '.spec.runtime.user // "polis"' "$manifest")
    if [[ "$user" == "root" ]]; then
        errors+=("runtime.user 'root' is privileged. Agents must run as unprivileged user (UID >= 1000).")
    fi

    # runtime.envFile must be under /home/polis/
    local envfile
    envfile=$(yq '.spec.runtime.envFile // ""' "$manifest")
    if [[ -n "$envfile" ]] && [[ "$envfile" != /home/polis/* ]]; then
        errors+=("runtime.envFile must be under /home/polis/: ${envfile}")
    fi

    # Path traversal checks for install, init, commands
    local field val
    for field in '.spec.install' '.spec.init' '.spec.commands'; do
        val=$(yq "${field} // \"\"" "$manifest")
        if [[ -n "$val" ]] && [[ "$val" == *..* ]]; then
            errors+=("${field} escapes agent directory (path traversal): ${val}")
        fi
    done

    # readWritePaths must be under allowed prefixes
    local rwcount
    rwcount=$(yq '.spec.security.readWritePaths | length // 0' "$manifest")
    for ((i = 0; i < rwcount; i++)); do
        local rwp
        rwp=$(yq ".spec.security.readWritePaths[${i}]" "$manifest")
        if [[ "$rwp" != /home/polis/* ]] && [[ "$rwp" != /tmp/* ]] && \
           [[ "$rwp" != /var/lib/* ]] && [[ "$rwp" != /var/log/* ]]; then
            errors+=("readWritePaths '${rwp}' is outside allowed prefixes.")
        fi
    done

    # runtime.env key validation
    local envcount
    envcount=$(yq '.spec.runtime.env | length // 0' "$manifest")
    if [[ "$envcount" -gt 0 ]]; then
        while IFS= read -r key; do
            if [[ ! "$key" =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]]; then
                errors+=("runtime.env key '${key}' invalid")
            fi
            local val
            val=$(yq ".spec.runtime.env.\"${key}\"" "$manifest")
            if [[ "$val" == *$'\n'* ]]; then
                errors+=("runtime.env value for '${key}' contains newline (injection risk)")
            fi
        done < <(yq '.spec.runtime.env | keys | .[]' "$manifest" 2>/dev/null)
    fi

    # Port conflict check
    local portcount
    portcount=$(yq '.spec.ports | length // 0' "$manifest")
    for ((i = 0; i < portcount; i++)); do
        local port
        port=$(yq ".spec.ports[${i}].default" "$manifest")
        for rp in $RESERVED_PORTS; do
            if [[ "$port" == "$rp" ]]; then
                errors+=("Port ${port} conflicts with a platform service. Choose a different port.")
            fi
        done
    done

    # memoryMax vs memoryLimit warning
    local memmax memlimit
    memmax=$(yq '.spec.security.memoryMax // ""' "$manifest")
    memlimit=$(yq '.spec.resources.memoryLimit // "4G"' "$manifest")
    if [[ -n "$memmax" ]] && [[ -n "$memlimit" ]]; then
        # Simple numeric comparison (strip unit suffix)
        local max_num="${memmax//[^0-9]/}" lim_num="${memlimit//[^0-9]/}"
        if [[ -n "$max_num" ]] && [[ -n "$lim_num" ]] && [[ "$max_num" -gt "$lim_num" ]]; then
            log_warn "memoryMax (${memmax}) exceeds Docker memoryLimit (${memlimit}). Docker will OOM-kill before systemd limit is reached."
        fi
    fi

    if [[ ${#errors[@]} -gt 0 ]]; then
        log_error "Manifest security validation failed for '${agent}':"
        for e in "${errors[@]}"; do
            echo "  - $e" >&2
        done
        audit_log "security_validated" "$agent" "fail" "${errors[0]}"
        exit 1
    fi
    audit_log "security_validated" "$agent" "ok"
}

# Generate compose.override.yaml from agent.yaml
generate_compose_override() {
    local agent="$1"
    local manifest="${PROJECT_ROOT}/agents/${agent}/agent.yaml"
    local outdir="${PROJECT_ROOT}/agents/${agent}/.generated"
    local outfile="${outdir}/compose.override.yaml"
    mkdir -p "$outdir"

    local name health_cmd health_int health_to health_ret health_sp mem_lim mem_res
    name=$(yq '.metadata.name' "$manifest")
    health_cmd=$(yq '.spec.health.command' "$manifest")
    health_int=$(yq '.spec.health.interval // "30s"' "$manifest")
    health_to=$(yq '.spec.health.timeout // "10s"' "$manifest")
    health_ret=$(yq '.spec.health.retries // 5' "$manifest")
    health_sp=$(yq '.spec.health.startPeriod // "120s"' "$manifest")
    mem_lim=$(yq '.spec.resources.memoryLimit // "4G"' "$manifest")
    mem_res=$(yq '.spec.resources.memoryReservation // "1G"' "$manifest")

    # Start building the file
    cat > "$outfile" << HEADER
# Platform-generated from agents/${name}/agent.yaml
# DO NOT EDIT — regenerated on every polis init
services:
  workspace:
HEADER

    # Ports
    local portcount
    portcount=$(yq '.spec.ports | length // 0' "$manifest")
    if [[ "$portcount" -gt 0 ]]; then
        echo "    ports:" >> "$outfile"
        for ((i = 0; i < portcount; i++)); do
            local cport henv hdef
            cport=$(yq ".spec.ports[${i}].container" "$manifest")
            henv=$(yq ".spec.ports[${i}].hostEnv" "$manifest")
            hdef=$(yq ".spec.ports[${i}].default" "$manifest")
            echo "      - \"\${${henv}:-${hdef}}:${cport}\"" >> "$outfile"
        done
    fi

    # env_file + volumes
    cat >> "$outfile" << VOLS
    env_file:
      - .env
    volumes:
      - ./agents/${name}/:/tmp/agents/${name}/:ro
      - ./agents/${name}/.generated/${name}.service:/etc/systemd/system/${name}.service:ro
      - ./agents/${name}/.generated/${name}.service.sha256:/etc/systemd/system/${name}.service.sha256:ro
      - ./agents/${name}/.generated/${name}.env:/run/${name}-env:ro
VOLS

    # Persistence volumes
    local perscount
    perscount=$(yq '.spec.persistence | length // 0' "$manifest")
    for ((i = 0; i < perscount; i++)); do
        local vname vpath
        vname=$(yq ".spec.persistence[${i}].name" "$manifest")
        vpath=$(yq ".spec.persistence[${i}].containerPath" "$manifest")
        echo "      - polis-agent-${name}-${vname}:${vpath}" >> "$outfile"
    done

    # Healthcheck
    cat >> "$outfile" << HEALTH
    healthcheck:
      test: ["CMD-SHELL", "systemctl is-active polis-init.service && systemctl is-active ${name}.service && ${health_cmd} && ip route | grep -q default"]
      interval: ${health_int}
      timeout: ${health_to}
      retries: ${health_ret}
      start_period: ${health_sp}
HEALTH

    # Deploy resources
    cat >> "$outfile" << DEPLOY
    deploy:
      resources:
        limits:
          memory: ${mem_lim}
        reservations:
          memory: ${mem_res}
DEPLOY

    # Named volumes section
    if [[ "$perscount" -gt 0 ]]; then
        echo "" >> "$outfile"
        echo "volumes:" >> "$outfile"
        for ((i = 0; i < perscount; i++)); do
            local vname
            vname=$(yq ".spec.persistence[${i}].name" "$manifest")
            echo "  polis-agent-${name}-${vname}:" >> "$outfile"
            echo "    name: polis-agent-${name}-${vname}" >> "$outfile"
        done
    fi

    audit_log "compose_generated" "$agent"
}

# Generate <name>.service systemd unit from agent.yaml
generate_systemd_unit() {
    local agent="$1"
    local manifest="${PROJECT_ROOT}/agents/${agent}/agent.yaml"
    local outdir="${PROJECT_ROOT}/agents/${agent}/.generated"
    mkdir -p "$outdir"

    local name disp_name cmd workdir user envfile init_script
    name=$(yq '.metadata.name' "$manifest")
    disp_name=$(yq '.metadata.displayName' "$manifest")
    cmd=$(yq '.spec.runtime.command' "$manifest")
    workdir=$(yq ".spec.runtime.workdir // \"/opt/agents/${name}\"" "$manifest")
    user=$(yq '.spec.runtime.user // "polis"' "$manifest")
    envfile=$(yq '.spec.runtime.envFile // ""' "$manifest")
    init_script=$(yq '.spec.init // ""' "$manifest")

    local protect_sys protect_home no_new_priv priv_tmp mem_max cpu_quota
    protect_sys=$(yq '.spec.security.protectSystem // "strict"' "$manifest")
    protect_home=$(yq '.spec.security.protectHome // "read-only"' "$manifest")
    no_new_priv=$(yq '.spec.security.noNewPrivileges // true' "$manifest")
    priv_tmp=$(yq '.spec.security.privateTmp // true' "$manifest")
    mem_max=$(yq '.spec.security.memoryMax // ""' "$manifest")
    cpu_quota=$(yq '.spec.security.cpuQuota // ""' "$manifest")

    # Collect readWritePaths
    local rwpaths=""
    local rwcount
    rwcount=$(yq '.spec.security.readWritePaths | length // 0' "$manifest")
    for ((i = 0; i < rwcount; i++)); do
        local p
        p=$(yq ".spec.security.readWritePaths[${i}]" "$manifest")
        rwpaths="${rwpaths}${rwpaths:+ }${p}"
    done

    local outfile="${outdir}/${name}.service"
    cat > "$outfile" << UNIT_HEADER
# Platform-generated from agents/${name}/agent.yaml
# DO NOT EDIT — regenerated on every polis init
[Unit]
Description=${disp_name}
After=network-online.target polis-init.service
Wants=network-online.target
Requires=polis-init.service
StartLimitIntervalSec=300
StartLimitBurst=5

[Service]
Type=simple
User=${user}
Group=${user}
WorkingDirectory=-${workdir}
UNIT_HEADER

    # EnvironmentFile
    if [[ -n "$envfile" ]]; then
        echo "EnvironmentFile=-${envfile}" >> "$outfile"
    fi

    # Platform-injected CA trust
    cat >> "$outfile" << CA_ENV

# Platform-injected CA trust
Environment="NODE_EXTRA_CA_CERTS=/etc/ssl/certs/polis-ca.pem"
Environment="SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt"
Environment="REQUESTS_CA_BUNDLE=/etc/ssl/certs/ca-certificates.crt"
CA_ENV

    # User-defined env vars
    local envcount
    envcount=$(yq '.spec.runtime.env | length // 0' "$manifest")
    if [[ "$envcount" -gt 0 ]]; then
        echo "" >> "$outfile"
        echo "# User-defined env vars from manifest" >> "$outfile"
        while IFS= read -r key; do
            local val
            val=$(yq ".spec.runtime.env.\"${key}\"" "$manifest")
            echo "Environment=\"${key}=${val}\"" >> "$outfile"
        done < <(yq '.spec.runtime.env | keys | .[]' "$manifest" 2>/dev/null)
    fi

    # ExecStartPre (init script — runs as root via '+' prefix for directory setup)
    if [[ -n "$init_script" ]]; then
        echo "" >> "$outfile"
        echo "ExecStartPre=+/tmp/agents/${name}/${init_script}" >> "$outfile"
    fi

    # ExecStart + stop/restart
    cat >> "$outfile" << EXEC

ExecStart=${cmd}

ExecStop=/bin/kill -SIGTERM \$MAINPID
TimeoutStopSec=30
Restart=on-failure
RestartSec=10
EXEC

    # Security hardening
    cat >> "$outfile" << SEC

# Security hardening
NoNewPrivileges=${no_new_priv}
ProtectSystem=${protect_sys}
ProtectHome=${protect_home}
SEC

    if [[ -n "$rwpaths" ]]; then
        echo "ReadWritePaths=${rwpaths}" >> "$outfile"
    fi
    echo "PrivateTmp=${priv_tmp}" >> "$outfile"

    # BindReadOnlyPaths — always present (PrivateTmp hides /tmp)
    echo "BindReadOnlyPaths=/tmp/agents/${name}" >> "$outfile"

    if [[ -n "$mem_max" ]]; then
        echo "MemoryMax=${mem_max}" >> "$outfile"
    fi
    if [[ -n "$cpu_quota" ]]; then
        echo "CPUQuota=${cpu_quota}" >> "$outfile"
    fi

    # Logging + Install
    cat >> "$outfile" << TAIL

StandardOutput=journal
StandardError=journal
SyslogIdentifier=${name}

[Install]
WantedBy=multi-user.target
TAIL

    # Generate SHA-256 hash
    sha256sum "$outfile" | cut -d' ' -f1 > "${outdir}/${name}.service.sha256"

    audit_log "systemd_generated" "$agent"
}

# Generate per-agent env file (only declared variables)
generate_agent_env() {
    local agent="$1"
    local manifest="${PROJECT_ROOT}/agents/${agent}/agent.yaml"
    local outdir="${PROJECT_ROOT}/agents/${agent}/.generated"
    local name
    name=$(yq '.metadata.name' "$manifest")
    local outfile="${outdir}/${name}.env"
    mkdir -p "$outdir"

    : > "$outfile"

    local env_file="${PROJECT_ROOT}/.env"
    [[ -f "$env_file" ]] || return 0

    # Collect declared variable names from envOneOf + envOptional
    local vars=()
    while IFS= read -r v; do
        [[ -n "$v" ]] && vars+=("$v")
    done < <(yq '.spec.requirements.envOneOf // [] | .[]' "$manifest" 2>/dev/null)
    while IFS= read -r v; do
        [[ -n "$v" ]] && vars+=("$v")
    done < <(yq '.spec.requirements.envOptional // [] | .[]' "$manifest" 2>/dev/null)

    for var in "${vars[@]}"; do
        local line
        line=$(grep "^${var}=" "$env_file" 2>/dev/null | head -1 || true)
        if [[ -n "$line" ]]; then
            echo "$line" >> "$outfile"
        fi
    done
    chmod 600 "$outfile"
}

discover_agents() {
    local agents_dir="${PROJECT_ROOT}/agents"
    for agent_dir in "$agents_dir"/*/; do
        [[ "$(basename "$agent_dir")" == "_template" ]] && continue
        if [[ -f "$agent_dir/agent.yaml" ]]; then
            basename "$agent_dir"
        fi
    done
}

validate_agent() {
    local agent="$1"
    local agent_dir="${PROJECT_ROOT}/agents/${agent}"
    local manifest="${agent_dir}/agent.yaml"

    if [[ ! -f "$manifest" ]]; then
        log_error "Unknown agent: ${agent}"
        log_info "Available agents:"
        discover_agents | sed 's/^/  - /'
        exit 1
    fi

    check_yq

    # Required fields check
    local missing=()
    for field in '.apiVersion' '.kind' '.metadata.name' '.spec.packaging' '.spec.runtime.command' '.spec.health.command'; do
        local val
        val=$(yq "${field} // \"\"" "$manifest")
        if [[ -z "$val" || "$val" == "null" ]]; then
            missing+=("$field")
        fi
    done
    if [[ ${#missing[@]} -gt 0 ]]; then
        log_error "agent.yaml missing required fields: ${missing[*]}"
        audit_log "validation_failed" "$agent" "fail" "missing: ${missing[*]}"
        exit 1
    fi

    # Packaging check (Phase 1: script only)
    local pkg
    pkg=$(yq '.spec.packaging' "$manifest")
    if [[ "$pkg" != "script" ]]; then
        log_error "Unsupported packaging type: ${pkg}. Only 'script' is supported."
        exit 1
    fi

    # Install script existence
    local install_path
    install_path=$(yq '.spec.install // ""' "$manifest")
    if [[ -n "$install_path" ]] && [[ ! -f "${agent_dir}/${install_path}" ]]; then
        log_error "Install script not found: ${agent_dir}/${install_path}"
        exit 1
    fi

    validate_manifest_security "$agent"
}

build_compose_flags() {
    local agent="$1"
    local flags="-f ${COMPOSE_FILE} --env-file ${ENV_FILE}"

    local override="${PROJECT_ROOT}/agents/${agent}/.generated/compose.override.yaml"
    if [[ -f "$override" ]]; then
        flags="${flags} -f ${override}"
    fi

    echo "$flags"
}

dispatch_agent_command() {
    local agent="$1"
    local subcmd="${2:-}"
    shift 2 || true

    validate_agent "$agent"
    load_agent_yaml "$agent"

    case "$subcmd" in
        init)
            log_info "Waiting for ${AGENT_DISPLAY_NAME} to initialize..."
            local init_retries=30
            while [[ $init_retries -gt 0 ]]; do
                if docker exec polis-workspace systemctl is-active "$AGENT_SERVICE_NAME" &>/dev/null; then
                    break
                fi
                sleep 2
                ((init_retries--))
            done
            
            if [[ $init_retries -eq 0 ]]; then
                log_warn "${AGENT_DISPLAY_NAME} service not active yet."
                echo "Check status with: ./cli/polis.sh ${agent} status"
                exit 1
            fi
            
            # Wait for agent to fully initialize (token available)
            local commands_script="${PROJECT_ROOT}/agents/${agent}/commands.sh"
            if [[ -f "$commands_script" ]]; then
                init_retries=15
                while [[ $init_retries -gt 0 ]]; do
                    if bash "$commands_script" "polis-workspace" token &>/dev/null; then
                        break
                    fi
                    sleep 2
                    ((init_retries--))
                done
                
                if [[ $init_retries -eq 0 ]]; then
                    log_warn "${AGENT_DISPLAY_NAME} initialization taking longer than expected."
                    echo "Check logs with: ./cli/polis.sh ${agent} logs"
                    exit 1
                fi
                
                echo ""
                bash "$commands_script" "polis-workspace" token
                echo ""
                log_info "To connect: open the Control UI, paste the token above, and click Connect."
                log_success "${AGENT_DISPLAY_NAME} is ready."
            else
                log_success "${AGENT_DISPLAY_NAME} is ready."
            fi
            ;;
        status)
            docker exec polis-workspace systemctl status "$AGENT_SERVICE_NAME"
            ;;
        logs)
            docker exec polis-workspace journalctl -u "$AGENT_SERVICE_NAME" -n "${1:-50}" --no-pager
            ;;
        shell)
            docker exec -it -u polis polis-workspace /bin/bash
            ;;
        restart)
            docker exec polis-workspace systemctl restart "$AGENT_SERVICE_NAME"
            log_success "Restarted ${AGENT_DISPLAY_NAME}"
            ;;
        *)
            # Try agent-specific commands.sh
            local cmd_path
            cmd_path=$(yq '.spec.commands // ""' "${PROJECT_ROOT}/agents/${agent}/agent.yaml")
            local commands_script="${PROJECT_ROOT}/agents/${agent}/${cmd_path}"
            if [[ -n "$cmd_path" ]] && [[ -f "$commands_script" ]]; then
                bash "$commands_script" "polis-workspace" "$subcmd" "$@"
            else
                log_error "Unknown command: ${subcmd}"
                echo "Generic commands: init, status, logs, shell, restart"
                exit 1
            fi
            ;;
    esac
}

scaffold_agent() {
    local name="$1"
    local target="${PROJECT_ROOT}/agents/${name}"
    local template="${PROJECT_ROOT}/agents/_template"

    if [[ -d "$target" ]]; then
        log_error "Agent '${name}' already exists at ${target}"
        exit 1
    fi
    if [[ ! -d "$template" ]]; then
        log_error "Template directory not found: ${template}"
        exit 1
    fi

    cp -r "$template" "$target"
    # Replace CHANGEME in agent.yaml and install.sh
    sed -i "s/CHANGEME/${name}/g" "$target/agent.yaml" "$target/install.sh"
    chmod +x "$target/install.sh"
    log_success "Scaffolded agent '${name}' at ${target}"
    log_info "Edit agent.yaml in ${target}/ then run: ./polis init --agent=${name} --local"
}

validate_agent_env() {
    local agent="$1"
    local env_file="${PROJECT_ROOT}/.env"
    local manifest="${PROJECT_ROOT}/agents/${agent}/agent.yaml"

    load_agent_yaml "$agent"

    # Create .env if missing
    if [[ ! -f "$env_file" ]]; then
        touch "$env_file"
        chmod 600 "$env_file"
    fi

    # Check requirements.envOneOf
    local oneofcount
    oneofcount=$(yq '.spec.requirements.envOneOf | length // 0' "$manifest")
    if [[ "$oneofcount" -gt 0 ]]; then
        local found=false
        local varlist=""
        while IFS= read -r var; do
            varlist="${varlist}${varlist:+ }${var}"
            if grep -qE "^${var}=.+" "$env_file" 2>/dev/null; then
                found=true
                break
            fi
        done < <(yq '.spec.requirements.envOneOf | .[]' "$manifest")
        if [[ "$found" == "false" ]]; then
            log_warn "No required API key configured for ${AGENT_DISPLAY_NAME}!"
            echo "Edit ${env_file} and set one of: ${varlist}"
        fi
    fi
}

# Get workspace container name
get_workspace_container() {
    echo "polis-workspace"
}

# Show usage
show_usage() {
    cat << 'EOF'
Polis - Secure AI Workspace with Traffic Inspection

Usage: polis.sh <command> [options]

Quick Start:
  init                  Setup everything and start (recommended first command)

Core Commands:
  init                  Full initialization (checks Docker, installs Sysbox, starts containers)
  up                    Start containers
  down                  Stop and remove containers
  status                Show container status
  logs [service]        Show container logs
  shell                 Enter workspace shell

Agent Commands:
  <agent> <subcmd>      Run agent command (e.g. openclaw token, openclaw status)
    status              Show agent service status
    logs [n]            Show last n lines of agent logs (default: 50)
    shell               Enter workspace shell
    restart             Restart agent service
    help                Show all commands for this agent (includes agent-specific ones)

  agents list           List available agents
  agents info <name>    Show agent metadata
  agent scaffold <name> Create new agent from template

Options:
  --agent=<name>        Agent to use (default: openclaw)
  --local               Build from source instead of pulling images
  --no-cache            Build without Docker cache

Examples:
  ./cli/polis.sh init                           # First-time setup (openclaw)
  ./cli/polis.sh init --agent=openclaw --local  # Build from source
  ./cli/polis.sh openclaw connect               # Pair a device
  ./cli/polis.sh openclaw help                  # Show all openclaw commands
  ./cli/polis.sh openclaw token                 # Get access token
  ./cli/polis.sh openclaw logs 100              # View last 100 log lines
  ./cli/polis.sh agents list                    # List available agents
  ./cli/polis.sh agent scaffold myagent         # Create new agent

EOF
}

# Action Dispatcher
case "${1:-}" in
    init)
        echo ""
        echo "╔═══════════════════════════════════════════════════════════════╗"
        echo "║                    Polis Initialization                       ║"
        echo "╚═══════════════════════════════════════════════════════════════╝"
        echo ""
        
        # Determine agent (base if not specified)
        EFFECTIVE_AGENT="${AGENT_NAME:-base}"
        if [[ "$EFFECTIVE_AGENT" != "base" ]]; then
            validate_agent "$EFFECTIVE_AGENT"
            load_agent_yaml "$EFFECTIVE_AGENT"
            log_info "Using agent: ${AGENT_DISPLAY_NAME} (${EFFECTIVE_AGENT})"
        fi
        
        # Step 0: Environment checks
        log_step "Checking environment..."
        
        
        if ! check_docker_version; then
            exit 1
        fi
        
        # Step 1: Setup Sysbox runtime
        echo ""
        log_step "Setting up Sysbox runtime..."
        if ! setup_sysbox; then
            echo ""
            log_error "Sysbox setup failed. Cannot proceed."
            echo "The workspace container requires Sysbox for secure Docker-in-Docker support."
            exit 1
        fi
        
        # Step 2: Generate CA if needed
        echo ""
        log_step "Setting up CA certificate..."
        if ! generate_ca; then
            log_error "CA generation failed. Cannot proceed."
            exit 1
        fi
        
        # Step 3: Setup Valkey TLS and secrets
        echo ""
        log_step "Setting up Valkey state management..."
        if ! setup_valkey; then
            log_error "Valkey setup failed. Cannot proceed."
            exit 1
        fi
        
        # Step 4: Validate agent environment
        echo ""
        if [[ "$EFFECTIVE_AGENT" != "base" ]]; then
            log_step "Checking agent environment..."
            validate_agent_env "$EFFECTIVE_AGENT"
        fi
        
        # Step 5: Generate agent artifacts from manifest
        if [[ "$EFFECTIVE_AGENT" != "base" ]]; then
            echo ""
            log_step "Generating agent artifacts from manifest..."
            generate_compose_override "$EFFECTIVE_AGENT"
            generate_systemd_unit "$EFFECTIVE_AGENT"
            generate_agent_env "$EFFECTIVE_AGENT"
            # Ensure agent scripts are executable (systemd returns 203/EXEC otherwise)
            find "${PROJECT_ROOT}/agents/${EFFECTIVE_AGENT}" -name '*.sh' -exec chmod +x {} \;
            log_success "Generated artifacts in agents/${EFFECTIVE_AGENT}/.generated/"
        fi
        
        # Step 6: Clean up existing containers
        echo ""
        log_step "Cleaning up existing containers..."
        docker compose -f "$COMPOSE_FILE" --env-file "$ENV_FILE" down --volumes --remove-orphans 2>/dev/null || true
        docker network prune -f 2>/dev/null || true
        log_success "Environment cleaned."
        
        # Step 7: Build or pull images
        COMPOSE_FLAGS=$(build_compose_flags "$EFFECTIVE_AGENT")
        
        if [[ "$LOCAL_BUILD" == "true" ]]; then
            echo ""
            log_step "Building containers from source..."
            
            # Build base workspace image
            docker build $NO_CACHE \
                -f "${PROJECT_ROOT}/services/workspace/Dockerfile" \
                -t "polis-workspace-oss:latest" \
                "${PROJECT_ROOT}"
            
            # Build remaining services
            docker compose -f "$COMPOSE_FILE" --env-file "$ENV_FILE" build $NO_CACHE
        else
            echo ""
            echo "=== Checking images at GitHub Container Registry ==="
            REGISTRY="${POLIS_REGISTRY:-ghcr.io/odralabshq}"
            
            IMAGES="polis-gate-oss:latest polis-sentinel-oss:latest polis-workspace-oss:latest"
            
            missing_images=false
            for img in $IMAGES; do
                if ! docker manifest inspect "${REGISTRY}/${img}" >/dev/null 2>&1; then
                    log_warn "Image ${REGISTRY}/${img} not found in registry."
                    missing_images=true
                fi
            done
            
            if [[ "$missing_images" == "true" ]]; then
                log_info "Some images not in registry. Building from source..."
                LOCAL_BUILD="true"
                docker build $NO_CACHE \
                    -f "${PROJECT_ROOT}/services/workspace/Dockerfile" \
                    -t "polis-workspace-oss:latest" \
                    "${PROJECT_ROOT}"
                docker compose -f "$COMPOSE_FILE" --env-file "$ENV_FILE" build $NO_CACHE
            else
                echo ""
                echo "=== Pulling images from registry ==="
                for img in $IMAGES; do
                    log_info "Pulling ${REGISTRY}/${img}..."
                    docker pull "${REGISTRY}/${img}"
                    docker tag "${REGISTRY}/${img}" "${img}"
                done
            fi
        fi
        
        echo ""
        echo "=== Starting containers ==="
        # shellcheck disable=SC2086
        docker compose $COMPOSE_FLAGS up -d
        
        log_info "Waiting for services to initialize..."
        sleep 5
        
        # Temporarily relax DLP security level during bootstrap so agent
        # install scripts can download packages (apt, npm, bun) without
        # being blocked by the HITL prompt for new domains. The TTL
        # auto-expires after 5 minutes, restoring balanced mode.
        if [[ "$EFFECTIVE_AGENT" != "base" ]]; then
            log_info "Setting DLP to relaxed mode for bootstrap (5 min TTL)..."
            docker exec polis-state sh -c "
                REDISCLI_AUTH=\$(cat /run/secrets/valkey_mcp_admin_password) \
                valkey-cli --tls --cert /etc/valkey/tls/client.crt \
                    --key /etc/valkey/tls/client.key --cacert /etc/valkey/tls/ca.crt \
                    --user mcp-admin --no-auth-warning \
                    SET polis:config:security_level relaxed EX 300" 2>/dev/null || \
                log_warn "Could not set DLP to relaxed mode — agent install may fail"
        fi
        
        # shellcheck disable=SC2086
        docker compose $COMPOSE_FLAGS ps
        
        # Show agent info and offer device pairing if agent has token command
        if [[ "$EFFECTIVE_AGENT" != "base" ]]; then
            echo ""
            log_success "${AGENT_DISPLAY_NAME} workspace started!"
            echo ""
            
            # Wait for polis-init.service to finish (install.sh + service enablement)
            log_info "Waiting for workspace initialization to complete (this may take several minutes)..."
            init_timeout=600  # 10 minutes max
            elapsed=0
            while [[ $elapsed -lt $init_timeout ]]; do
                init_state=$(docker exec polis-workspace systemctl show polis-init.service --property=ActiveState --value 2>/dev/null || echo "unknown")
                init_sub=$(docker exec polis-workspace systemctl show polis-init.service --property=SubState --value 2>/dev/null || echo "unknown")
                init_result=$(docker exec polis-workspace systemctl show polis-init.service --property=Result --value 2>/dev/null || echo "unknown")
                
                # oneshot+RemainAfterExit services go to active/exited on success
                if [[ "$init_state" == "active" && "$init_sub" == "exited" && "$init_result" == "success" ]]; then
                    log_success "Workspace initialization complete."
                    break
                elif [[ "$init_state" == "inactive" && "$init_result" == "success" ]]; then
                    log_success "Workspace initialization complete."
                    break
                elif [[ "$init_state" == "failed" || "$init_result" == "failed" ]]; then
                    log_error "Workspace initialization failed (result: ${init_result})."
                    echo "Check logs with: docker exec polis-workspace journalctl -u polis-init.service --no-pager"
                    exit 1
                fi
                
                # Show progress every 30 seconds
                if (( elapsed % 30 == 0 && elapsed > 0 )); then
                    log_info "Still initializing... (${elapsed}s elapsed)"
                fi
                
                sleep 5
                ((elapsed += 5))
            done
            
            if [[ $elapsed -ge $init_timeout ]]; then
                log_error "Workspace initialization timed out after ${init_timeout}s."
                echo "Check logs with: docker exec polis-workspace journalctl -u polis-init.service --no-pager"
                exit 1
            fi
            
            # Wait for agent service to become active
            log_info "Waiting for ${AGENT_DISPLAY_NAME} to start..."
            svc_retries=30
            while [[ $svc_retries -gt 0 ]]; do
                if docker exec polis-workspace systemctl is-active "${EFFECTIVE_AGENT}" &>/dev/null; then
                    break
                fi
                sleep 2
                ((svc_retries--))
            done
            
            if [[ $svc_retries -eq 0 ]]; then
                log_error "${AGENT_DISPLAY_NAME} service failed to start."
                echo "Check logs with: docker exec polis-workspace journalctl -u ${EFFECTIVE_AGENT}.service --no-pager"
                exit 1
            fi
            
            # Wait for agent to fully initialize (token available)
            commands_script="${PROJECT_ROOT}/agents/${EFFECTIVE_AGENT}/commands.sh"
            if [[ -f "$commands_script" ]]; then
                token_retries=15
                while [[ $token_retries -gt 0 ]]; do
                    if bash "$commands_script" "polis-workspace" token &>/dev/null; then
                        break
                    fi
                    sleep 2
                    ((token_retries--))
                done
                
                if [[ $token_retries -eq 0 ]]; then
                    log_warn "${AGENT_DISPLAY_NAME} initialization taking longer than expected."
                    echo "Check logs with: docker exec polis-workspace journalctl -u ${EFFECTIVE_AGENT}.service --no-pager"
                    exit 1
                fi
                
                echo ""
                echo "════════════════════════════════════════════════════════════════"
                echo ""
                bash "$commands_script" "polis-workspace" token
                echo ""
                log_info "To connect: open the Control UI, paste the token above, and click Connect."
                echo ""
                echo "════════════════════════════════════════════════════════════════"
            fi
            
            log_success "${AGENT_DISPLAY_NAME} is ready."
        fi
        ;;
        
    build)
        echo "=== Polis: Building images ==="
        EFFECTIVE_AGENT="${AGENT_NAME:-base}"
        COMPOSE_FLAGS=$(build_compose_flags "$EFFECTIVE_AGENT")
        
        # Find first non-flag argument after 'build'
        SERVICE=""
        for arg in "${@:2}"; do
            if [[ "$arg" != --* ]]; then
                SERVICE="$arg"
                break
            fi
        done

        if [[ -n "$SERVICE" ]]; then
            log_info "Building service: $SERVICE"
            # shellcheck disable=SC2086
            docker compose $COMPOSE_FLAGS build $NO_CACHE "$SERVICE"
        else
            # shellcheck disable=SC2086
            docker compose $COMPOSE_FLAGS build $NO_CACHE
        fi
        ;;
        
    up)
        echo "=== Polis: Starting Containers ==="
        EFFECTIVE_AGENT="${AGENT_NAME:-base}"
        # Ensure .env exists
        touch "${PROJECT_ROOT}/.env"
        COMPOSE_FLAGS=$(build_compose_flags "$EFFECTIVE_AGENT")
        # shellcheck disable=SC2086
        docker compose $COMPOSE_FLAGS up -d
        # shellcheck disable=SC2086
        docker compose $COMPOSE_FLAGS ps
        ;;
        
    down)
        echo "=== Polis: Removing Containers ==="
        docker compose -f "$COMPOSE_FILE" --env-file "$ENV_FILE" down --volumes --remove-orphans
        
        echo "=== Polis: Cleaning Secrets and Certificates ==="
        rm -rf "${PROJECT_ROOT}/secrets/"*.txt
        rm -rf "${PROJECT_ROOT}/secrets/"*.acl
        rm -rf "${PROJECT_ROOT}/certs/"*.pem
        rm -rf "${PROJECT_ROOT}/certs/"*.crt
        rm -rf "${PROJECT_ROOT}/certs/"*.key
        rm -rf "${PROJECT_ROOT}/certs/"*.srl
        
        log_success "Containers, networks, volumes, secrets, and certificates removed."
        log_info "Run 'polis init' to regenerate secrets and certificates."
        ;;
        
    stop)
        echo "=== Polis: Stopping Containers ==="
        EFFECTIVE_AGENT="${AGENT_NAME:-base}"
        COMPOSE_FLAGS=$(build_compose_flags "$EFFECTIVE_AGENT")
        # shellcheck disable=SC2086
        docker compose $COMPOSE_FLAGS stop
        ;;
        
    start)
        echo "=== Polis: Starting Existing Containers ==="
        EFFECTIVE_AGENT="${AGENT_NAME:-base}"
        COMPOSE_FLAGS=$(build_compose_flags "$EFFECTIVE_AGENT")
        # shellcheck disable=SC2086
        docker compose $COMPOSE_FLAGS start
        # shellcheck disable=SC2086
        docker compose $COMPOSE_FLAGS ps
        ;;
        
    status)
        echo "=== Polis: Container Status ==="
        docker compose -f "$COMPOSE_FILE" --env-file "$ENV_FILE" ps
        ;;
        
    logs)
        SERVICE="${2:-}"
        if [[ -n "$SERVICE" ]]; then
            docker compose -f "$COMPOSE_FILE" --env-file "$ENV_FILE" logs --tail=50 -f "$SERVICE"
        else
            docker compose -f "$COMPOSE_FILE" --env-file "$ENV_FILE" logs --tail=50 -f
        fi
        ;;
        
    shell)
        echo "=== Polis: Entering Workspace Shell ==="
        docker exec -it -u polis polis-workspace /bin/bash
        ;;
        
    test)
        echo "=== Polis: Running Tests ==="
        shift  # Remove 'test' from args
        if [[ -x "${PROJECT_ROOT}/tests/run-tests.sh" ]]; then
            exec "${PROJECT_ROOT}/tests/run-tests.sh" "$@"
        else
            log_error "Test runner not found at: ${PROJECT_ROOT}/tests/run-tests.sh"
            exit 1
        fi
        ;;
        
    setup-ca)
        echo "=== Polis: CA Certificate Setup ==="
        if [[ "${2:-}" == "--force" ]]; then
            log_info "Removing existing CA..."
            rm -f "${PROJECT_ROOT}/certs/ca/ca.key" "${PROJECT_ROOT}/certs/ca/ca.pem"
        fi
        generate_ca
        ;;
        
    setup-sysbox)
        echo "=== Polis: Sysbox Runtime Setup ==="
        if [[ "${2:-}" == "--force" ]]; then
            log_info "Force reinstalling Sysbox..."
            if dpkg -l | grep -q sysbox; then
                sudo apt-get remove -y sysbox-ce 2>/dev/null || true
            fi
        fi
        setup_sysbox
        ;;
        
    setup-env)
        EFFECTIVE_AGENT="${AGENT_NAME:-openclaw}"
        validate_agent_env "$EFFECTIVE_AGENT"
        ;;
        
    # Agent management commands
    agents)
        case "${2:-}" in
            list)
                echo "=== Available Agents ==="
                for agent in $(discover_agents); do
                    load_agent_yaml "$agent"
                    echo "  ${AGENT_NAME}: ${AGENT_DESCRIPTION}"
                done
                ;;
            info)
                agent="${3:?Usage: polis agents info <name>}"
                validate_agent "$agent"
                load_agent_yaml "$agent"
                manifest="${PROJECT_ROOT}/agents/${agent}/agent.yaml"
                mem_lim=$(yq '.spec.resources.memoryLimit // "4G"' "$manifest")
                mem_res=$(yq '.spec.resources.memoryReservation // "1G"' "$manifest")
                echo "=== Agent: ${AGENT_DISPLAY_NAME} ==="
                echo "  Name:        ${AGENT_NAME}"
                echo "  Version:     ${AGENT_VERSION}"
                echo "  Description: ${AGENT_DESCRIPTION}"
                echo "  Service:     ${AGENT_SERVICE_NAME}"
                echo "  Command:     ${AGENT_COMMAND}"
                echo "  Memory:      ${mem_lim} (reserved: ${mem_res})"
                ;;
            *)
                echo "Usage: polis agents <list|info <name>>"
                exit 1
                ;;
        esac
        ;;
        
    agent)
        case "${2:-}" in
            scaffold)
                name="${3:?Usage: polis agent scaffold <name>}"
                scaffold_agent "$name"
                ;;
            *)
                echo "Usage: polis agent scaffold <name>"
                exit 1
                ;;
        esac
        ;;
        
    help|--help|-h)
        show_usage
        ;;
        
    *)
        # Dynamic agent dispatch: if $1 matches a discovered agent, route to dispatch
        cmd="${1:-}"
        if [[ -n "$cmd" ]] && [[ -f "${PROJECT_ROOT}/agents/${cmd}/agent.yaml" ]]; then
            shift
            dispatch_agent_command "$cmd" "$@"
        else
            show_usage
            exit 1
        fi
        ;;
esac
