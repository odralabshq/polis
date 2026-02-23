#!/bin/bash
# install-polis.sh — Install Polis orchestration files into VM image
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive

echo "==> Installing Polis orchestration files..."

# Create polis directory structure
sudo mkdir -p /opt/polis
sudo chown ubuntu:ubuntu /opt/polis

# Extract polis bundle (uploaded by packer)
cd /opt/polis
tar -xzf /tmp/polis-config.tar.gz
rm /tmp/polis-config.tar.gz

# Install netcat for SSH proxy and yq for agent manifest parsing
sudo apt-get update -qq
sudo apt-get install -y --no-install-recommends netcat-openbsd
sudo wget -qO /usr/local/bin/yq https://github.com/mikefarah/yq/releases/latest/download/yq_linux_amd64
sudo chmod +x /usr/local/bin/yq

# Create systemd service for polis
sudo tee /etc/systemd/system/polis.service > /dev/null << 'EOF'
[Unit]
Description=Polis Secure Workspace
After=docker.service sysbox.service
Requires=docker.service

[Service]
Type=oneshot
RemainAfterExit=yes
WorkingDirectory=/opt/polis
ExecStartPre=/opt/polis/scripts/setup-certs.sh
ExecStart=/usr/bin/docker compose up -d
ExecStop=/usr/bin/docker compose down
TimeoutStartSec=120
User=root

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable polis.service

echo "==> Polis installation complete"
