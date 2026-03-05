use anyhow::Result;

/// Validates that a public key has a safe format for use in shell commands.
///
/// Accepts `ssh-ed25519` or `ssh-rsa` key types and ensures only safe
/// characters are present to prevent shell injection.
///
/// # Errors
///
/// Returns an error if the key type is not recognised or contains invalid
/// characters.
pub fn validate_pubkey(key: &str) -> Result<()> {
    anyhow::ensure!(
        key.starts_with("ssh-ed25519 ") || key.starts_with("ssh-rsa "),
        "invalid public key format"
    );
    anyhow::ensure!(
        key.chars()
            .all(|c| c.is_ascii_alphanumeric() || " +/=@.-\n".contains(c)),
        "public key contains invalid characters"
    );
    Ok(())
}

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
