#!/usr/bin/env bash
# Pre-configured curl/nc mock for unit tests.

load "$(dirname "${BASH_SOURCE[0]}")/mock_helper.bash"

setup_network_mock() {
    mock_command "curl" "" 0
    mock_command "nc" "" 0
    mock_command "openssl" "" 0
}

teardown_network_mock() {
    mock_reset
    unset -f curl nc openssl 2>/dev/null || true
}
