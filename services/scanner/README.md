# Scanner Service

Malware and virus scanning engine, based on ClamAV.

## Features

- Real-time scanning of intercepted traffic.
- Automatic definition updates via freshclam.
- Standard ICAP interface for integration with the Sentinel service.

## Configuration

- `config/clamd.conf`: ClamAV daemon configuration.
- `config/freshclam.conf`: Auto-update configuration.
- `config/squidclamav.conf`: ICAP scanner bridge settings.
