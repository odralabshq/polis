//! Integration tests for the container update flow (issue 08).
//!
//! Tests cover the public struct API and pure helper logic.
//! Tests that require `update_containers()` directly are deferred until
//! `VmChecker` trait injection is implemented (see testability recommendation
//! in `update.rs`), as that function calls `multipass` which is an external
//! dependency that must not be invoked from tests.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use polis_cli::commands::update::{ContainerUpdate, RollbackInfo};

// ── Struct API surface ────────────────────────────────────────────────────────

#[test]
fn test_container_update_struct_fields_are_accessible() {
    let u = ContainerUpdate {
        service_key: "gate".to_string(),
        image_name: "polis-gate-oss".to_string(),
        current_version: "v0.3.0".to_string(),
        target_version: "v0.3.1".to_string(),
    };
    assert_eq!(u.service_key, "gate");
    assert_eq!(u.image_name, "polis-gate-oss");
    assert_eq!(u.current_version, "v0.3.0");
    assert_eq!(u.target_version, "v0.3.1");
}

#[test]
fn test_rollback_info_struct_fields_are_accessible() {
    let r = RollbackInfo {
        previous_refs: vec![(
            "gate".to_string(),
            "ghcr.io/odralabshq/polis-gate-oss:v0.3.0".to_string(),
        )],
    };
    assert_eq!(r.previous_refs.len(), 1);
    assert_eq!(r.previous_refs[0].0, "gate");
    assert!(r.previous_refs[0].1.contains("v0.3.0"));
}

#[test]
fn test_container_update_debug_format_contains_fields() {
    let u = ContainerUpdate {
        service_key: "gate".to_string(),
        image_name: "polis-gate-oss".to_string(),
        current_version: "v0.3.0".to_string(),
        target_version: "v0.3.1".to_string(),
    };
    let debug = format!("{u:?}");
    assert!(debug.contains("gate"));
    assert!(debug.contains("v0.3.1"));
}

#[test]
fn test_rollback_info_debug_format_contains_fields() {
    let r = RollbackInfo {
        previous_refs: vec![("gate".to_string(), "ghcr.io/odralabshq/polis-gate-oss:v0.3.0".to_string())],
    };
    let debug = format!("{r:?}");
    assert!(debug.contains("gate"));
}
