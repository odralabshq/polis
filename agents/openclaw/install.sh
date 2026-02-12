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
# NODE_EXTRA_CA_CERTS is needed for corepack/pnpm which use Node.js fetch()
if [[ -f /usr/local/share/ca-certificates/polis-ca.crt ]]; then
    update-ca-certificates 2>/dev/null || true
    export NODE_EXTRA_CA_CERTS=/etc/ssl/certs/polis-ca.pem
fi

# Ensure apt can reach Debian mirrors in proxied environments.
apt_update() {
    if apt-get update; then
        return 0
    fi

    echo "[openclaw-install] apt-get update failed, retrying with HTTPS Debian sources..."
    if [[ -f /etc/apt/sources.list.d/debian.sources ]]; then
        sed -i 's|http://deb.debian.org|https://deb.debian.org|g' /etc/apt/sources.list.d/debian.sources
        sed -i 's|http://security.debian.org|https://security.debian.org|g' /etc/apt/sources.list.d/debian.sources
    fi

    apt-get update
}

# Install build dependencies when apt is available.
if apt_update; then
    apt-get install -y --no-install-recommends \
        unzip git build-essential python3 jq ca-certificates curl \
        || echo "[openclaw-install] WARNING: apt dependency install partially failed, continuing with fallback path"
else
    echo "[openclaw-install] WARNING: apt-get update failed, continuing with fallback install path"
fi

# Install Node.js 22 from upstream binary (avoids distro/NodeSource coupling).
NODE_VERSION="22.20.0"
NODE_DIST="node-v${NODE_VERSION}-linux-x64"
NODE_TARBALL="${NODE_DIST}.tar.xz"
curl -fsSL "https://nodejs.org/dist/v${NODE_VERSION}/${NODE_TARBALL}" -o "/tmp/${NODE_TARBALL}"
mkdir -p /usr/local/lib/nodejs
tar -xJf "/tmp/${NODE_TARBALL}" -C /usr/local/lib/nodejs
ln -sf "/usr/local/lib/nodejs/${NODE_DIST}/bin/node" /usr/local/bin/node
ln -sf "/usr/local/lib/nodejs/${NODE_DIST}/bin/npm" /usr/local/bin/npm
ln -sf "/usr/local/lib/nodejs/${NODE_DIST}/bin/npx" /usr/local/bin/npx
ln -sf "/usr/local/lib/nodejs/${NODE_DIST}/bin/corepack" /usr/local/bin/corepack
ln -sf /usr/local/bin/node /usr/bin/node
corepack enable

# Pre-install pnpm so corepack doesn't need network access at runtime
corepack prepare pnpm@latest --activate
rm -rf /var/lib/apt/lists/*

# Install Bun
curl -fsSL https://bun.sh/install | bash
export PATH="/root/.bun/bin:${PATH}"

# Download and build OpenClaw
cd /app || { mkdir -p /app && cd /app; }
rm -rf /app/*
curl -fsSL https://codeload.github.com/openclaw/openclaw/tar.gz/refs/heads/main -o /tmp/openclaw.tar.gz
tar -xzf /tmp/openclaw.tar.gz -C /app --strip-components=1
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

# Final ownership
chown -R polis:polis /app /home/polis

# Mark as installed (idempotency guard)
touch "$MARKER"
echo "[openclaw-install] Installation complete."
