# Polis Developer Guide

Welcome to the Polis project. This guide explains the repository structure, our service-oriented architecture, and the patterns we use for development and testing.

## Repository Philosophy

Polis is organized as a modular monorepo. We prioritize:

- **Service Locality**: Keep code, configuration, scripts, and tests for a specific service together.
- **Infrastructure as Code**: All environment setup, networking, and security policies are defined in code.
- **Security-First**: Minimal capabilities, non-root execution, and strict seccomp profiles for all services.

## High-Level Structure

```text
.
├── agents/             # Higher-level agent configurations (e.g., OpenClaw)
├── lib/                # Shared libraries (Shell, Rust, etc.)
├── services/           # Core system services (Gateway, ICAP, Resolver, etc.)
│   └── <service>/      # Standard service directory structure
├── tests/              # Global tests and helpers
│   ├── e2e/            # Full system flow verification
│   ├── helpers/        # Shared BATS helpers (common.bash)
│   └── integration/    # Global/Cross-service integration tests
└── tools/              # Global development and automation tools
```

## Service Anatomy

Every service in `services/` follows a standard structure to ensure consistency across the project:

```text
services/<name>/
├── config/             # Service-specific configuration files
├── scripts/            # Initialization, health checks, and lifecycle scripts
├── tests/
│   ├── unit/           # Fast tests for scripts and logic (no containers)
│   └── integration/    # Tests verifying containerized service behavior
├── Dockerfile          # Service container definition
└── README.md           # Service-specific documentation
```

## Development Workflows

### 0. Customizing VM Resources (CPU/Memory)

By default, the development VM is created with 4 CPUs and 16GB of RAM. You can change these resources without losing your data by using the `multipass set` command while the VM is stopped.

**Instructions:**

1. **Stop the VM**:

   ```bash
   multipass stop polis-dev
   ```

2. **Set the new resources**:

   ```bash
   # Set CPUs (e.g., to 8)
   multipass set local.polis-dev.cpus=8

   # Set Memory (e.g., to 16GB)
   multipass set local.polis-dev.memory=16G
   ```

3. **Start the VM**:

   ```bash
   multipass start polis-dev
   ```

*Note: You can verify the changes by running `multipass info polis-dev` after starting.*

### 1. Adding a New Service

1. Create a new directory in `services/`.
2. Populate the standard directories (`config/`, `scripts/`, `tests/`).
3. Create a `Dockerfile` and a `README.md`.
4. Add the service definition to the root `docker-compose.yml`.
5. Add unit and integration tests in the service's `tests/` folder.
6. Register the service in the test runner's health check list if applicable.

### 2. Standard Service Lifecycle

Most services use an `init.sh` script (located in `scripts/`) to perform runtime setup (like updating CA certs) before dropping privileges using `setpriv` to run the main application as a non-root user.

## Testing Standard

We use [BATS](https://github.com/bats-core/bats-core) (Bash Automated Testing System) for our test suite.

### Test Categories

- **Unit Tests**: Test individual scripts and logic. They should be fast and not depend on running containers.
- **Integration Tests**: Verify the service's behavior within its container environment, including security hardening, networking, and resilience.
- **E2E Tests**: Verify cross-service traffic flows and user-facing requirements.

### Robust Path Resolution

Always use `tests/helpers/common.bash` for path resolution. It dynamically finds the `PROJECT_ROOT` regardless of the test's nesting depth.

```bash
# Example test setup
setup() {
    # Dynamically find helpers directory relative to PROJECT_ROOT
    load "../../../../tests/helpers/common.bash"
    require_container "$MY_SERVICE_CONTAINER"
}
```

### Running Tests

Use the global test runner at the root of the project:

```bash
./tests/run-tests.sh unit         # Run all unit tests
./tests/run-tests.sh integration  # Run all integration tests
./tests/run-tests.sh all          # Run everything
./tests/run-tests.sh <path>.bats  # Run a specific test file
```

## Best Practices

- **Never hardcode paths**: Use `${PROJECT_ROOT}` or `${TESTS_DIR}`.
- **Modularize Integration Tests**: Split large test files by concern (e.g., `configuration.bats`, `security.bats`, `resilience.bats`).
- **Use Descriptive Test Tags**: Prefix tests with the service or concern name (e.g., `gate-config: ...`).
- **Clean up legacy code**: If you move or rename a component, ensure old directories and unused fallback definitions are removed.
