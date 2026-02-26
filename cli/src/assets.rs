//! Embedded assets — the 3 static files compiled into the CLI binary.
//!
//! At compile time, `include_dir!` embeds everything under `.build/assets/`:
//!   - `cloud-init.yaml`          — passed to `multipass launch --cloud-init`
//!   - `image-digests.json`       — used to verify pulled Docker image digests
//!   - `polis-setup.config.tar`   — transferred into the VM and extracted to `/opt/polis`

use std::path::PathBuf;

use anyhow::{Context, Result};
use include_dir::{Dir, include_dir};

/// All 3 embedded assets, compiled in at build time.
static EMBEDDED_ASSETS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../.build/assets");

/// Extract all embedded assets to a temporary directory.
///
/// Returns `(path, guard)` where `path` is the directory containing the
/// extracted files and `guard` is a [`tempfile::TempDir`] that deletes the
/// directory when dropped.
///
/// # Errors
///
/// Returns an error if the temporary directory cannot be created or if any
/// asset fails to extract.
pub fn extract_assets() -> Result<(PathBuf, tempfile::TempDir)> {
    let dir = tempfile::tempdir().context("creating temp dir for assets")?;
    EMBEDDED_ASSETS
        .extract(dir.path())
        .context("extracting embedded assets")?;
    Ok((dir.path().to_path_buf(), dir))
}

/// Return the raw bytes of a single embedded asset without extracting to disk.
///
/// # Errors
///
/// Returns an error if no asset with the given `name` exists.
pub fn get_asset(name: &str) -> Result<&'static [u8]> {
    EMBEDDED_ASSETS
        .get_file(name)
        .map(|f| f.contents())
        .ok_or_else(|| anyhow::anyhow!("embedded asset not found: {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_assets_creates_temp_dir_with_files() {
        let (path, _guard) = extract_assets().expect("extract_assets");
        assert!(path.exists(), "extracted dir should exist while guard is held");
        // All 3 expected files must be present.
        for name in &["cloud-init.yaml", "image-digests.json", "polis-setup.config.tar"] {
            assert!(
                path.join(name).exists(),
                "expected asset {name} to be extracted"
            );
        }
    }

    #[test]
    fn extract_assets_cleanup_on_drop() {
        let path = {
            let (p, _guard) = extract_assets().expect("extract_assets");
            p
        };
        // After the guard is dropped the directory should be gone.
        assert!(
            !path.exists(),
            "temp dir should be deleted after TempDir guard is dropped"
        );
    }

    #[test]
    fn get_asset_returns_bytes_for_known_files() {
        for name in &["cloud-init.yaml", "image-digests.json", "polis-setup.config.tar"] {
            let bytes = get_asset(name).unwrap_or_else(|e| panic!("get_asset({name}): {e}"));
            // Bytes slice must be non-null (zero-length is fine for stub tar).
            let _ = bytes;
        }
    }

    #[test]
    fn get_asset_errors_for_unknown_file() {
        let result = get_asset("does-not-exist.txt");
        assert!(result.is_err(), "should error for unknown asset");
    }
}
