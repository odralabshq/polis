//! Logs command — display and stream workspace activity.

use anyhow::Result;
use clap::Args;
use owo_colors::OwoColorize as _;
use polis_common::types::{ActivityEvent, ActivityEventType, BlockReason, InspectionStatus};

use crate::output::OutputContext;
use crate::valkey::ActivityStreamReader;

/// Arguments for the logs command.
#[derive(Args)]
pub struct LogsArgs {
    /// Follow log output (like tail -f)
    #[arg(short, long)]
    pub follow: bool,

    /// Show only security events (blocks, violations)
    #[arg(long)]
    pub security: bool,
}

/// Run the logs command.
///
/// # Errors
///
/// Returns an error if the activity stream cannot be read.
pub async fn run(
    ctx: &OutputContext,
    reader: &impl ActivityStreamReader,
    args: LogsArgs,
) -> Result<()> {
    if args.follow {
        stream_logs(ctx, reader, args.security).await
    } else {
        show_recent_logs(ctx, reader, args.security).await
    }
}

async fn show_recent_logs(
    ctx: &OutputContext,
    reader: &impl ActivityStreamReader,
    security_only: bool,
) -> Result<()> {
    let events = reader.get_activity(100).await?;

    if events.is_empty() {
        println!("  No activity yet");
        return Ok(());
    }

    for event in events.iter().rev() {
        if security_only && !is_security_event(event) {
            continue;
        }
        print_event(ctx, event);
    }

    Ok(())
}

async fn stream_logs(
    ctx: &OutputContext,
    reader: &impl ActivityStreamReader,
    security_only: bool,
) -> Result<()> {
    let mut last_id = "$".to_string();

    let recent = reader.get_activity(20).await?;
    for event in recent.iter().rev() {
        if security_only && !is_security_event(event) {
            continue;
        }
        print_event(ctx, event);
    }

    loop {
        let events = reader.stream_activity(&last_id, 5000).await?;
        for (id, event) in events {
            if security_only && !is_security_event(&event) {
                continue;
            }
            print_event(ctx, &event);
            last_id = id;
        }
    }
}

fn is_security_event(event: &ActivityEvent) -> bool {
    matches!(event.event_type, ActivityEventType::Block)
        || event.status == InspectionStatus::Blocked
}

fn print_event(ctx: &OutputContext, event: &ActivityEvent) {
    let time = event.ts.format("%H:%M:%S").to_string();

    match event.event_type {
        ActivityEventType::Request => {
            let dest = event.dest.as_deref().unwrap_or("unknown");
            let method = event.method.as_deref().unwrap_or("???");
            let path = event.path.as_deref().unwrap_or("/");
            println!(
                "  [{}] {} {} {} {}",
                time.style(ctx.styles.dim),
                "→".style(ctx.styles.info),
                dest,
                method,
                path
            );
        }
        ActivityEventType::Response => {
            let dest = event.dest.as_deref().unwrap_or("unknown");
            let detail = event.detail.as_deref().unwrap_or("inspected");
            println!(
                "  [{}] {} {} ({})",
                time.style(ctx.styles.dim),
                "←".style(ctx.styles.success),
                dest,
                detail
            );
        }
        ActivityEventType::Scan => {
            let dest = event.dest.as_deref().unwrap_or("unknown");
            println!(
                "  [{}] {} {} scanned (clean)",
                time.style(ctx.styles.dim),
                "✓".style(ctx.styles.success),
                dest
            );
        }
        ActivityEventType::Block => {
            print_block_event(ctx, &time, event);
        }
        ActivityEventType::Agent => {
            let detail = event.detail.as_deref().unwrap_or("lifecycle event");
            println!(
                "  [{}] {} Agent: {}",
                time.style(ctx.styles.dim),
                "●".style(ctx.styles.info),
                detail
            );
        }
    }
}

fn print_block_event(ctx: &OutputContext, time: &str, event: &ActivityEvent) {
    let reason = event.reason.as_ref().map_or("policy violation", |r| match r {
        BlockReason::CredentialDetected => "credential detected",
        BlockReason::MalwareDomain => "malware domain",
        BlockReason::UrlBlocked => "url blocked",
        BlockReason::FileInfected => "file infected",
    });

    println!(
        "  [{}] {} Blocked: {}",
        time.style(ctx.styles.dim),
        "⚠".style(ctx.styles.warning),
        reason
    );

    if let Some(dest) = &event.dest {
        println!("             Destination: {dest}");
    }

    if let Some(detail) = &event.detail {
        println!("             Action: {detail}");
    }

    println!();
}

#[cfg(test)]
mod tests {
    use super::is_security_event;
    use chrono::Utc;
    use polis_common::types::{ActivityEvent, ActivityEventType, InspectionStatus};

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

    #[test]
    fn test_is_security_event_block_type_returns_true() {
        let event = make_event(ActivityEventType::Block, InspectionStatus::Blocked);
        assert!(is_security_event(&event));
    }

    #[test]
    fn test_is_security_event_blocked_status_non_block_type_returns_true() {
        // InspectionStatus::Blocked on a non-Block event type is still a security event
        let event = make_event(ActivityEventType::Response, InspectionStatus::Blocked);
        assert!(is_security_event(&event));
    }

    #[test]
    fn test_is_security_event_request_inspected_returns_false() {
        let event = make_event(ActivityEventType::Request, InspectionStatus::Inspected);
        assert!(!is_security_event(&event));
    }

    #[test]
    fn test_is_security_event_scan_clean_returns_false() {
        let event = make_event(ActivityEventType::Scan, InspectionStatus::Clean);
        assert!(!is_security_event(&event));
    }

    #[test]
    fn test_is_security_event_agent_inspected_returns_false() {
        let event = make_event(ActivityEventType::Agent, InspectionStatus::Inspected);
        assert!(!is_security_event(&event));
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use anyhow::Result;
    use chrono::Utc;
    use polis_common::types::{ActivityEvent, ActivityEventType, InspectionStatus};
    use proptest::prelude::*;

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

    fn arb_event_type() -> impl Strategy<Value = ActivityEventType> {
        prop_oneof![
            Just(ActivityEventType::Request),
            Just(ActivityEventType::Response),
            Just(ActivityEventType::Scan),
            Just(ActivityEventType::Block),
            Just(ActivityEventType::Agent),
        ]
    }

    fn arb_status() -> impl Strategy<Value = InspectionStatus> {
        prop_oneof![
            Just(InspectionStatus::Inspected),
            Just(InspectionStatus::Clean),
            Just(InspectionStatus::Blocked),
        ]
    }

    fn arb_event() -> impl Strategy<Value = ActivityEvent> {
        (arb_event_type(), arb_status()).prop_map(|(event_type, status)| ActivityEvent {
            ts: Utc::now(),
            event_type,
            dest: None,
            method: None,
            path: None,
            status,
            reason: None,
            detail: None,
        })
    }

    proptest! {
        /// Block event type is always a security event regardless of status
        #[test]
        fn prop_is_security_event_block_type_always_true(status in arb_status()) {
            let event = ActivityEvent {
                ts: Utc::now(),
                event_type: ActivityEventType::Block,
                dest: None, method: None, path: None,
                status, reason: None, detail: None,
            };
            prop_assert!(is_security_event(&event));
        }

        /// Blocked status is always a security event regardless of event type
        #[test]
        fn prop_is_security_event_blocked_status_always_true(event_type in arb_event_type()) {
            let event = ActivityEvent {
                ts: Utc::now(),
                event_type,
                dest: None, method: None, path: None,
                status: InspectionStatus::Blocked,
                reason: None, detail: None,
            };
            prop_assert!(is_security_event(&event));
        }

        /// Non-Block type with non-Blocked status is never a security event
        #[test]
        fn prop_is_security_event_non_block_non_blocked_always_false(
            event_type in arb_event_type()
                .prop_filter("not Block", |t| !matches!(t, ActivityEventType::Block)),
            status in arb_status()
                .prop_filter("not Blocked", |s| !matches!(s, InspectionStatus::Blocked)),
        ) {
            let event = ActivityEvent {
                ts: Utc::now(),
                event_type,
                dest: None, method: None, path: None,
                status, reason: None, detail: None,
            };
            prop_assert!(!is_security_event(&event));
        }

        /// run() with any non-failing reader always returns Ok
        #[test]
        #[allow(clippy::expect_used)]
        fn prop_run_non_failing_reader_returns_ok(
            events in prop::collection::vec(arb_event(), 0..20)
        ) {
            let rt = tokio::runtime::Runtime::new().expect("runtime");
            let ctx = crate::output::OutputContext::new(true, false);
            let reader = FakeStream { events, fail: false };
            let result = rt.block_on(run(&ctx, &reader, LogsArgs { follow: false, security: false }));
            prop_assert!(result.is_ok());
        }

        /// run() with security filter and any events always returns Ok
        #[test]
        #[allow(clippy::expect_used)]
        fn prop_run_security_filter_always_returns_ok(
            events in prop::collection::vec(arb_event(), 0..20)
        ) {
            let rt = tokio::runtime::Runtime::new().expect("runtime");
            let ctx = crate::output::OutputContext::new(true, false);
            let reader = FakeStream { events, fail: false };
            let result = rt.block_on(run(&ctx, &reader, LogsArgs { follow: false, security: true }));
            prop_assert!(result.is_ok());
        }

        /// run() with a failing reader always returns Err
        #[test]
        #[allow(clippy::expect_used)]
        fn prop_run_failing_reader_always_returns_err(_seed in 0u32..100) {
            let rt = tokio::runtime::Runtime::new().expect("runtime");
            let ctx = crate::output::OutputContext::new(true, false);
            let reader = FakeStream { events: vec![], fail: true };
            let result = rt.block_on(run(&ctx, &reader, LogsArgs { follow: false, security: false }));
            prop_assert!(result.is_err());
        }
    }
}
