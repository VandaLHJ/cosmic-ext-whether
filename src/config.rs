use cosmic::cosmic_config;
use cosmic::cosmic_config::{cosmic_config_derive::CosmicConfigEntry, Config, CosmicConfigEntry};

use crate::types::SavedLocation;

pub const APP_ID: &str = "com.github.nwxnw.cosmic-ext-whether";

/// Detect whether to default to Fahrenheit based on the user's locale.
///
/// Checks `LC_MEASUREMENT` then `LANG` for a country code.
/// US, Liberia (LR), and Myanmar (MM) use Fahrenheit; everyone else uses Celsius.
/// Falls back to `true` (Fahrenheit) if no locale can be determined.
fn detect_fahrenheit_default() -> bool {
    let locale_str = std::env::var("LC_MEASUREMENT")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("LANG").ok().filter(|s| !s.is_empty()));

    let Some(locale) = locale_str else {
        return true;
    };

    // Extract country code from e.g. "en_US.UTF-8" or "en_US"
    // Find the '_' separator, then take the next 2 chars as country code
    let country = locale
        .find('_')
        .and_then(|pos| locale.get(pos + 1..pos + 3))
        .map(|c| c.to_uppercase());

    match country.as_deref() {
        Some("US") | Some("LR") | Some("MM") => true,
        Some(_) => false,
        None => true, // Can't parse → preserve existing default
    }
}

#[derive(Debug, Clone, PartialEq, Eq, CosmicConfigEntry)]
#[version = 4]
pub struct WhetherConfig {
    pub use_fahrenheit: bool,
    pub locations: Vec<SavedLocation>,
    pub active_location_index: usize,
    pub refresh_interval_minutes: u32,
}

impl Default for WhetherConfig {
    fn default() -> Self {
        Self {
            use_fahrenheit: detect_fahrenheit_default(),
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
    // Try loading v4 config
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

    // v4 config doesn't exist — try migrating from v3
    // SavedLocation's new `country_code` field has #[serde(default)] so v3 data
    // deserializes correctly (all locations get country_code: None).
    if let Ok(v3_handle) = Config::new(APP_ID, 3) {
        if let Ok(cfg) = WhetherConfig::get_entry(&v3_handle) {
            if let Ok(v4_handle) = Config::new(APP_ID, WhetherConfig::VERSION) {
                let _ = cfg.write_entry(&v4_handle);
                return (cfg, Some(v4_handle));
            }
            return (cfg, None);
        }
    }

    // Try migrating from v2
    if let Ok(v2_handle) = Config::new(APP_ID, 2) {
        if let Ok(cfg) = WhetherConfig::get_entry(&v2_handle) {
            if let Ok(v4_handle) = Config::new(APP_ID, WhetherConfig::VERSION) {
                let _ = cfg.write_entry(&v4_handle);
                return (cfg, Some(v4_handle));
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
