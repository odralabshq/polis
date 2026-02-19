# Polis - Secure Workspace for AI Coding Agents

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/version-0.3.0-green.svg)](CHANGELOG.md)

Polis is a secure runtime for AI coding agents. It wraps any AI agent in an isolated container where all network traffic is intercepted, inspected for malware, and audited — without modifying the agent itself.

## The Problem

AI agents make HTTP requests, download packages, and execute code autonomously. A container alone doesn't stop an agent from exfiltrating secrets over HTTPS, pulling malicious dependencies, or connecting to unauthorized services. You need network-level visibility and control.

Polis solves this by routing all agent traffic through a TLS-intercepting proxy with real-time malware scanning. The agent runs normally; Polis handles security transparently.

## ⚡️ Quick Start (Users)

Install the Polis CLI and run an agent:

```bash
# Install CLI
curl -sSL https://polis.dev/install.sh | bash

# Run an agent
polis run claude-dev
```

The CLI downloads a pre-built VM image and starts the agent. No source code or build tools required.

### CLI Commands

| Command | Description |
|---------|-------------|
| `polis run [agent]` | Create workspace and start agent |
| `polis start` | Start existing workspace |
| `polis stop` | Stop workspace (preserves state) |
| `polis delete` | Remove workspace |
| `polis status` | Show workspace and agent status |
| `polis connect` | Show connection options (SSH, UI) |
| `polis doctor` | Diagnose issues |
| `polis update` | Update Polis CLI |

### Configuration

```bash
# Set default agent
polis config set defaults.agent claude-dev

# View configuration
polis config show
```

---

## 🛠️ Quick Start (Developers)

For contributing to Polis, clone the repo and use `just`:

### Native Linux (Recommended)

```bash
# Install prerequisites
sudo apt-get install -y docker.io docker-compose-v2

# Install Sysbox
SYSBOX_VERSION="0.6.4"
wget https://downloads.nestybox.com/sysbox/releases/v${SYSBOX_VERSION}/sysbox-ce_${SYSBOX_VERSION}-0.linux_amd64.deb
sudo apt-get install -y ./sysbox-ce_${SYSBOX_VERSION}-0.linux_amd64.deb
sudo systemctl restart docker

# Clone and build
git clone --recursive https://github.com/OdraLabsHQ/polis.git
cd polis
just setup && just build && just up
```

### Development VM (macOS/Windows)

```bash
# Clone repo locally
git clone --recursive https://github.com/OdraLabsHQ/polis.git
cd polis

# Create dev VM (requires Multipass)
./tools/dev-vm.sh create

# Enter VM and build
./tools/dev-vm.sh shell
just setup && just build && just up
```

### VS Code Remote Development

1. Get SSH config: `./tools/dev-vm.sh ssh-config >> ~/.ssh/config`
2. In VS Code: `Cmd+Shift+P` → "Remote-SSH: Connect to Host" → `polis-dev`
3. Open folder: `/home/ubuntu/polis`

See [docs/DEVELOPER.md](docs/DEVELOPER.md) for full development guide.

---

## 🏗️ Architecture

Polis routes all workspace traffic through a TLS-intercepting proxy with ICAP-based content inspection:

```text
  Browser ──► http://localhost:18789 (Agent UI)
                      │
  ┌───────────────────┼────────────────────────────┐
  │  Workspace (Sysbox-isolated)                   │
  │                   │                            │
  │    AI Agent (OpenClaw, or any agent)           │
  │         • Full dev environment                 │
  │         • Docker-in-Docker support             │
  │         • No host access                       │
  │                   │ all traffic                │
  └───────────────────┼────────────────────────────┘
                      ▼
  ┌────────────────────────────────────────────────┐
  │  G3Proxy ──► TLS inspect ──► ICAP ──► ClamAV  │
  └───────────────────┬────────────────────────────┘
                      ▼
                  Internet
```

### Network Isolation

Three isolated Docker networks ensure the workspace can never bypass inspection:

| Network | Subnet | Purpose |
|---------|--------|---------|
| internal-bridge | 10.10.1.0/24 | Workspace ↔ Gateway (only route out) |
| gateway-bridge | 10.30.1.0/24 | Gateway ↔ ICAP (content inspection) |
| external-bridge | 10.20.1.0/24 | Gateway ↔ Internet |

### Key Components

| Component | Purpose | Location |
|-----------|---------|----------|
| **Resolver** | DNS entry point (CoreDNS), domain filtering | `services/resolver` |
| **Gateway** | TLS-intercepting proxy (g3proxy), traffic routing | `services/gate` |
| **Sentinel** | Content inspection logic (c-icap), DLP, approvals | `services/sentinel` |
| **Scanner** | Real-time malware scanning (ClamAV) | `services/scanner` |
| **Toolbox** | MCP tools | `services/toolbox` |
| **State** | Redis-compatible data store (Valkey) | `services/state` |
| **Workspace** | Isolated environment (Sysbox) | `services/workspace` |

## 🔐 What We Address

| Threat | How |
|--------|-----|
| Agent exfiltrates API keys or credentials over HTTPS | TLS interception — all encrypted traffic is decrypted and inspected by g3proxy |
| Malicious packages or downloads | ClamAV scans every HTTP response via ICAP before it reaches the agent |
| Agent connects to unauthorized services | Only HTTP/HTTPS (80/443) allowed outbound; all other ports blocked via iptables |
| Container escape to host system | Sysbox runtime provides VM-like isolation without privileged mode |
| IPv6 bypass of proxy controls | IPv6 disabled at Docker network level and via sysctl/ip6tables in containers |
| Agent accesses Docker socket or host resources | No Docker socket mounted; only read-only CA cert and init scripts bind-mounted |
| DNS tunneling exfiltration | All traffic forced through proxy; non-HTTP ports blocked |
| Cloud metadata service access (169.254.169.254) | Blocked by network isolation — workspace has no route to metadata endpoint |

### Coming Soon

| Threat | Status |
|--------|--------|
| Typosquatted packages (`nxdebug` vs `nx-debug`) | 🔜 Coming soon |
| Poisoned dependencies in lockfiles | 🔜 Coming soon |
| DLP engine with secrets/PII detection | 🔜 Coming soon |
| MCP tool gateway with filesystem policies | 🔜 Coming soon |
| Tool chaining for exfiltration (DB read → HTTP POST) | 🔜 Coming soon |
| Indirect prompt injection via fetched content | 🔜 Coming soon |

## 🖥️ Platform Support

| Platform | Status | Notes |
|----------|--------|-------|
| **Native Linux** (Ubuntu/Debian) | ✅ **Recommended** | Best performance, full Sysbox support |
| **Multipass VM** (Windows/macOS/Linux) | ✅ **Supported** | Cross-platform via `polis` CLI |
| Other Linux distros | 🔜 Coming soon | RHEL, Fedora, Arch |

## 🔌 Agent Plugin System

Polis is agent-agnostic. OpenClaw is the default, but any agent can be packaged as a plugin under `agents/<name>/`:

```bash
agents/openclaw/
├── agent.conf              # Metadata and required env vars
├── install.sh              # Runs during image build
├── commands.sh             # Agent-specific CLI commands
├── compose.override.yaml   # Ports, volumes, healthcheck
├── config/openclaw.service # Systemd unit
└── scripts/
    ├── init.sh             # Pre-start setup (token generation, etc.)
    └── health.sh           # Health check
```

List and manage agents:

```bash
polis agents list           # List available agents
polis agents info claude    # Show agent details
```

## ⚙️ Configuration

Add at least one API key to your config:

```bash
polis config set anthropic_api_key sk-ant-...
polis config set openai_api_key sk-proj-...
```

Or set environment variables before running:

```bash
export ANTHROPIC_API_KEY=sk-ant-...
polis run
```

## 🔧 Troubleshooting

**Multipass not found:**

```bash
# macOS
brew install multipass

# Linux
sudo snap install multipass

# Windows
winget install Canonical.Multipass
```

**Workspace won't start:**

```bash
polis doctor    # Diagnose issues
polis delete    # Clean slate
polis run       # Try again
```

**Full reset (developers):**

```bash
just clean-all && just build && just setup && just up
```

## 🛡️ Security Framework Alignment

Polis is designed against industry security frameworks:

- **OWASP Top 10 for Agentic Applications** — Agent-specific threat coverage
- **MITRE ATLAS** — AI-specific threat tactics and techniques
- **NIST AI RMF** — Risk management framework alignment

## 📄 License

Apache 2.0 - See [LICENSE](LICENSE) for details.

## ⚠️ Disclaimer

Polis provides defense-in-depth security but is not a silver bullet. Always review agent outputs before deployment, keep secrets out of workspaces, and monitor audit logs.

---

Built with ❤️ in Warsaw 🇵🇱
