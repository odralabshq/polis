# State Service

Persistent data store for the Polis environment, based on Valkey.

## Features

- Redis-compatible API for fast state management.
- mTLS-secured communication with clients.
- ACL-based user management for fine-grained access control.

## Configuration

- `config/valkey.conf`: Server configuration.
- `scripts/generate-certs.sh`: Toolbox for generating mTLS certificates.
- `scripts/generate-secrets.sh`: Helper for ACL and password generation.
