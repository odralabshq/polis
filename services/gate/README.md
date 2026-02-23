# Gateway Service

Traffic entry point for the Polis environment, based on g3proxy.

## Features

- Transparent and explicit proxying of agent traffic.
- mTLS termination and user authentication.
- Seamless integration with the Sentinel service via ICAP.

## Configuration

- `config/g3proxy.yaml`: Main proxy configuration.
- `config/g3fcgen.yaml`: Dynamic certificate generation config.
- `scripts/init.sh`: Service initialization and network setup.
