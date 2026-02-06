# OpenClaw Agent

AI coding agent with gateway UI and device pairing, running inside the Polis workspace.

## Files

| File | Purpose |
|---|---|
| `agent.conf` | Shell-sourceable metadata read by the CLI |
| `install.sh` | Build-time script (runs inside container to install OpenClaw) |
| `compose.override.yaml` | Compose merge file for ports, volumes, healthcheck, resources |
| `commands.sh` | Agent-specific CLI commands: `token`, `devices`, `onboard`, `cli` |
| `scripts/init.sh` | Container init script — generates gateway token and config |
| `scripts/health.sh` | Container health check script |
| `config/openclaw.service` | Systemd unit for the OpenClaw gateway |
| `config/env.example` | Example environment variables |

## Quick Start

```bash
./polis init --agent=openclaw --local
./polis openclaw token
```

Control UI: `http://localhost:18789`
