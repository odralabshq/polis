//! Image infrastructure — GitHub release resolution.

use anyhow::{Context, Result};

/// Resolved release information from GitHub.
#[derive(Debug)]
pub struct ResolvedRelease {
    /// The release tag (e.g. `v0.3.0`).
    pub tag: String,
}

/// GitHub releases API URL.
pub const GITHUB_RELEASES_URL: &str =
    "https://api.github.com/repos/OdraLabsHQ/polis/releases?per_page=10";

/// Resolve the latest release tag from GitHub releases.
///
/// Used by `polis doctor` for version drift checks.
///
/// # Errors
///
/// Returns an error if the network is unavailable or no release is found.
pub fn resolve_latest_image_url() -> Result<ResolvedRelease> {
    let url =
        std::env::var("POLIS_GITHUB_API_URL").unwrap_or_else(|_| GITHUB_RELEASES_URL.to_string());
    let token = std::env::var("GITHUB_TOKEN").unwrap_or_default();

    let req = ureq::get(&url)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", "polis-cli");
    let req = if token.is_empty() {
        req
    } else {
        req.set("Authorization", &format!("Bearer {token}"))
    };

    let body: serde_json::Value = match req.call() {
        Ok(resp) => serde_json::from_str(&resp.into_string().context("reading response")?)
            .context("parsing response")?,
        Err(ureq::Error::Status(403, _)) => anyhow::bail!(
            "cannot check for updates: rate limited.\n\nTry again in a few minutes, or set GITHUB_TOKEN."
        ),
        Err(ureq::Error::Status(code, _)) => {
            anyhow::bail!("cannot check for updates: HTTP {code}")
        }
        Err(_) => anyhow::bail!(
            "cannot check for updates: no network connection.\n\nFor offline setup: https://polis.dev/docs/offline"
        ),
    };

    let releases = body.as_array().context("invalid response")?;

    for release in releases {
        if let Some(tag) = release["tag_name"].as_str()
            && !tag.is_empty()
        {
            return Ok(ResolvedRelease {
                tag: tag.to_string(),
            });
        }
    }
    anyhow::bail!("no releases found in recent GitHub releases")
}
