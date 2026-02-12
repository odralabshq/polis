#!/bin/bash
# agents/openclaw/install.sh
# Installs OpenClaw dependencies and builds the app inside the workspace container.
# Runs at container boot via systemd ExecStartPre (as root).
# Idempotent: skips if already installed.
set -euo pipefail

MARKER="/var/lib/openclaw-installed"
if [[ -f "$MARKER" ]]; then
    echo "[openclaw-install] Already installed, skipping."
    exit 0
fi

echo "[openclaw-install] First boot — installing OpenClaw..."

# Trust Polis CA for SSL connections (if present)
if [[ -f /usr/local/share/ca-certificates/polis-ca.crt ]]; then
    update-ca-certificates 2>/dev/null || true
    export NODE_EXTRA_CA_CERTS=/etc/ssl/certs/ca-certificates.crt
fi

# Install build dependencies and Node.js 22
apt-get update && apt-get install -y --no-install-recommends \
    gnupg unzip git build-essential python3 jq
mkdir -p /etc/apt/keyrings
curl -fsSL https://deb.nodesource.com/gpgkey/nodesource-repo.gpg.key \
    | gpg --dearmor -o /etc/apt/keyrings/nodesource.gpg
echo "deb [signed-by=/etc/apt/keyrings/nodesource.gpg] https://deb.nodesource.com/node_22.x nodistro main" \
    > /etc/apt/sources.list.d/nodesource.list
apt-get update && apt-get install -y --no-install-recommends nodejs
corepack enable

# Pre-install pnpm so corepack doesn't need network access at runtime
corepack prepare pnpm@latest --activate
rm -rf /var/lib/apt/lists/*

# Install Bun
curl -fsSL https://bun.sh/install | bash
export PATH="/root/.bun/bin:${PATH}"

# Clone and build OpenClaw
cd /app || mkdir -p /app && cd /app
git clone --depth 1 https://github.com/openclaw/openclaw.git .
pnpm install --frozen-lockfile
OPENCLAW_A2UI_SKIP_MISSING=1 pnpm build
OPENCLAW_PREFER_PNPM=1 pnpm ui:build

# Set production environment
export NODE_ENV=production

# Create directories with proper permissions
mkdir -p /home/polis/.openclaw/{workspace,agents,sessions}
chown -R polis:polis /app /home/polis/.openclaw

# Copy scripts from agent bundle
cp /tmp/agents/openclaw/scripts/health.sh /usr/local/bin/openclaw-health.sh
cp /tmp/agents/openclaw/scripts/init.sh /usr/local/bin/openclaw-init.sh
chmod 755 /usr/local/bin/openclaw-health.sh /usr/local/bin/openclaw-init.sh

# Install SOUL.md (HITL security workflow instructions for the agent)
mkdir -p /usr/local/share/openclaw
cp /tmp/agents/openclaw/config/SOUL.md /usr/local/share/openclaw/SOUL.md
chmod 644 /usr/local/share/openclaw/SOUL.md

# Create openclaw CLI wrapper
printf '#!/bin/bash\nexec /usr/bin/node /app/dist/index.js "$@"\n' > /usr/local/bin/openclaw
chmod 755 /usr/local/bin/openclaw

# Enable systemd service
ln -sf /etc/systemd/system/openclaw.service \
       /etc/systemd/system/multi-user.target.wants/openclaw.service

# Final ownership
chown -R polis:polis /app /home/polis

# Mark as installed (idempotency guard)
touch "$MARKER"
echo "[openclaw-install] Installation complete."
