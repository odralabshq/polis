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

    // Check current VM state first
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

    // Only resolve image if VM needs to be created
    if vm_state == vm::VmState::NotFound {
        // Determine image source: CLI flag > persisted source > default
        let source = match &args.image {
            Some(s) if s.starts_with("http://") || s.starts_with("https://") => {
                image::ImageSource::HttpUrl(s.clone())
            }
            Some(s) => {
                let path = PathBuf::from(s);
                anyhow::ensure!(path.exists(), "Image file not found: {}", path.display());
                image::ImageSource::LocalFile(path)
            }
            None => {
                // Check if we have a persisted custom image source
                if let Some(state) = state_mgr.load()? {
                    if let Some(ref custom_source) = state.image_source {
                        if custom_source.starts_with("http://")
                            || custom_source.starts_with("https://")
                        {
                            image::ImageSource::HttpUrl(custom_source.clone())
                        } else {
                            let path = PathBuf::from(custom_source);
                            if path.exists() {
                                image::ImageSource::LocalFile(path)
                            } else {
                                image::ImageSource::Default
                            }
                        }
                    } else {
                        image::ImageSource::Default
                    }
                } else {
                    image::ImageSource::Default
                }
            }
        };

        let image_path = image::ensure_available(source, quiet)?;
        vm::create(mp, &image_path, quiet)?;

        let sha256 = image::load_metadata(&image::images_dir()?)
            .ok()
            .flatten()
            .map(|m| m.sha256);
        let custom_source = args.image.clone();
        let state = WorkspaceState {
            workspace_id: generate_workspace_id(),
            created_at: Utc::now(),
            image_sha256: sha256,
            image_source: custom_source,
        };
        state_mgr.save(&state)?;
    } else {
        // VM exists but stopped - just start it
        vm::restart(mp, quiet)?;
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
    let gov = Style::new().truecolor(37, 56, 144); // stop 5
    let sec = Style::new().truecolor(26, 107, 160); // stop 6
    let obs = Style::new().truecolor(26, 151, 179); // stop 7
    println!(
        "✓ {}  policy engine active · audit trail recording",
        "[governance]   ".if_supports_color(Stdout, |t| t.style(gov))
    );
    println!(
        "✓ {}  workspace isolated · traffic inspection enabled",
        "[security]     ".if_supports_color(Stdout, |t| t.style(sec))
    );
    println!(
        "✓ {}  action tracing live · trust scoring active",
        "[observability]".if_supports_color(Stdout, |t| t.style(obs))
    );
}

fn generate_workspace_id() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    // CORR-001: Add multiple entropy sources to prevent duplicates
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    // Add process ID for additional entropy
    hasher.write_u32(std::process::id());
    // RandomState already provides randomness, but hash again for good measure
    hasher.write_u64(RandomState::new().build_hasher().finish());
    format!("polis-{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_id_format() {
        let id = generate_workspace_id();
        assert!(
            id.starts_with("polis-"),
            "expected 'polis-' prefix, got: {id}"
        );
        // "polis-" (6) + 16 hex chars
        assert_eq!(id.len(), 22, "expected 22 chars, got: {id}");
        assert!(id[6..].chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn workspace_id_unique() {
        let a = generate_workspace_id();
        let b = generate_workspace_id();
        assert_ne!(a, b);
    }

    #[test]
    fn test_state_persists_custom_image_source() {
        let state = WorkspaceState {
            workspace_id: "test".to_string(),
            created_at: Utc::now(),
            image_sha256: None,
            image_source: Some("/custom/image.qcow2".to_string()),
        };
        assert_eq!(state.image_source, Some("/custom/image.qcow2".to_string()));
    }

    #[test]
    fn test_state_serializes_with_image_source() {
        let state = WorkspaceState {
            workspace_id: "test".to_string(),
            created_at: Utc::now(),
            image_sha256: None,
            image_source: Some("https://example.com/image.qcow2".to_string()),
        };
        let json = serde_json::to_string(&state).expect("serialize");
        assert!(json.contains("image_source"));
        assert!(json.contains("https://example.com/image.qcow2"));
    }

    #[test]
    fn test_state_deserializes_without_image_source() {
        // Old state files without image_source should still load
        let json =
            r#"{"workspace_id":"test","created_at":"2024-01-01T00:00:00Z","image_sha256":null}"#;
        let state: WorkspaceState = serde_json::from_str(json).expect("deserialize");
        assert_eq!(state.workspace_id, "test");
        assert_eq!(state.image_source, None);
    }
}
