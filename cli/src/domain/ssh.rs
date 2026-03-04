use anyhow::Result;

/// Validates that `key` is an ed25519 public key with non-empty key material.
///
/// Accepts the raw public key format: `ssh-ed25519 <base64-material>`.
///
/// # Errors
///
/// Returns an error if the key does not start with `ssh-ed25519 ` or has no
/// key material after the prefix.
pub fn validate_host_key(key: &str) -> Result<()> {
    let material = key
        .strip_prefix("ssh-ed25519 ")
        .ok_or_else(|| anyhow::anyhow!("host key must be an ed25519 key (got: {key:?})"))?;
    anyhow::ensure!(!material.trim().is_empty(), "host key has no key material");
    Ok(())
}
