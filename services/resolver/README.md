# Resolver Service

DNS entry point for the Polis environment, based on CoreDNS.

## Features

- Provides internal DNS resolution for the local network.
- Implements blocklist filtering for known malicious domains.
- Integrated with the Polis observability stack.

## Configuration

- `config/Corefile`: Primary CoreDNS configuration.
- `Dockerfile`: Multi-stage build for a hardened CoreDNS binary.
