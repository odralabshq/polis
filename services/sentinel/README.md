# Sentinel Service

The logic hub of the Polis architecture, based on c-icap.

## Features

- Deep content inspection of HTTP traffic.
- Data Loss Prevention (DLP) using custom C modules.
- Human-in-the-loop (HITL) approval system for sensitive actions.

## Modules

- `modules/dlp/`: Custom C module for PII and secret detection.
- `modules/approval/`: Logic for intercepting and pausing requests for approval.

## Configuration

- `config/c-icap.conf`: Main ICAP server configuration.
- `config/polis_approval.conf`: Approval module settings.
- `config/polis_dlp.conf`: DLP regex and rule definitions.
