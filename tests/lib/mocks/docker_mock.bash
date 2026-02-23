#!/usr/bin/env bash
# Pre-configured docker mock for unit tests.
# Loads mock_helper.bash and sets up docker with sensible defaults.

load "$(dirname "${BASH_SOURCE[0]}")/mock_helper.bash"

setup_docker_mock() {
    mock_command "docker" "" 0
}

teardown_docker_mock() {
    mock_reset
    unset -f docker 2>/dev/null || true
}
