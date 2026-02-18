//! Integration tests for `logs::run()` (issue 10).
//!
//! RED phase: these tests will fail to compile until the engineer:
//!   1. Adds `ActivityStreamReader` trait to `cli/src/valkey.rs`
//!   2. Changes `run()` to accept `impl ActivityStreamReader` instead of creating `ValkeyClient` internally
//!   3. Exports `pub mod commands;` and `pub mod output;` from `cli/src/lib.rs`
//!
//! See testability recommendations in the issue spec.

#![allow(clippy::expect_used)]

use anyhow::Result;
use chrono::Utc;
use polis_cli::commands::logs::{run, LogsArgs};
use polis_cli::output::OutputContext;
use polis_cli::valkey::ActivityStreamReader;
use polis_common::types::{ActivityEvent, ActivityEventType, InspectionStatus};

// ---------------------------------------------------------------------------
// Test double
// ---------------------------------------------------------------------------

struct FakeStream {
    events: Vec<ActivityEvent>,
    fail: bool,
}

impl ActivityStreamReader for FakeStream {
    async fn get_activity(&self, _count: usize) -> Result<Vec<ActivityEvent>> {
        if self.fail {
            Err(anyhow::anyhow!("connection refused"))
        } else {
            Ok(self.events.clone())
        }
    }

    async fn stream_activity(
        &self,
        _last_id: &str,
        _timeout_ms: u64,
    ) -> Result<Vec<(String, ActivityEvent)>> {
        Ok(vec![])
    }
}

fn make_event(event_type: ActivityEventType, status: InspectionStatus) -> ActivityEvent {
    ActivityEvent {
        ts: Utc::now(),
        event_type,
        dest: None,
        method: None,
        path: None,
        status,
        reason: None,
        detail: None,
    }
}

// ---------------------------------------------------------------------------
// run() — happy path
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_run_empty_stream_returns_ok() {
    let ctx = OutputContext::new(true, false);
    let reader = FakeStream { events: vec![], fail: false };
    let result = run(&ctx, &reader, LogsArgs { follow: false, security: false }).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_run_with_mixed_events_no_filter_returns_ok() {
    let ctx = OutputContext::new(true, false);
    let reader = FakeStream {
        events: vec![
            make_event(ActivityEventType::Request, InspectionStatus::Inspected),
            make_event(ActivityEventType::Block, InspectionStatus::Blocked),
        ],
        fail: false,
    };
    let result = run(&ctx, &reader, LogsArgs { follow: false, security: false }).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_run_security_filter_with_no_block_events_returns_ok() {
    let ctx = OutputContext::new(true, false);
    let reader = FakeStream {
        events: vec![make_event(ActivityEventType::Request, InspectionStatus::Inspected)],
        fail: false,
    };
    let result = run(&ctx, &reader, LogsArgs { follow: false, security: true }).await;
    assert!(result.is_ok());
}

// ---------------------------------------------------------------------------
// run() — error path
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_run_reader_error_returns_err() {
    let ctx = OutputContext::new(true, false);
    let reader = FakeStream { events: vec![], fail: true };
    let result = run(&ctx, &reader, LogsArgs { follow: false, security: false }).await;
    assert!(result.is_err());
}
