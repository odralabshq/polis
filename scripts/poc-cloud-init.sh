#!/usr/bin/env bash
# =============================================================================
# POC: Cloud-Init User Journey Simulation
# =============================================================================
# Simulates the FULL user journey that `polis start` will perform after the
# cloud-init migration. Run this from the repo root.
#
# What it does:
#   Phase 1 — Launch VM with cloud-init (Docker, Sysbox, hardening)
#   Phase 2 — Bundle config on host → transfer into VM → generate certs
#             → pull Docker images → start services → health check
#
# Usage:
#   bash scripts/poc-cloud-init.sh
#
# Environment:
#   POLIS_IMAGE_VERSION  — image tag for docker compose (default: latest)
#   VM_NAME              — multipass VM name (default: polis-test)
#   VM_CPUS              — CPU count (default: 2)
#   VM_MEMORY            — memory (default: 8G)
#   VM_DISK              — disk size (default: 20G)
# =============================================================================
set -euo pipefail

# ── Configuration ────────────────────────────────────────────────────────────
POLIS_IMAGE_VERSION="${POLIS_IMAGE_VERSION:-latest}"
VM_NAME="${VM_NAME:-polis-test}"
VM_CPUS="${VM_CPUS:-2}"
VM_MEMORY="${VM_MEMORY:-8G}"
VM_DISK="${VM_DISK:-20G}"
VM_POLIS_ROOT="/opt/polis"
CLOUD_INIT="cloud-init.yaml"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

log_info()  { echo -e "${BLUE}[INFO]${NC}  $*"; }
log_ok()    { echo -e "${GREEN}[OK]${NC}    $*"; }
log_warn()  { echo -e "${YELLOW}[WARN]${NC}  $*"; }
log_error() { echo -e "${RED}[ERROR]${NC} $*" >&2; }
log_step()  { echo -e "\n${BLUE}══════════════════════════════════════════════════${NC}"; echo -e "${BLUE}  $*${NC}"; echo -e "${BLUE}══════════════════════════════════════════════════${NC}"; }


# ── Preflight checks ────────────────────────────────────────────────────────
preflight() {
    log_step "Preflight checks"

    if ! command -v multipass &>/dev/null; then
        log_error "multipass not found. Install: https://multipass.run/install"
        exit 1
    fi
    log_ok "multipass found"

    if [[ ! -f "${CLOUD_INIT}" ]]; then
        log_error "cloud-init.yaml not found. Run from repo root."
        exit 1
    fi
    log_ok "cloud-init.yaml found"

    if [[ ! -f "docker-compose.yml" ]]; then
        log_error "docker-compose.yml not found. Run from repo root."
        exit 1
    fi
    log_ok "docker-compose.yml found"
}

# ── Cleanup existing VM ─────────────────────────────────────────────────────
cleanup_existing() {
    if multipass info "${VM_NAME}" &>/dev/null 2>&1; then
        log_warn "Existing VM '${VM_NAME}' found — deleting..."
        multipass delete "${VM_NAME}" 2>/dev/null || true
        multipass purge 2>/dev/null || true
        log_ok "Old VM removed"
    fi
}

# ── Phase 1: Launch VM with cloud-init ───────────────────────────────────────
phase1_launch() {
    log_step "Phase 1: Launch VM with cloud-init"
    log_info "Launching ${VM_NAME} (${VM_CPUS} CPUs, ${VM_MEMORY} RAM, ${VM_DISK} disk)..."
    log_info "This installs Docker, Sysbox, and applies hardening. Takes 3-5 minutes."

    multipass launch 24.04 \
        --name "${VM_NAME}" \
        --cpus "${VM_CPUS}" \
        --memory "${VM_MEMORY}" \
        --disk "${VM_DISK}" \
        --cloud-init "${CLOUD_INIT}" \
        --timeout 900

    log_ok "VM launched"

    # Wait for cloud-init to fully complete
    log_info "Waiting for cloud-init to finish..."
    multipass exec "${VM_NAME}" -- cloud-init status --wait 2>/dev/null || true
    log_ok "Cloud-init complete"
}

# ── Phase 2: Bundle and transfer config ──────────────────────────────────────
phase2_bundle_and_transfer() {
    log_step "Phase 2: Bundle config and transfer into VM"

    local bundle_dir
    bundle_dir=$(mktemp -d --tmpdir="${HOME}")
    trap "rm -rf '${bundle_dir}'" RETURN

    log_info "Bundling polis config..."

    # docker-compose.yml (strip @sha256 digests)
    sed 's/@sha256:[a-f0-9]\{64\}//g' docker-compose.yml > "${bundle_dir}/docker-compose.yml"

    # .env with pinned versions
    cat > "${bundle_dir}/.env" << EOF
POLIS_RESOLVER_VERSION=${POLIS_IMAGE_VERSION}
POLIS_CERTGEN_VERSION=${POLIS_IMAGE_VERSION}
POLIS_GATE_VERSION=${POLIS_IMAGE_VERSION}
POLIS_SENTINEL_VERSION=${POLIS_IMAGE_VERSION}
POLIS_SCANNER_VERSION=${POLIS_IMAGE_VERSION}
POLIS_WORKSPACE_VERSION=${POLIS_IMAGE_VERSION}
POLIS_HOST_INIT_VERSION=${POLIS_IMAGE_VERSION}
POLIS_STATE_VERSION=${POLIS_IMAGE_VERSION}
POLIS_TOOLBOX_VERSION=${POLIS_IMAGE_VERSION}
EOF

    # Service configs and scripts
    for svc in resolver certgen gate sentinel scanner state toolbox workspace host-init; do
        if [[ -d "services/${svc}/config" ]]; then
            mkdir -p "${bundle_dir}/services/${svc}"
            cp -r "services/${svc}/config" "${bundle_dir}/services/${svc}/"
        fi
        if [[ -d "services/${svc}/scripts" ]]; then
            mkdir -p "${bundle_dir}/services/${svc}"
            cp -r "services/${svc}/scripts" "${bundle_dir}/services/${svc}/"
        fi
    done

    # Setup scripts
    mkdir -p "${bundle_dir}/scripts"
    cp packer/scripts/setup-certs.sh "${bundle_dir}/scripts/"
    cp scripts/generate-agent.sh "${bundle_dir}/scripts/"
    chmod 755 "${bundle_dir}/scripts/"*.sh

    # Polis config
    mkdir -p "${bundle_dir}/config"
    cp config/polis.yaml "${bundle_dir}/config/"

    # Placeholder directories for certs and secrets
    mkdir -p "${bundle_dir}/certs/ca" "${bundle_dir}/certs/valkey" "${bundle_dir}/certs/toolbox"
    mkdir -p "${bundle_dir}/secrets"

    # Create config tarball
    local config_tar="${bundle_dir}/polis-config.tar.gz"
    tar -czf "${config_tar}" -C "${bundle_dir}" \
        docker-compose.yml .env services scripts config certs secrets

    log_ok "Config bundle created ($(du -h "${config_tar}" | cut -f1))"

    # Bundle agents
    local agents_dir="${bundle_dir}/agents-staging"
    mkdir -p "${agents_dir}"
    local has_agents=false
    for agent_dir in agents/*/; do
        [[ -d "${agent_dir}" ]] || continue
        local name
        name=$(basename "${agent_dir}")
        [[ "${name}" == "_template" ]] && continue
        [[ -f "${agent_dir}/agent.yaml" ]] || continue
        cp -r "${agent_dir}" "${agents_dir}/${name}"
        has_agents=true
    done

    local agents_tar="${bundle_dir}/polis-agents.tar.gz"
    if [[ "${has_agents}" == true ]]; then
        # Generate agent artifacts on host (requires yq)
        if command -v yq &>/dev/null; then
            for agent_dir in "${agents_dir}"/*/; do
                [[ -d "${agent_dir}" ]] || continue
                local name
                name=$(basename "${agent_dir}")
                ./scripts/generate-agent.sh "${name}" "${agents_dir}"
            done
        else
            log_warn "yq not found on host — agent artifacts will be generated inside VM"
        fi
        tar -czf "${agents_tar}" -C "${agents_dir}" .
        log_ok "Agents bundle created ($(du -h "${agents_tar}" | cut -f1))"
    fi

    # Transfer into VM
    log_info "Transferring config bundle into VM..."
    multipass transfer "${config_tar}" "${VM_NAME}:/tmp/polis-config.tar.gz"
    log_ok "Config transferred"

    if [[ "${has_agents}" == true ]]; then
        log_info "Transferring agents bundle into VM..."
        multipass transfer "${agents_tar}" "${VM_NAME}:/tmp/polis-agents.tar.gz"
        log_ok "Agents transferred"
    fi

    # Extract inside VM
    log_info "Extracting config inside VM..."
    multipass exec "${VM_NAME}" -- bash -c "
        cd ${VM_POLIS_ROOT}
        sudo tar -xzf /tmp/polis-config.tar.gz
        sudo chown -R ubuntu:ubuntu ${VM_POLIS_ROOT}
        find ${VM_POLIS_ROOT} -name '*.sh' -exec chmod +x {} \;
        rm -f /tmp/polis-config.tar.gz
    "
    log_ok "Config extracted to ${VM_POLIS_ROOT}"

    if [[ "${has_agents}" == true ]]; then
        log_info "Extracting agents inside VM..."
        multipass exec "${VM_NAME}" -- bash -c "
            mkdir -p ${VM_POLIS_ROOT}/agents
            cd ${VM_POLIS_ROOT}/agents
            sudo tar -xzf /tmp/polis-agents.tar.gz
            sudo chown -R ubuntu:ubuntu ${VM_POLIS_ROOT}/agents
            rm -f /tmp/polis-agents.tar.gz
        "
        log_ok "Agents extracted"
    fi
}


# ── Phase 3: Generate certs inside VM ────────────────────────────────────────
phase3_generate_certs() {
    log_step "Phase 3: Generate certificates"
    log_info "Running setup-certs.sh inside VM..."

    multipass exec "${VM_NAME}" -- bash -c "
        cd ${VM_POLIS_ROOT}
        chmod +x scripts/setup-certs.sh
        chmod +x services/state/scripts/*.sh 2>/dev/null || true
        chmod +x services/toolbox/scripts/*.sh 2>/dev/null || true
        sudo bash scripts/setup-certs.sh
    "
    log_ok "Certificates generated"
}

# ── Phase 4: Load Docker images ──────────────────────────────────────────────
phase4_load_images() {
    log_step "Phase 4: Load Docker images"

    local images_tar=".build/polis-images.tar"
    if [[ ! -f "${images_tar}" ]]; then
        log_error "${images_tar} not found. Run 'just build' first to build and export Docker images."
        exit 1
    fi

    local tar_size
    tar_size=$(du -h "${images_tar}" | cut -f1)
    log_info "Transferring Docker images into VM (${tar_size})... this may take a minute."
    multipass transfer "${images_tar}" "${VM_NAME}:/tmp/polis-images.tar"
    log_ok "Images transferred"

    log_info "Loading images into Docker..."
    multipass exec "${VM_NAME}" -- bash -c "sudo docker load -i /tmp/polis-images.tar && rm -f /tmp/polis-images.tar"
    log_ok "Docker images loaded"

    log_info "Available images:"
    multipass exec "${VM_NAME}" -- bash -c "docker images --format 'table {{.Repository}}\t{{.Tag}}\t{{.Size}}'" || true
}

# ── Phase 5: Start services ─────────────────────────────────────────────────
phase5_start_services() {
    log_step "Phase 5: Start services"
    log_info "Restarting Docker cleanly (stop sysbox + wipe netns state)..."
    multipass exec "${VM_NAME}" -- bash -c "sudo systemctl stop docker.socket docker 2>/dev/null; sudo systemctl stop sysbox sysbox-mgr sysbox-fs 2>/dev/null; sudo rm -f /var/run/docker/netns/*; sudo systemctl start sysbox-mgr sysbox-fs sysbox 2>/dev/null || true; sudo systemctl reset-failed docker 2>/dev/null; sudo systemctl start docker && sleep 5"
    log_info "Running docker compose up -d..."

    # Build compose args — include agent overlay if agents exist
    local compose_args="-f ${VM_POLIS_ROOT}/docker-compose.yml"
    local agent_overlay
    agent_overlay=$(multipass exec "${VM_NAME}" -- bash -c "
        for f in ${VM_POLIS_ROOT}/agents/*/.generated/compose.agent.yaml; do
            [ -f \"\$f\" ] && echo \"\$f\"
        done
    " 2>/dev/null || true)

    if [[ -n "${agent_overlay}" ]]; then
        while IFS= read -r overlay; do
            compose_args="${compose_args} -f ${overlay}"
        done <<< "${agent_overlay}"
        log_info "Including agent overlay(s)"
    fi

    multipass exec "${VM_NAME}" -- bash -c "
        cd ${VM_POLIS_ROOT}
        docker compose ${compose_args} up -d --remove-orphans 2>&1
    "
    log_ok "Services started"
}

# ── Phase 6: Health check ───────────────────────────────────────────────────
phase6_health_check() {
    log_step "Phase 6: Health check"
    log_info "Waiting for services to become healthy (up to 3 minutes)..."

    local max_attempts=18
    local attempt=0
    local all_healthy=false

    while [[ ${attempt} -lt ${max_attempts} ]]; do
        attempt=$((attempt + 1))
        local status
        status=$(multipass exec "${VM_NAME}" -- bash -c "
            cd ${VM_POLIS_ROOT}
            docker compose ps --format '{{.Name}} {{.Status}}' 2>/dev/null
        " || true)

        if [[ -z "${status}" ]]; then
            log_info "Attempt ${attempt}/${max_attempts}: waiting for containers..."
            sleep 10
            continue
        fi

        local unhealthy=0
        local total=0
        while IFS= read -r line; do
            [[ -z "${line}" ]] && continue
            total=$((total + 1))
            if ! echo "${line}" | grep -qiE '(healthy|running)'; then
                unhealthy=$((unhealthy + 1))
            fi
        done <<< "${status}"

        if [[ ${unhealthy} -eq 0 && ${total} -gt 0 ]]; then
            all_healthy=true
            break
        fi

        log_info "Attempt ${attempt}/${max_attempts}: ${total} containers, ${unhealthy} not ready"
        sleep 10
    done

    echo ""
    log_info "Container status:"
    multipass exec "${VM_NAME}" -- bash -c "cd ${VM_POLIS_ROOT} && docker compose ps" || true
    echo ""

    if [[ "${all_healthy}" == true ]]; then
        log_ok "All services healthy"
    else
        log_warn "Some services may still be starting. Check with:"
        echo "  multipass exec ${VM_NAME} -- bash -c 'cd ${VM_POLIS_ROOT} && docker compose ps'"
        echo "  multipass exec ${VM_NAME} -- bash -c 'cd ${VM_POLIS_ROOT} && docker compose logs --tail=20'"
    fi
}

# ── Verification summary ────────────────────────────────────────────────────
verify() {
    log_step "Verification"

    log_info "Docker version:"
    multipass exec "${VM_NAME}" -- docker --version

    log_info "Docker Compose version:"
    multipass exec "${VM_NAME}" -- docker compose version

    log_info "Sysbox runtime:"
    multipass exec "${VM_NAME}" -- bash -c "docker info 2>/dev/null | grep -A2 Runtimes" || true

    log_info "Hardening — kernel.dmesg_restrict:"
    multipass exec "${VM_NAME}" -- sysctl kernel.dmesg_restrict

    log_info "Auditd status:"
    multipass exec "${VM_NAME}" -- systemctl is-active auditd || true

    log_info "Polis directory:"
    multipass exec "${VM_NAME}" -- ls -la "${VM_POLIS_ROOT}/"

    log_info "Certificates:"
    multipass exec "${VM_NAME}" -- bash -c "ls -la ${VM_POLIS_ROOT}/certs/ca/ ${VM_POLIS_ROOT}/certs/valkey/ ${VM_POLIS_ROOT}/certs/toolbox/ 2>/dev/null" || true

    log_info "Secrets:"
    multipass exec "${VM_NAME}" -- bash -c "ls ${VM_POLIS_ROOT}/secrets/ 2>/dev/null" || true
}

# ── Main ─────────────────────────────────────────────────────────────────────
main() {
    echo ""
    echo "╔══════════════════════════════════════════════════════════════╗"
    echo "║     Polis Cloud-Init POC — Full User Journey Simulation    ║"
    echo "╚══════════════════════════════════════════════════════════════╝"
    echo ""

    local start_time
    start_time=$(date +%s)

    preflight
    cleanup_existing
    phase1_launch
    phase2_bundle_and_transfer
    phase3_generate_certs
    phase4_load_images
    phase5_start_services
    phase6_health_check
    verify

    local end_time elapsed
    end_time=$(date +%s)
    elapsed=$((end_time - start_time))
    local minutes=$((elapsed / 60))
    local seconds=$((elapsed % 60))

    echo ""
    echo "╔══════════════════════════════════════════════════════════════╗"
    echo "║                    POC Complete                             ║"
    echo "╠══════════════════════════════════════════════════════════════╣"
    echo "║  Total time: ${minutes}m ${seconds}s"
    echo "║  VM name:    ${VM_NAME}"
    echo "║  Connect:    multipass shell ${VM_NAME}"
    echo "║  Cleanup:    multipass delete ${VM_NAME} && multipass purge"
    echo "╚══════════════════════════════════════════════════════════════╝"
    echo ""
}

main "$@"
