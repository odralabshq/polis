#!/usr/bin/env bats
# bats file_tags=unit,security
# Valkey ACL structure validation

setup() {
    load "../../lib/test_helper.bash"
    ACL_FILE="$PROJECT_ROOT/secrets/valkey_users.acl"
}

@test "acl: ACL file exists" {
    [ -f "$ACL_FILE" ]
}

@test "acl: defines expected users" {
    for user in mcp-agent mcp-admin log-writer healthcheck dlp-reader; do
        run grep "^user ${user} " "$ACL_FILE"
        assert_success
    done
}
