#!/bin/bash
# harden-vm.sh - Apply CIS-aligned VM hardening
# Addresses: V8 (VM hardening - sysctl, AppArmor, auditd, Docker daemon)

set -euo pipefail

echo "==> Applying VM hardening..."

# ============================================================================
# Sysctl hardening (CIS Ubuntu 24.04 Level 1)
# NOTE: net.ipv4.ip_forward=1 is REQUIRED for Docker - do NOT disable
# ============================================================================

cat <<'EOF' | sudo tee /etc/sysctl.d/99-polis-hardening.conf
# Polis VM Hardening - CIS Ubuntu 24.04 Level 1
kernel.randomize_va_space = 2
kernel.dmesg_restrict = 1
kernel.kptr_restrict = 2
fs.suid_dumpable = 0
kernel.yama.ptrace_scope = 2
# NOTE: net.ipv4.ip_forward=1 required for Docker networking
EOF

sudo sysctl --system

# ============================================================================
# AppArmor
# ============================================================================

echo "==> Enabling AppArmor..."
sudo systemctl enable apparmor
sudo systemctl start apparmor || true

# ============================================================================
# Auditd with Docker rules
# ============================================================================

echo "==> Installing and configuring auditd..."
sudo apt-get update
sudo apt-get install -y auditd

cat <<'EOF' | sudo tee /etc/audit/rules.d/docker.rules
# Docker daemon audit rules
-w /usr/bin/docker -p rwxa -k docker
-w /var/lib/docker -p rwxa -k docker
-w /etc/docker -p rwxa -k docker
-w /usr/lib/systemd/system/docker.service -p rwxa -k docker
-w /etc/default/docker -p rwxa -k docker
-w /etc/docker/daemon.json -p rwxa -k docker
-w /usr/bin/containerd -p rwxa -k docker
EOF

sudo systemctl enable auditd
sudo systemctl restart auditd || true

# ============================================================================
# Docker daemon hardening
# ============================================================================

echo "==> Hardening Docker daemon..."

DAEMON_JSON="/etc/docker/daemon.json"
HARDENING_CONFIG='{
  "runtimes": {
    "sysbox-runc": { "path": "/usr/bin/sysbox-runc" }
  },
  "no-new-privileges": true,
  "live-restore": true,
  "userland-proxy": false
}'

# Merge with existing config or create new
if [[ -f "${DAEMON_JSON}" ]]; then
    echo "${HARDENING_CONFIG}" | sudo jq -s '.[0] * .[1]' "${DAEMON_JSON}" - \
        | sudo tee "${DAEMON_JSON}.new" > /dev/null
    sudo mv "${DAEMON_JSON}.new" "${DAEMON_JSON}"
else
    echo "${HARDENING_CONFIG}" | sudo tee "${DAEMON_JSON}" > /dev/null
fi

# Restart Docker and wait for readiness
sudo systemctl restart docker

TIMEOUT=30
end_time=$((SECONDS + TIMEOUT))
while ! sudo docker info >/dev/null 2>&1; do
    if [[ ${SECONDS} -ge ${end_time} ]]; then
        echo "ERROR: Docker not ready after hardening" >&2
        exit 1
    fi
    sleep 1
done

echo "==> VM hardening complete"
