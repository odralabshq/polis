# Polis - Secure Workspace for AI Coding Agents

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/version-0.3.0-green.svg)](CHANGELOG.md)

Polis is a secure runtime for AI coding agents. It wraps any AI agent in an isolated container where all network traffic is intercepted, inspected for malware, and audited — without modifying the agent itself.

## The Problem

AI agents make HTTP requests, download packages, and execute code autonomously. A container alone doesn't stop an agent from exfiltrating secrets over HTTPS, pulling malicious dependencies, or connecting to unauthorized services. You need network-level visibility and control.

Polis solves this by routing all agent traffic through a TLS-intercepting proxy with real-time malware scanning. The agent runs normally; Polis handles security transparently.

## ⚡️ Quick Start for Users

Run Polis in a fully automated VM. Everything is pre-configured via cloud-init:

### 1. Install Multipass

**Windows:**

```powershell
winget install Canonical.Multipass
```

**macOS:**

```bash
brew install multipass
```

**Linux:**

```bash
sudo snap install multipass
```

### 2. Create and Start Polis VM

```bash
# Download cloud-init config
wget https://raw.githubusercontent.com/OdraLabsHQ/polis/main/polis-vm.yaml

# Launch VM (takes 5-10 minutes to provision)
multipass launch \
  --name polis-vm \
  --cpus 4 \
  --memory 8G \
  --disk 50G \
  --cloud-init polis-vm.yaml \
  24.04

# Wait for cloud-init to complete
multipass exec polis-vm -- cloud-init status --wait
```

### 3. Configure and Start

```bash
# Access the VM
multipass shell polis-vm

# Configure your API key (at least one required)
cd ~/polis
nano .env  # Add ANTHROPIC_API_KEY or OPENAI_API_KEY

# Initialize and start Polis
./cli/polis.sh init --agent=openclaw

# Initialize the agent and get your access token
./cli/polis.sh openclaw init
```

Access the agent UI at `http://<VM_IP>:18789` (get IP with `multipass info polis-vm`).

---

## 🛠️ Quick Start for Developers

Edit code locally, build and run in VM. Fully automated via scripts:

- **Linux/macOS**: `./tools/polis-dev.sh`
- **Windows**: `.\tools\polis-dev.ps1`

### 1. Install Multipass

Same as above (Windows/macOS/Linux instructions).

### 2. Clone Repository and Create Dev VM

**Linux/macOS:**

```bash
# Clone on your host machine
git clone --recursive https://github.com/OdraLabsHQ/polis.git
cd polis

# Make script executable
chmod +x tools/polis-dev.sh

# Create development VM
./tools/polis-dev.sh create
```

**Windows (PowerShell):**

```powershell
# Clone on your host machine
git clone --recursive https://github.com/OdraLabsHQ/polis.git
cd polis

# Create development VM
.\tools\polis-dev.ps1 create
```

This will:

- Launch Ubuntu VM with cloud-init (polis-dev.yaml)
- Install Docker + Sysbox automatically
- Mount your local polis directory at ~/polis in VM

### 3. Build and Run from Local Source

**From your host terminal:**

```bash
# Linux/macOS
./tools/polis-dev.sh shell

# Windows
.\tools\polis-dev.ps1 shell

# Configure API keys
cd ~/polis
cp agents/openclaw/config/env.example .env
nano .env  # Add your API keys

# Build from local source and start
./cli/polis.sh init --agent=openclaw --local

# Get access token
./cli/polis.sh openclaw init
```

**Development Workflow:**

```bash
# Edit files on your host machine (with your IDE)
# Changes are instantly reflected in the VM

# Rebuild after changes (from host)
./tools/polis-dev.sh rebuild     # Linux/macOS
.\tools\polis-dev.ps1 rebuild    # Windows

# Or manually in VM shell
cd ~/polis
./cli/polis.sh down
./cli/polis.sh init --local --no-cache
```

**Why No Permission Issues?**

The VM's `ubuntu` user is configured with UID/GID 1000:1000 (matching most host systems). This means:

- Files you create on your host are owned by UID 1000
- Files created in the VM are also owned by UID 1000
- Both contexts can read/write seamlessly

**Useful Commands:**

*Linux/macOS:*

```bash
./tools/polis-dev.sh create    # Create dev VM
./tools/polis-dev.sh shell     # Enter VM shell
./tools/polis-dev.sh rebuild   # Rebuild from source
./tools/polis-dev.sh stop      # Stop VM
./tools/polis-dev.sh start     # Start VM
./tools/polis-dev.sh status    # Show VM info
./tools/polis-dev.sh delete    # Delete VM
```

*Windows (PowerShell):*

```powershell
.\tools\polis-dev.ps1 create   # Create dev VM
.\tools\polis-dev.ps1 shell    # Enter VM shell
.\tools\polis-dev.ps1 rebuild  # Rebuild from source
.\tools\polis-dev.ps1 stop     # Stop VM
.\tools\polis-dev.ps1 start    # Start VM
.\tools\polis-dev.ps1 status   # Show VM info
.\tools\polis-dev.ps1 delete   # Delete VM
```

**If You Still Have Permission Issues:**

On rare systems where your host UID is not 1000:

```bash
# Check your host UID
id -u

# If not 1000, fix permissions manually
./tools/polis-dev.sh fix-perms
```

---

## 🐧 Native Linux Installation

For bare-metal Ubuntu/Debian systems:

```bash
# Install Docker and Docker Compose
sudo apt-get update
sudo apt-get install -y docker.io docker-compose-v2

# Install Sysbox
SYSBOX_VERSION="0.6.4"
wget https://downloads.nestybox.com/sysbox/releases/v${SYSBOX_VERSION}/sysbox-ce_${SYSBOX_VERSION}-0.linux_amd64.deb
sudo apt-get install -y ./sysbox-ce_${SYSBOX_VERSION}-0.linux_amd64.deb
sudo systemctl restart docker

# Clone Polis
git clone --recursive https://github.com/OdraLabsHQ/polis.git
cd polis

# Configure and start
cp agents/openclaw/config/env.example .env
nano .env  # Add your API keys
./cli/polis.sh init --agent=openclaw
```

## 🏗️ Architecture

Polis routes all workspace traffic through a TLS-intercepting proxy with ICAP-based content inspection:

```
  Browser ──► http://localhost:18789 (Agent UI)
                      │
  ┌───────────────────┼────────────────────────────┐
  │  Workspace (Sysbox-isolated)                   │
  │                   │                            │
  │    AI Agent (OpenClaw, or any agent)            │
  │         • Full dev environment                 │
  │         • Docker-in-Docker support             │
  │         • No host access                       │
  │                   │ all traffic                 │
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
| Agent connects to unauthorized services | Only HTTP/HTTPS (80/443) allowed outbound; all other ports blocked via nftables |
| Container escape to host system | Sysbox runtime provides VM-like isolation without privileged mode |
| IPv6 bypass of proxy controls | IPv6 disabled at Docker network level and via sysctl/nftables in containers |
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
| **Multipass VM** (Windows/macOS/Linux) | ✅ **Recommended** | Cross-platform, consistent environment |
| Other Linux distros | 🔜 Coming soon | RHEL, Fedora, Arch |

## 📋 Command Reference

### Lifecycle

| Command | Description |
|---------|-------------|
| `polis init` | Full setup: Docker check, Sysbox install, CA generation, image build, start |
| `polis up` | Start containers |
| `polis down` | Stop and remove containers |
| `polis stop` | Stop containers (preserves state) |
| `polis start` | Start existing stopped containers |
| `polis status` | Show container health |
| `polis logs [service]` | Stream container logs |
| `polis shell` | Enter workspace shell |

### Agent Commands

| Command | Description |
|---------|-------------|
| `polis <agent> init` | Initialize agent, wait for ready, show access token |
| `polis <agent> status` | Show agent service status |
| `polis <agent> logs [n]` | Show last n lines of agent logs (default: 50) |
| `polis <agent> restart` | Restart agent service |
| `polis <agent> shell` | Enter workspace shell |
| `polis <agent> help` | Show all commands for this agent |

### Agent Management

| Command | Description |
|---------|-------------|
| `polis agents list` | List available agents |
| `polis agents info <name>` | Show agent metadata |
| `polis agent scaffold <name>` | Create new agent from template |

### Setup

| Command | Description |
|---------|-------------|
| `polis setup-ca [--force]` | Generate or regenerate CA certificate |
| `polis setup-sysbox [--force]` | Install or reinstall Sysbox runtime |
| `polis setup-env` | Validate agent environment variables |
| `polis build [service]` | Build container images |
| `polis test [unit\|integration\|e2e]` | Run tests |

### Init Options

| Flag | Description |
|------|-------------|
| `--agent=<name>` | Agent to use (default: `openclaw`) |
| `--local` | Build images from source instead of pulling from registry |
| `--no-cache` | Build without Docker cache |

## 🔌 Agent Plugin System

Polis is agent-agnostic. OpenClaw is the default, but any agent can be packaged as a plugin under `agents/<name>/`:

```
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

Create a new agent:

```bash
polis agent scaffold myagent
```

## ⚙️ Configuration

Add at least one API key to `.env`:

```bash
ANTHROPIC_API_KEY=sk-ant-...    # → Claude (auto-detected)
OPENAI_API_KEY=sk-proj-...      # → GPT-4o
OPENROUTER_API_KEY=sk-or-...    # → Multiple models
```

After changing API keys, rebuild:

```bash
polis down && polis init --agent=openclaw
```

Proxy configuration lives in `services/gate/config/g3proxy.yaml`. Sentinel logic is in `services/sentinel/config/c-icap.conf`. Global settings are in `config/polis.yaml`. Network isolation is defined in `docker-compose.yml`.

## 🔧 Troubleshooting

**Sysbox not detected** — Start services manually, then restart Docker:

```bash
sudo systemctl stop docker docker.socket
sudo systemctl restart sysbox-mgr sysbox-fs
sudo systemctl start docker
docker info | grep sysbox
```


**Full reset:**

```bash
polis down
docker rmi $(docker images --filter "reference=polis-*" -q) 2>/dev/null
rm -f certs/ca/ca.key certs/ca/ca.pem
polis init --agent=openclaw
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
