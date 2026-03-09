//! Infrastructure implementation of the `ConfigStore` port.

use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::application::ports::ConfigStore;
use crate::domain::config::PolisConfig;
use crate::infra::secure_fs::SecureFs;

/// Production implementation of `ConfigStore` that uses a YAML file on disk.
pub struct YamlConfigStore {
    explicit_path: Option<PathBuf>,
}

impl YamlConfigStore {
    /// Create a new `YamlConfigStore` using the default path resolution (env var / home dir).
    #[must_use]
    pub fn new() -> Self {
        Self {
            explicit_path: None,
        }
    }

    /// Create a `YamlConfigStore` with an explicit config file path.
    ///
    /// When set, `path()` returns this value directly, bypassing env var and
    /// home-directory resolution. Intended for use in tests.
    #[must_use]
    pub fn with_path(path: PathBuf) -> Self {
        Self {
            explicit_path: Some(path),
        }
    }
}

impl Default for YamlConfigStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigStore for YamlConfigStore {
    /// # Errors
    ///
    /// This function will return an error if the underlying operations fail.
    fn load(&self) -> Result<PolisConfig> {
        let path = self.path()?;
        if !path.exists() {
            return Ok(PolisConfig::default());
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        serde_yaml_ng::from_str(&content)
            .with_context(|| format!("cannot parse {}", path.display()))
    }

    /// # Errors
    ///
    /// This function will return an error if the underlying operations fail.
    fn save(&self, config: &PolisConfig) -> Result<()> {
        let path = self.path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
        let content = serde_yaml_ng::to_string(config).context("cannot serialize config")?;
        std::fs::write(&path, content)
            .with_context(|| format!("cannot write {}", path.display()))?;

        SecureFs::set_permissions(&path, 0o600)?;
        Ok(())
    }

    /// # Errors
    ///
    /// This function will return an error if the underlying operations fail.
    fn path(&self) -> Result<PathBuf> {
        if let Some(ref p) = self.explicit_path {
            return Ok(p.clone());
        }
        if let Ok(val) = std::env::var("POLIS_CONFIG") {
            return Ok(PathBuf::from(val));
        }
        let polis_dir = crate::infra::polis_dir::PolisDir::new()?;
        Ok(polis_dir.config_path())
    }
}
