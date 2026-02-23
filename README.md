# Polis — Secure Workspace for AI Coding Agents

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/version-0.3.0--preview-orange.svg)](https://github.com/OdraLabsHQ/polis/releases)

> **⚠️ Experimental Preview** — Polis is under active development This platform in not yet recommended for production use.

Polis is a secure runtime for AI coding agents. It wraps any AI agent in an isolated VM where all network traffic is intercepted, inspected for malware, and audited — without modifying the agent itself.

## The Problem

AI agents make HTTP requests, download packages, and execute code autonomously. A container alone doesn't stop an agent from exfiltrating secrets over HTTPS, pulling malicious dependencies, or connecting to unauthorized services. You need network-level visibility and control.

Polis solves this by routing all agent traffic through a TLS-intercepting proxy with real-time malware scanning. The agent runs normally; Polis handles security transparently.

## ⚡️ Quick Start

### Linux

```bash
curl -fsSL https://raw.githubusercontent.com/OdraLabsHQ/polis/main/scripts/install.sh | bash
```

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/OdraLabsHQ/polis/main/scripts/install.ps1 | iex
```

### macOS

🔜 Coming soon. macOS support is on the roadmap.

---

The installer downloads the Polis CLI and a pre-built VM image (~1.8 GB), installs [Multipass](https://multipass.run) if needed, and starts the workspace. No source code or build tools required.

Once installed:

```bash
polis start                    # Start workspace with default agent (OpenClaw)
polis start --agent=openclaw   # Explicitly choose an agent
polis connect --ide vscode     # Open workspace in VS Code
```

To build from source instead, see [docs/DEVELOPER.md](docs/DEVELOPER.md).

---

## CLI Commands

| Command | Description |
|---------|-------------|
| `polis start` | Start workspace (downloads image on first run) |
| `polis start --agent=<name>` | Start with a specific agent |
| `polis start --image <path>` | Use a custom VM image |
| `polis stop` | Stop workspace (preserves state) |
| `polis delete` | Remove workspace |
| `polis delete --all` | Remove workspace, certs, config, and cached images |
| `polis status` | Show workspace and agent status |
| `polis connect` | Show connection options (SSH, IDE) |
| `polis connect --ide vscode` | Open workspace directly in VS Code |
| `polis connect --ide cursor` | Open workspace directly in Cursor |
| `polis exec <cmd>` | Run a command inside the workspace |
| `polis doctor` | Diagnose issues (workspace, network, image) |
| `polis update` | Update Polis to the latest signed release |
| `polis update --check` | Check for updates without applying |
| `polis config show` | Show current configuration |
| `polis config set <key> <value>` | Set a configuration value |
| `polis version` | Show CLI version |

### Agent Management

| Command | Description |
|---------|-------------|
| `polis agent list` | List installed agents |
| `polis agent add --path <folder>` | Install a new agent from a local folder |
| `polis agent remove <name>` | Remove an agent |
| `polis agent restart` | Restart the active agent's workspace |
| `polis agent update` | Re-generate config and recreate workspace |
| `polis agent shell` | Open an interactive shell in the workspace |
| `polis agent exec <cmd>` | Run a command in the workspace container |
| `polis agent cmd <args>` | Run an agent-specific command (defined in the agent's `commands.sh`) |

---

## Agents

Polis is agent-agnostic. Agents are defined under `agents/<name>/` with an `agent.yaml` manifest. [OpenClaw](https://github.com/nicepkg/openclaw) is the default bundled agent.

### OpenClaw

OpenClaw is an AI coding agent with a browser-based Control UI. It supports Anthropic, OpenAI, and OpenRouter as LLM providers.

```bash
# Set at least one API key before starting
export ANTHROPIC_API_KEY=sk-ant-...
# or: export OPENAI_API_KEY=sk-proj-...
# or: export OPENROUTER_API_KEY=sk-or-...

polis start
```

OpenClaw installs on first boot (~3–5 min). Once ready, open the Control UI:

```
http://<host-ip>:18789/#token=<token>
```

Get the token:

```bash
polis agent cmd token
```

On Multipass, use the VM IP (`multipass info polis` to find it). On native Linux, use `localhost`.

### Adding Custom Agents

Use the agent template to create your own:

```bash
cp -r agents/_template agents/my-agent
# Edit agents/my-agent/agent.yaml
polis agent add --path agents/my-agent
polis start --agent=my-agent
```

---

## Configuration

```bash
# Show current config
polis config show

# Set security level (balanced or strict)
polis config set security.level strict
```

| Level | Behavior |
|-------|----------|
| `balanced` (default) | New domains prompt for approval, known domains auto-allow |
| `strict` | All domains require explicit approval |

Credentials and malware are always blocked regardless of security level.

---

## 🏗️ Architecture

Polis routes all workspace traffic through a TLS-intercepting proxy with ICAP-based content inspection:

```text
  Browser ──► http://localhost:18789 (Agent UI)
                      │
  ┌───────────────────┼────────────────────────────┐
  │  Workspace (Sysbox-isolated VM)                │
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
| **Toolbox** | MCP tools for agent interaction | `services/toolbox` |
| **State** | Redis-compatible data store (Valkey) | `services/state` |
| **Workspace** | Isolated environment (Sysbox) | `services/workspace` |

---

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

---

## 🖥️ Platform Support

| Platform | Status | Notes |
|----------|--------|-------|
| **Linux** (Ubuntu/Debian) | ✅ Supported | Recommended for development and production |
| **Windows** | ✅ Supported | Via Multipass VM, PowerShell installer |
| **macOS** | 🔜 Coming soon | Multipass-based, on the roadmap |

---

## 🔧 Troubleshooting

**Multipass not found:**

```bash
# Linux
sudo snap install multipass

# Windows
winget install Canonical.Multipass
```

**Workspace won't start:**

```bash
polis doctor         # Diagnose issues
polis delete         # Clean slate
polis start          # Try again
```

**Full reset:**

```bash
polis delete --all   # Remove everything
polis start          # Fresh install
```

---

## 🛡️ Security Framework Alignment

Polis is designed against industry security frameworks:

- **OWASP Top 10 for Agentic Applications** — Agent-specific threat coverage
- **MITRE ATLAS** — AI-specific threat tactics and techniques
- **NIST AI RMF** — Risk management framework alignment

---

## 📄 License

Apache 2.0 — See [LICENSE](LICENSE) for details.

## ⚠️ Disclaimer

Polis provides defense-in-depth security but is not a silver bullet. Always review agent outputs before deployment, keep secrets out of workspaces, and monitor audit logs.

---

Built with ❤️ in Warsaw 🇵🇱
