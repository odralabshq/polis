//! `polis start` — start workspace (download and create if needed).

use std::path::PathBuf;

use anyhow::Result;
use chrono::Utc;
use clap::Args;

use crate::multipass::Multipass;
use crate::state::{StateManager, WorkspaceState};
use crate::workspace::{health, image, vm};

/// Arguments for the start command.
#[derive(Args)]
pub struct StartArgs {
    /// Use custom image instead of cached/downloaded
    #[arg(long)]
    pub image: Option<String>,
}

/// Run `polis start`.
///
/// # Errors
///
/// Returns an error if image acquisition, VM creation, or health check fails.
pub fn run(args: &StartArgs, mp: &impl Multipass, quiet: bool) -> Result<()> {
    let state_mgr = StateManager::new()?;

    // Determine image source
    let source = match &args.image {
        Some(s) if s.starts_with("http://") || s.starts_with("https://") => {
            image::ImageSource::HttpUrl(s.clone())
        }
        Some(s) => {
            let path = PathBuf::from(s);
            anyhow::ensure!(path.exists(), "Image file not found: {}", path.display());
            image::ImageSource::LocalFile(path)
        }
        None => image::ImageSource::Default,
    };

    // Ensure image is available
    let image_path = image::ensure_available(source, quiet)?;

    // Check current VM state
    let vm_state = vm::state(mp)?;

    if vm_state == vm::VmState::Running {
        if !quiet {
            println!();
            println!("Workspace is running.");
            println!();
            print_guarantees();
            println!();
            println!("Connect: polis connect");
            println!("Status:  polis status");
        }
        return Ok(());
    }

    // Ensure VM is running
    vm::ensure_running(mp, &image_path, quiet)?;

    // Save state if this is a new workspace
    if vm_state == vm::VmState::NotFound {
        let sha256 = image::load_metadata(&image::images_dir()?).ok().flatten().map(|m| m.sha256);
        let state = WorkspaceState {
            workspace_id: generate_workspace_id(),
            created_at: Utc::now(),
            image_sha256: sha256,
        };
        state_mgr.save(&state)?;
    }

    // Wait for healthy
    health::wait_ready(mp, quiet)?;

    // Print success
    if !quiet {
        println!();
        print_guarantees();
        println!();
        println!("Workspace ready.");
        println!();
        println!("Connect: polis connect");
        println!("Status:  polis status");
    }

    Ok(())
}

fn print_guarantees() {
    use owo_colors::{OwoColorize, Stream::Stdout, Style};
    let gov  = Style::new().truecolor(37, 56, 144);   // stop 5
    let sec  = Style::new().truecolor(26, 107, 160);  // stop 6
    let obs  = Style::new().truecolor(26, 151, 179);  // stop 7
    println!("✓ {}  policy engine active · audit trail recording",      "[governance]   ".if_supports_color(Stdout, |t| t.style(gov)));
    println!("✓ {}  workspace isolated · traffic inspection enabled",    "[security]     ".if_supports_color(Stdout, |t| t.style(sec)));
    println!("✓ {}  action tracing live · trust scoring active",         "[observability]".if_supports_color(Stdout, |t| t.style(obs)));
}

fn generate_workspace_id() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    format!("polis-{:016x}", hasher.finish())
}
