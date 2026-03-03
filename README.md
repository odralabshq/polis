# Polis — Secure Workspace for AI Coding Agents

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/version-0.4.0-orange.svg)](https://github.com/OdraLabsHQ/polis/releases)

> **⚠️ Experimental Preview** — Polis is under active development This platform in not yet recommended for production use.

Polis is a secure runtime for AI coding agents. It wraps any AI agent in an isolated VM where all network traffic is intercepted, inspected for malware, and audited — without modifying the agent itself.

## The Problem

AI agents make HTTP requests, download packages, and execute code autonomously. A container alone doesn't stop an agent from exfiltrating secrets over HTTPS, pulling malicious dependencies, or connecting to unauthorized services. You need network-level visibility and control.

Polis solves this by routing all agent traffic through a TLS-intercepting proxy with real-time malware scanning. The agent runs normally; Polis handles security transparently.

## ⚡️ Quick Start

## Supported Platforms

| OS | Architecture | Status |
|----|-------------|--------|
| Linux | amd64 | ✅ Supported |
| Windows | amd64 | ✅ Supported |
| macOS | arm64 | 🔜 Coming soon |

### Linux (amd64)

```bash
curl -fsSL https://raw.githubusercontent.com/OdraLabsHQ/polis/main/scripts/install.sh | bash
```

### Windows (PowerShell)

Works on PowerShell 5.1 and newer:

```powershell
irm https://raw.githubusercontent.com/OdraLabsHQ/polis/main/scripts/install.ps1 | iex
```

### macOS — Coming soon

🔜 macOS arm64 support is on the roadmap.

---

The installer downloads the Polis CLI and a pre-built VM image (~1.8 GB), installs [Multipass](https://multipass.run) if needed, and starts the workspace. No source code or build tools required.

Once installed:

```bash
polis status                   # Show workspace and agent status
polis connect                  # Connect to workspace via SSH or IDE
polis start --agent=openclaw   # Start Polis with pre-configured openclaw agent
```

To build from source instead, see [docs/DEVELOPER.md](docs/DEVELOPER.md).

### Windows Networking Notes

- Set your active network adapter to a **Private** network profile. Multipass VMs may not get an IP on Public networks.
- Turn off VPN during VM creation and startup if you experience networking issues.
- Applications running at `localhost` inside the workspace container are accessible through the VM IP. Find it with `multipass list`.

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

# Set security level (relaxed, balanced, or strict)
polis config set security.level strict
```

| Level | Behavior |
|-------|----------|
| `relaxed` | New domains auto-allowed, credentials trigger approval |
| `balanced` (default) | New domains prompt for approval, known domains auto-allow |
| `strict` | All domains require explicit approval |

**Notes:**
- Credentials (API keys, AWS keys, private keys) always trigger approval regardless of level
- Malware is always blocked regardless of level
- Changes propagate to running workspace immediately

---

## 🛡️ DLP & Network Security

Polis routes all workspace traffic through a transparent TLS-intercepting proxy (the "Governance Engine"). This means every HTTP/HTTPS request from inside the workspace is inspected before reaching the internet.

### How it works

- All outbound traffic goes through g3proxy → ICAP → governance engine
- Known-safe domains (package managers, GitHub, IDEs, AI tools) are on a built-in bypass list and pass through without inspection
- LLM provider domains (OpenAI, Anthropic, etc.) get full DLP scanning (secrets + PII detection)
- Unknown domains get light DLP scanning (secrets only) on `balanced` level, or are blocked outright on `strict`

### When a request is blocked

If you see a `403 Forbidden` or `curl: (22) The requested URL returned error: 403` inside the workspace, the governance engine likely blocked the request. This happens when:

- The domain isn't on the bypass list and the security level is `strict`
- The request body contains detected secrets or credentials
- The domain is on the DNS blocklist

### Inspecting blocked requests

From the host (outside the workspace):

```bash
# Check governance logs for a specific domain
polis exec -- cat /var/log/polis/governance.log | grep "example.com"

# Check the current security level
polis config show

# View the DNS blocklist
polis exec -- cat /etc/polis/dns-blocklist.txt
```

From inside the workspace (via SSH):

```bash
# Check if a domain is reachable
curl -v https://example.com

# Look at proxy logs
cat /var/log/polis/governance.log | tail -50
```

### Adding domain exceptions

If a legitimate domain is being blocked, you can add it to the bypass list via the governance API:

```bash
# Add a single domain exception
polis exec -- curl -s -X POST http://localhost:8082/exceptions/domains \
  -H 'Content-Type: application/json' \
  -d '{"domain": "example.com"}'

# Verify it was added
polis exec -- curl -s http://localhost:8082/exceptions/domains
```

Alternatively, lower the security level:

```bash
# Switch to relaxed (auto-allows unknown domains, still scans for credentials)
polis config set security.level relaxed

# Switch back to balanced (default)
polis config set security.level balanced
```

### Default bypass domains

Polis ships with a comprehensive bypass list covering common development tools. These domains are never blocked regardless of security level:

| Category | Examples |
|----------|---------|
| Linux packages | `deb.debian.org`, `archive.ubuntu.com`, `dl-cdn.alpinelinux.org` |
| Node.js / JS | `registry.npmjs.org`, `registry.yarnpkg.com`, `nodejs.org`, `bun.sh` |
| Python | `pypi.org`, `files.pythonhosted.org`, `conda.anaconda.org` |
| Rust | `crates.io`, `sh.rustup.rs`, `static.rust-lang.org` |
| Go | `proxy.golang.org`, `sum.golang.org` |
| Ruby / Java / PHP / .NET | `rubygems.org`, `repo1.maven.org`, `packagist.org`, `api.nuget.org` |
| Container registries | `*.docker.io`, `ghcr.io`, `quay.io`, `mcr.microsoft.com`, `public.ecr.aws` |
| GitHub | `github.com`, `*.githubusercontent.com` |
| VS Code | `marketplace.visualstudio.com`, `update.code.visualstudio.com` |
| Cursor | `api2.cursor.sh` – `api5.cursor.sh`, `*.gcpp.cursor.sh`, `marketplace.cursorapi.com` |
| Windsurf / Codeium | `server.codeium.com`, `windsurf-stable.codeiumdata.com` |
| JetBrains | `plugins.jetbrains.com`, `download.jetbrains.com`, `api.jetbrains.ai` |
| Kiro CLI | `cli.kiro.dev`, `desktop-release.q.us-east-1.amazonaws.com` |
| Amazon Q | `codewhisperer.us-east-1.amazonaws.com`, `q.us-east-1.amazonaws.com` |
| GitHub Copilot | `copilot-proxy.githubusercontent.com`, `*.githubcopilot.com` |
| Claude Code | `claude.ai`, `*.claude.ai` |
| Sourcegraph Cody | `sourcegraph.com`, `*.sourcegraph.com` |
| Tabnine | `tabnine.com`, `*.tabnine.com` |
| Continue.dev | `continue.dev` |
| Infrastructure | `releases.hashicorp.com`, `registry.terraform.io`, `dl.k8s.io`, `brew.sh` |
| Certificate authorities | `*.digicert.com`, `*.globalsign.com`, `letsencrypt.org` |
| CDNs | `*.s3.amazonaws.com`, `*.cloudfront.net` |

The full list is defined in `services/sentinel/modules/dlp/srv_polis_dlp.c` → `is_new_domain()`.

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
| Compromised agent exfiltrates credentials | TLS interception + DLP engine scans for AWS keys, GitHub tokens, OpenAI/Anthropic keys, private keys |
| Agent requests access to new domains | Human-in-the-loop (HITL) approval system blocks requests until user confirms |
| Malicious code downloaded by agent | ClamAV scans every HTTP response via ICAP before it reaches the agent |
| Agent attempts non-HTTP connections | Only HTTP/HTTPS (80/443) allowed outbound; all other ports blocked via iptables |
| Container escape vulnerability | Sysbox runtime provides VM-like isolation without privileged mode |
| Proxy bypass via IPv6 | IPv6 disabled at Docker network level and via sysctl/ip6tables in containers |
| Unauthorized host resource access | No Docker socket mounted; only read-only CA cert and init scripts bind-mounted |
| Data exfiltration via DNS tunneling | All traffic forced through proxy; non-HTTP ports blocked |
| Cloud metadata service access (SSRF) | Blocked by network isolation — workspace has no route to 169.254.169.254 |

### Coming Soon

| Threat | Status |
|--------|--------|
| Typosquatted packages (`nxdebug` vs `nx-debug`) | 🔜 Package name validation |
| Poisoned dependencies in lockfiles | 🔜 Dependency integrity checks |
| Tool chaining for exfiltration (DB read → HTTP POST) | 🔜 MCP tool call auditing |
| Indirect prompt injection via fetched content | 🔜 Content sanitization |

---

## 🖥️ Platform Support

| Platform | Status | Notes |
|----------|--------|-------|
| **Linux** (amd64) | ✅ Supported | Recommended |
| **Windows** (amd64) | ✅ Supported | Requires Hyper-V or VirtualBox |
| **macOS** (arm64) | 🔜 Coming soon | On the roadmap |

---

## 🔧 Troubleshooting

**Multipass not found (Linux):**

```bash
sudo snap install multipass
```

**Multipass not found (Windows):**

The installer handles this automatically. If you need to install manually, enable Hyper-V or install VirtualBox first, then download from [multipass.run](https://multipass.run/install).

**Windows VM has no network / can't reach internet:**

```powershell
# Switch your network adapter to Private profile
Set-NetConnectionProfile -InterfaceAlias "Wi-Fi" -NetworkCategory Private
# Disconnect VPN, then restart the VM
multipass restart polis
```

**Accessing agent UI on Windows:**

The agent UI runs on `localhost` inside the VM. Use the VM IP instead:

```powershell
multipass list   # Find the polis VM IP
# Open http://<vm-ip>:18789 in your browser
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

## 📄 License

Apache 2.0 — See [LICENSE](LICENSE) for details.

## ⚠️ Disclaimer

Polis provides defense-in-depth security but is not a silver bullet. Always review agent outputs before deployment, keep secrets out of workspaces, and monitor audit logs.

---

Built with ❤️ in Warsaw 🇵🇱
