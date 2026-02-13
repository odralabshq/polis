# Workspace Service

Hardened development and execution environment based on Sysbox.

## Features

- Level-2 container isolation for safe agent execution.
- Systemd-enabled environment for running complex workloads.
- Pre-configured with essential development tools and Polis integrations.

## Configuration

- `config/polis-init.service`: Systemd service for workspace initialization.
- `scripts/workspace-init.sh`: Bootstrap script for the environment.
- `Dockerfile`: Multi-stage build for the base workspace image.
