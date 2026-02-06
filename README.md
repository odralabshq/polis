# Polis

Secure AI workspace with full traffic inspection. Run AI agents in an isolated environment where all network traffic is monitored and scanned for malware.

## Quick Start

```bash
# Clone the repository
git clone https://github.com/OdraLabsHQ/polis-core.git
cd polis-core

# Initialize (installs dependencies, builds containers, starts services)
./tools/polis.sh init

# After init completes, open the Control UI
# http://localhost:18789
```

That's it! The `init` command handles everything:
- ✅ Checks Docker version compatibility
- ✅ Installs Sysbox runtime (for secure containers)
- ✅ Generates TLS certificates
- ✅ Builds/pulls container images
- ✅ Starts all services
- ✅ Offers to pair your first device

## Requirements

- **Linux** (native or WSL2 on Windows)
- **Docker** 20.10.x - 25.x (version 27+ has known issues with Sysbox)
- **4GB+ RAM** recommended

### Docker Version Note

If you're on Docker 27+, you may need to downgrade:
```bash
sudo apt-get install docker-ce=5:25.0.5-1~ubuntu.22.04~jammy docker-ce-cli=5:25.0.5-1~ubuntu.22.04~jammy
```

## Configuration

Before running `init`, add your AI provider API key to `.env`:

```bash
# Copy the example config
cp config/openclaw.env.example .env

# Edit and add your API key (at least one required)
nano .env
```

Supported providers:
- `ANTHROPIC_API_KEY` - Claude (recommended)
- `OPENAI_API_KEY` - GPT-4
- `OPENROUTER_API_KEY` - Multiple models via single key

## Commands

### Essential Commands

| Command | Description |
|---------|-------------|
| `./tools/polis.sh init` | First-time setup - does everything |
| `./tools/polis.sh connect` | Pair a new device |
| `./tools/polis.sh openclaw token` | Show access token and URL |
| `./tools/polis.sh status` | Check if services are running |
| `./tools/polis.sh down` | Stop everything |

### OpenClaw Commands

| Command | Description |
|---------|-------------|
| `openclaw token` | Display gateway token and Control UI URL |
| `openclaw status` | Show service health and status |
| `openclaw logs [n]` | View last n log lines (default: 50) |
| `openclaw shell` | Enter the workspace shell |
| `openclaw restart` | Restart the OpenClaw service |
| `openclaw devices` | List all paired devices |
| `openclaw devices approve` | Approve pending device requests |

### Other Commands

| Command | Description |
|---------|-------------|
| `up` | Start containers |
| `down` | Stop and remove containers |
| `logs [service]` | View container logs |
| `shell` | Enter workspace shell |
| `build [--no-cache]` | Rebuild images |

## Device Pairing

After `init` completes, you'll be prompted to pair a device. You can also do this anytime:

```bash
./tools/polis.sh connect
```

This will:
1. Show your gateway token
2. Guide you to open http://localhost:18789
3. Wait for you to request pairing in the UI
4. Automatically approve the device

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        Host Machine                          │
│  ┌─────────────────────────────────────────────────────────┐│
│  │                    Polis Workspace                       ││
│  │  ┌─────────────┐                                        ││
│  │  │  OpenClaw   │ ◄── AI Agent + Control UI              ││
│  │  │  Gateway    │     http://localhost:18789             ││
│  │  └──────┬──────┘                                        ││
│  │         │ All traffic                                   ││
│  │         ▼                                               ││
│  │  ┌─────────────┐     ┌─────────────┐                   ││
│  │  │  G3Proxy    │────►│    ICAP     │                   ││
│  │  │  Gateway    │     │   Scanner   │                   ││
│  │  └──────┬──────┘     └──────┬──────┘                   ││
│  │         │                   │                           ││
│  │         │            ┌──────┴──────┐                   ││
│  │         │            │   ClamAV    │                   ││
│  │         │            │  Antivirus  │                   ││
│  │         │            └─────────────┘                   ││
│  └─────────┼───────────────────────────────────────────────┘│
│            │                                                 │
│            ▼                                                 │
│       Internet                                               │
└─────────────────────────────────────────────────────────────┘
```

**Key Components:**
- **OpenClaw** - AI agent gateway with web Control UI
- **G3Proxy** - TLS-intercepting proxy for traffic inspection
- **ICAP/ClamAV** - Malware scanning for all downloads
- **Sysbox** - Secure container runtime (Docker-in-Docker)

## Troubleshooting

### "Sysbox setup failed"

Sysbox requires a compatible kernel. On WSL2:
```bash
# Ensure you're using WSL2, not WSL1
wsl --set-version Ubuntu 2
```

### "Docker version may have compatibility issues"

Downgrade Docker to a compatible version:
```bash
sudo apt-get install docker-ce=5:25.0.5-1~ubuntu.22.04~jammy
```

### OpenClaw not starting

Check the logs:
```bash
./tools/polis.sh openclaw logs 100
```

Common issues:
- No API key configured in `.env`
- Port 18789 already in use

### Reset everything

```bash
./tools/polis.sh down
rm -rf certs/ca/*.key certs/ca/*.pem
./tools/polis.sh init
```

## Development

Build from source instead of pulling images:
```bash
./tools/polis.sh init --local
```

Run tests:
```bash
./tools/polis.sh test
```

## License

[Add your license here]

## Links

- [OpenClaw Documentation](https://docs.openclaw.ai)
- [G3Proxy](https://github.com/bytedance/g3)
- [Sysbox](https://github.com/nestybox/sysbox)
