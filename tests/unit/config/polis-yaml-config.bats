#!/usr/bin/env bats
# bats file_tags=unit,config
# polis.yaml configuration validation

setup() {
    load "../../lib/test_helper.bash"
    CONFIG="$PROJECT_ROOT/config/polis.yaml"
}

@test "polis yaml config: file exists" {
    [ -f "$CONFIG" ]
}

@test "polis yaml config: has security_level field" {
    run grep "^security_level:" "$CONFIG"
    assert_success
}

@test "polis yaml config: valid YAML syntax" {
    # Basic structural check — no tabs, proper indentation
    run grep -P "^\t" "$CONFIG"
    assert_failure
}

@test "polis yaml config: no hardcoded secrets" {
    # Ensure no actual password/secret/key values in non-comment lines
    run bash -c "grep -vE '^\s*#' '$CONFIG' | grep -iE '(password|secret_key|api_key):\s+\S'"
    assert_failure
}
