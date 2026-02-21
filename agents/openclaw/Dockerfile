# agents/openclaw/Dockerfile
# Extends the base workspace image with OpenClaw pre-installed.
# This avoids the 3-5 minute runtime install on every fresh container start.
#
# Build:  docker compose -f docker-compose.yml -f agents/openclaw/.generated/compose.override.yaml build workspace
# Or:     docker build -f agents/openclaw/Dockerfile -t polis-workspace-openclaw:latest .
#
# The base workspace image must be built first (docker compose build workspace).

ARG WORKSPACE_IMAGE=ghcr.io/odralabshq/polis-workspace-oss:latest
# checkov:skip=CKV_DOCKER_7: base image is injected at build time via WORKSPACE_IMAGE ARG; default :latest is never used in production
FROM ${WORKSPACE_IMAGE}

# ── Install Node.js 22 + build deps ────────────────────────────────
RUN apt-get update && apt-get install -y --no-install-recommends \
        gnupg unzip git build-essential python3 jq \
    && mkdir -p /etc/apt/keyrings \
    && curl -fsSL https://deb.nodesource.com/gpgkey/nodesource-repo.gpg.key \
        | gpg --batch --yes --dearmor -o /etc/apt/keyrings/nodesource.gpg \
    && echo "deb [signed-by=/etc/apt/keyrings/nodesource.gpg] https://deb.nodesource.com/node_22.x nodistro main" \
        > /etc/apt/sources.list.d/nodesource.list \
    && apt-get update && apt-get install -y --no-install-recommends nodejs \
    && corepack disable 2>/dev/null || true \
    && npm install -g pnpm@latest \
    && rm -rf /var/lib/apt/lists/*

# ── Install Bun ────────────────────────────────────────────────────
ENV HOME=/root
RUN curl -fsSL https://bun.sh/install | bash
ENV PATH="/root/.bun/bin:${PATH}"

# ── Clone and build OpenClaw ───────────────────────────────────────
WORKDIR /app
RUN git clone --depth 1 https://github.com/openclaw/openclaw.git . \
    && pnpm install --frozen-lockfile --network-concurrency=4 \
    && OPENCLAW_A2UI_SKIP_MISSING=1 pnpm build \
    && OPENCLAW_PREFER_PNPM=1 pnpm ui:build \
    && rm -rf .git

# ── Install agent scripts and CLI wrapper ──────────────────────────
COPY agents/openclaw/scripts/health.sh   /usr/local/bin/openclaw-health.sh
COPY agents/openclaw/scripts/init.sh     /usr/local/bin/openclaw-init.sh
COPY agents/openclaw/config/SOUL.md      /usr/local/share/openclaw/SOUL.md

# Install polis security CLI wrappers
COPY agents/openclaw/scripts/polis-toolbox-call.sh    /usr/local/share/openclaw/scripts/polis-toolbox-call.sh
COPY agents/openclaw/scripts/polis-report-block.sh    /usr/local/share/openclaw/scripts/polis-report-block.sh
COPY agents/openclaw/scripts/polis-check-status.sh    /usr/local/share/openclaw/scripts/polis-check-status.sh
COPY agents/openclaw/scripts/polis-list-pending.sh    /usr/local/share/openclaw/scripts/polis-list-pending.sh
COPY agents/openclaw/scripts/polis-security-status.sh /usr/local/share/openclaw/scripts/polis-security-status.sh
COPY agents/openclaw/scripts/polis-security-log.sh    /usr/local/share/openclaw/scripts/polis-security-log.sh

RUN chmod 755 /usr/local/bin/openclaw-health.sh /usr/local/bin/openclaw-init.sh \
    && chmod 755 /usr/local/share/openclaw/scripts/*.sh \
    && printf '#!/bin/bash\nexec /usr/bin/node /app/dist/index.js "$@"\n' > /usr/local/bin/openclaw \
    && chmod 755 /usr/local/bin/openclaw

# ── Prepare directories ───────────────────────────────────────────
RUN mkdir -p /home/polis/.openclaw/{workspace,agents,sessions} \
    && chown -R polis:polis /app /home/polis/.openclaw

# ── Mark as pre-installed (install.sh will skip) ──────────────────
RUN touch /var/lib/openclaw-installed

ENV NODE_ENV=production
WORKDIR /
ENTRYPOINT ["/sbin/init"]
