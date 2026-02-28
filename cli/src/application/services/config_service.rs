//! Application service — configuration use-cases.

use crate::application::ports::ConfigStore;
use crate::domain::config::PolisConfig;
use anyhow::Result;

/// Load configuration.
pub fn load_config(store: &impl ConfigStore) -> Result<PolisConfig> {
    store.load()
}

/// Save configuration.
pub fn save_config(store: &impl ConfigStore, config: &PolisConfig) -> Result<()> {
    store.save(config)
}
