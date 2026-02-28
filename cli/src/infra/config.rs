//! Infrastructure implementation of the `ConfigStore` port.

use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::application::ports::ConfigStore;
use crate::domain::config::PolisConfig;

/// Production implementation of `ConfigStore` that uses a YAML file on disk.
pub struct YamlConfigStore;

impl ConfigStore for YamlConfigStore {
    fn load(&self) -> Result<PolisConfig> {
        let path = self.path()?;
        if !path.exists() {
            return Ok(PolisConfig::default());
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        serde_yaml::from_str(&content).with_context(|| format!("cannot parse {}", path.display()))
    }

    fn save(&self, config: &PolisConfig) -> Result<()> {
        let path = self.path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
        let content = serde_yaml::to_string(config).context("cannot serialize config")?;
        std::fs::write(&path, content)
            .with_context(|| format!("cannot write {}", path.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .with_context(|| format!("cannot set permissions on {}", path.display()))?;
        }
        Ok(())
    }

    fn path(&self) -> Result<PathBuf> {
        if let Ok(val) = std::env::var("POLIS_CONFIG") {
            return Ok(PathBuf::from(val));
        }
        let home =
            dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;
        Ok(home.join(".polis").join("config.yaml"))
    }
}
