# Polis - Secure Workspace for AI Coding Agents

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)

Polis is a **defense-in-depth security layer** for AI coding agents. It provides a containerized workspace where agents can operate autonomously while all their actions are controlled, inspected, and audited.

## ⚡️ Get Started in 60 Seconds

```bash
# 1. Install the CLI
curl -fsSL https://raw.githubusercontent.com/odralabshq/polis/main/scripts/install.sh | bash

# 2. Setup your local environment
polis install

# 3. Start secure workspace for your agents
polis up

# 4. Access via terminal
polis shell
```

## Security Problems We Solve

AI coding agents introduce threats absent from traditional software: uncontrolled goal drift, multi-step reasoning that bypasses security gates, and recursive capability amplification through tool chaining.

### Security By Default

Simple sandboxing only isolates processes—an agent in a Docker container can still exfiltrate data to any domain, send secrets over encrypted channels, and execute malicious code via test runners or git hooks. Polis adds network-level controls that containers alone cannot provide.

| Threat                                                       | Status         |
| ------------------------------------------------------------ | -------------- |
| Agent exfiltrates API keys, tokens, credentials              | ✅ Addressed   |
| PII leakage (names, emails, SSN)                             | ✅ Addressed   |
| DNS tunneling exfiltration                                   | ✅ Addressed   |
| Exfiltration via "safe" commands (`npm test`, git hooks)     | ✅ Addressed   |
| Agent connects to unauthorized domains                       | ✅ Addressed   |
| Agent bypasses proxy via direct connections                  | ✅ Addressed   |
| Cloud metadata service access (169.254.169.254)              | ✅ Addressed   |
| TLS-encrypted malicious traffic                              | ✅ Addressed   |
| Agent escapes to host system                                 | ✅ Addressed   |
| Privilege escalation inside container                        | ✅ Addressed   |
| Typosquatted packages (`nxdebug` vs `nx-debug`)              | 🔜 Coming soon |
| Poisoned dependencies in lockfiles                           | 🔜 Coming soon |
| Compromised MCP tool descriptors                             | 🔜 Coming soon |
| Malicious extensions and dependencies                        | 🔜 Coming soon |
| Arbitrary code execution via generated code                  | 🔜 Coming soon |
| Unsafe deserialization (pickle, eval)                        | 🔜 Coming soon |

### Governance By Default

Sandboxes provide no visibility into what agents are doing. You can't prove what an agent did, enforce policies on tool usage, or require human approval for destructive actions. Polis makes every action auditable and controllable.

| Threat                                                       | Status         |
| ------------------------------------------------------------ | -------------- |
| No visibility into agent actions                             | ✅ Addressed   |
| Cannot prove what agent did/didn't do                        | ✅ Addressed   |
| Human approval bypassed                                      | ✅ Addressed   |
| Over-privileged tool access                                  | ✅ Addressed   |
| Unlimited API/tool usage                                     | ✅ Addressed   |
| Tool chaining for exfiltration (DB read → HTTP POST)         | 🔜 Coming soon |
| Shell injection via reflected prompts                        | 🔜 Coming soon |
| Indirect prompt injection via fetched content                | 🔜 Coming soon |
| Goal drift via manipulated context                           | 🔜 Coming soon |
| Hidden instructions in documents/emails                      | 🔜 Coming soon |
| Configuration drift undetected                               | 🔜 Coming soon |

### Agent Development Environment

A sandbox is restrictive, not productive. Developers need full tools, not a locked-down shell. Polis provides a complete development environment where agents can build, test, and deploy—while every action passes through the security plane.

| Feature                                                      | Status         |
| ------------------------------------------------------------ | -------------- |
| Full dev environment (VS Code, terminal, Node.js, Python)    | ✅ Available   |
| Works with any MCP agent (Copilot, Claude, Cursor, Aider)    | ✅ Available   |
| Docker-in-Docker (build containers inside workspace)         | ✅ Available   |
| No agent modifications required                              | ✅ Available   |
| Developer mode (relaxed controls, audit preserved)           | ✅ Available   |

## Platform Support

| Platform                       | Status          | Notes                              |
| ------------------------------ | --------------- | ---------------------------------- |
| Debian/Ubuntu + Sysbox         | ✅ Supported    | Recommended for production         |
| WSL2 (Debian/Ubuntu) + Sysbox  | ✅ Supported    | Auto-configured by `polis install` |
| Other Linux distros            | 🔜 Coming soon  | RHEL, Fedora, Arch                 |
| macOS                          | 🔜 Coming soon  |                                    |
| Windows (native)               | 🔜 Coming soon  |                                    |

Run `polis doctor` to check your security posture and system compatibility.

## Command Reference

### Lifecycle

| Command           | Description                                      |
| ----------------- | ------------------------------------------------ |
| `polis install`   | Setup environment, pull images, generate certs   |
| `polis up`        | Start the security plane and workspace           |
| `polis down`      | Stop containers (preserves state)                |
| `polis uninstall` | Remove containers and clean up resources         |
| `polis update`    | Pull new images and recreate containers          |

### Interaction

| Command        | Description                                         |
| -------------- | --------------------------------------------------- |
| `polis shell`  | Open a secure terminal inside the workspace         |
| `polis agents` | Manage persistent, monitored agent sessions         |

### Observability

| Command        | Description                          |
| -------------- | ------------------------------------ |
| `polis status` | View container health                |
| `polis logs`   | Stream security and workspace logs   |
| `polis doctor` | Validate system requirements         |

### `polis agents`

Manage persistent, monitored sessions for your agents. Polis wraps your agent process (e.g., `claude`, `aider`) in a secure TTY session.

```bash
# Start a new agent session
polis agents start claude

# List active sessions
polis agents

# Stop a running session
polis agents stop claude
```

## Configuration

Edit `polis.yaml` to customize:

```yaml
gateway:
  domains:
    allowed:
      - api.openai.com
      - github.com
      - registry.npmjs.org

governance:
  secrets:
    enabled: true
    action: block  # block | redact | alert
  pii:
    enabled: true
    action: redact

toolbox:
  filesystem:
    allowed_paths: [/workspace]
    denied_paths: ["**/.ssh", "**/.aws"]
```

## Security Framework Alignment

Polis is designed against industry security frameworks:

- **OWASP Top 10 for Agentic Applications 2026** — ASI01-ASI10 coverage
- **MITRE ATLAS** — AI-specific threat tactics and techniques
- **NIST AI RMF** — Risk management framework alignment

## License

Apache 2.0 - see [LICENSE](LICENSE)

## Disclaimer

Polis provides defense-in-depth security but is not a silver bullet. Always review agent outputs before deployment, keep secrets out of workspaces, and monitor audit logs.

---

Built with ❤️ in Warsaw 🇵🇱
