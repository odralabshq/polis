//! Integration tests for `ValkeyClient::get_activity` and `stream_activity` (issue 09).
//!
//! RED phase: these tests reference methods that do not yet exist on `ValkeyClient`.
//! They will fail to compile until `get_activity` and `stream_activity` are implemented.
//!
//! Tests that require a live Valkey instance are marked `#[ignore]`.
//! Run them with: `cargo test -p polis-cli --test valkey_activity -- --ignored`
//!
//! ⚠️  Testability note: `get_activity` and `stream_activity` make direct network calls
//! with no trait abstraction. Tests below are therefore `#[ignore]`d integration tests.
//! Recommendation: extract an `ActivityStreamReader` trait (see bottom of file) so the
//! caller can be tested with a mock — hand off to the Senior Rust Engineer.

#![allow(clippy::expect_used)]

// `polis-cli` is a binary crate; re-export the public surface via `lib.rs` or
// use `#[path]` to reach the module directly.  For now the tests reference the
// public types that *will* be exported once the crate exposes a lib target.
//
// Until then this file acts as a compile-time RED signal: it will fail to build
// because `polis_cli` is not yet a library crate.  The engineer must either:
//   (a) add `[lib]` to cli/Cargo.toml and re-export `ValkeyClient`, or
//   (b) move these tests into `cli/src/valkey.rs` as async `#[cfg(test)]` tests.

use polis_cli::valkey::{ValkeyClient, ValkeyConfig};

// ---------------------------------------------------------------------------
// get_activity — requires live Valkey
// ---------------------------------------------------------------------------

/// An empty stream returns an empty Vec, not an error.
#[tokio::test]
#[ignore = "requires live Valkey at 127.0.0.1:6379"]
async fn test_get_activity_empty_stream_returns_empty_vec() {
    let client = ValkeyClient::new(&ValkeyConfig::default()).expect("create client");
    let events = client.get_activity(10).await.expect("get_activity should not error on empty stream");
    assert!(events.is_empty(), "no events expected on a fresh stream");
}

/// Requesting zero events returns an empty Vec.
#[tokio::test]
#[ignore = "requires live Valkey at 127.0.0.1:6379"]
async fn test_get_activity_count_zero_returns_empty_vec() {
    let client = ValkeyClient::new(&ValkeyConfig::default()).expect("create client");
    let events = client.get_activity(0).await.expect("count=0 should succeed");
    assert!(events.is_empty());
}

// ---------------------------------------------------------------------------
// stream_activity — requires live Valkey
// ---------------------------------------------------------------------------

/// A blocking read with a short timeout on an empty stream returns an empty Vec.
#[tokio::test]
#[ignore = "requires live Valkey at 127.0.0.1:6379"]
async fn test_stream_activity_timeout_on_empty_stream_returns_empty_vec() {
    let client = ValkeyClient::new(&ValkeyConfig::default()).expect("create client");
    // Use "$" as last_id to read only new entries; 100 ms timeout.
    let events = client
        .stream_activity("$", 100)
        .await
        .expect("stream_activity should not error on timeout");
    assert!(events.is_empty(), "no new events expected within timeout");
}

// ---------------------------------------------------------------------------
// ⚠️  Testability Recommendation
// ---------------------------------------------------------------------------
//
// `get_activity` and `stream_activity` call Valkey directly with no seam for
// injection.  To enable deterministic unit tests, extract:
//
//   pub trait ActivityStreamReader {
//       async fn get_activity(&self, count: usize) -> Result<Vec<ActivityEvent>>;
//       async fn stream_activity(
//           &self,
//           last_id: &str,
//           timeout_ms: u64,
//       ) -> Result<Vec<(String, ActivityEvent)>>;
//   }
//
// Implement it on `ValkeyClient` and accept `impl ActivityStreamReader` in the
// `logs` command handler.  In tests, provide a `FakeActivityStream` that returns
// canned `ActivityEvent` values.
//
// This is a production code change — hand off to the Senior Rust Engineer.
