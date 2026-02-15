#!/usr/bin/env bash
# Core loader — every test file loads this first.
# Sets PROJECT_ROOT, TESTS_DIR, loads bats-support + bats-assert.

export PROJECT_ROOT="$(cd "$(dirname "${BATS_TEST_FILENAME}")" && while [[ ! -f Justfile ]] && [[ "$PWD" != "/" ]]; do cd ..; done; pwd)"
export TESTS_DIR="${PROJECT_ROOT}/tests"
export FIXTURE_DIR="${TESTS_DIR}/lib/fixtures"

load "${TESTS_DIR}/bats/bats-support/load.bash"
load "${TESTS_DIR}/bats/bats-assert/load.bash"
