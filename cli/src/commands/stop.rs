//! `polis stop` — stop workspace, preserving all data.

use anyhow::Result;

use crate::multipass::Multipass;
use crate::workspace::vm;

/// Run `polis stop`.
///
/// # Errors
///
/// Returns an error if the workspace cannot be stopped.
pub async fn run(mp: &impl Multipass, quiet: bool) -> Result<()> {
    let state = vm::state(mp).await?;

    match state {
        vm::VmState::NotFound => {
            if !quiet {
                println!();
                println!("No workspace to stop.");
                println!();
                println!("Create one: polis start");
            }
        }
        vm::VmState::Stopped => {
            if !quiet {
                println!();
                println!("Workspace is already stopped.");
                println!();
                println!("Resume: polis start");
            }
        }
        vm::VmState::Running | vm::VmState::Starting => {
            if !quiet {
                println!("Stopping workspace...");
            }
            vm::stop(mp).await?;
            if !quiet {
                println!();
                println!("Workspace stopped. Your data is preserved.");
                println!();
                println!("Resume: polis start");
            }
        }
    }

    Ok(())
}
