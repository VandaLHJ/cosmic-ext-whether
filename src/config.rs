use cosmic::cosmic_config;
use cosmic::cosmic_config::{cosmic_config_derive::CosmicConfigEntry, Config, CosmicConfigEntry};

use crate::types::SavedLocation;

pub const APP_ID: &str = "com.github.nwxnw.cosmic-ext-whether";

#[derive(Debug, Clone, PartialEq, Eq, CosmicConfigEntry)]
#[version = 3]
pub struct WhetherConfig {
    pub use_fahrenheit: bool,
    pub locations: Vec<SavedLocation>,
    pub active_location_index: usize,
    pub refresh_interval_minutes: u32,
}

impl Default for WhetherConfig {
    fn default() -> Self {
        Self {
            use_fahrenheit: true,
            locations: vec![],
            active_location_index: 0,
            refresh_interval_minutes: 30,
        }
    }
}

impl WhetherConfig {
    pub fn active_location(&self) -> Option<&SavedLocation> {
        self.locations.get(self.active_location_index)
    }
}

pub fn load_config() -> (WhetherConfig, Option<Config>) {
    // Try loading v3 config
    match Config::new(APP_ID, WhetherConfig::VERSION) {
        Ok(config) => match WhetherConfig::get_entry(&config) {
            Ok(cfg) => return (cfg, Some(config)),
            Err((_, cfg)) => {
                // Partial load succeeded — new fields get defaults
                let _ = cfg.write_entry(&config);
                return (cfg, Some(config));
            }
        },
        Err(_) => {}
    }

    // v3 config doesn't exist — try migrating from v2
    // SavedLocation's new `source` field has #[serde(default)] so v2 data
    // deserializes correctly (all locations default to NWS).
    if let Ok(v2_handle) = Config::new(APP_ID, 2) {
        if let Ok(cfg) = WhetherConfig::get_entry(&v2_handle) {
            // Write migrated config to v3
            if let Ok(v3_handle) = Config::new(APP_ID, WhetherConfig::VERSION) {
                let _ = cfg.write_entry(&v3_handle);
                return (cfg, Some(v3_handle));
            }
            return (cfg, None);
        }
    }

    (WhetherConfig::default(), None)
}

pub fn save_config(config_handle: &Option<Config>, cfg: &WhetherConfig) {
    if let Some(handle) = config_handle {
        let _ = cfg.write_entry(handle);
    }
}
