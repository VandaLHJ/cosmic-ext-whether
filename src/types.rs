use serde::{Deserialize, Serialize};

// --- Weather source selection ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WeatherSource {
    #[default]
    Nws,
    OpenMeteo,
}

impl WeatherSource {
    pub fn label(&self) -> &'static str {
        match self {
            WeatherSource::Nws => "NWS",
            WeatherSource::OpenMeteo => "Open-Meteo",
        }
    }

    pub fn toggle(&self) -> Self {
        match self {
            WeatherSource::Nws => WeatherSource::OpenMeteo,
            WeatherSource::OpenMeteo => WeatherSource::Nws,
        }
    }

    /// Returns the list of weather sources valid for a given country code.
    /// `None` (legacy/unknown) is treated as US to preserve migrated v3 locations.
    pub fn available_for(country_code: Option<&str>) -> Vec<WeatherSource> {
        match country_code {
            Some("us") | None => vec![WeatherSource::Nws, WeatherSource::OpenMeteo],
            _ => vec![WeatherSource::OpenMeteo],
        }
    }

}

/// Unified weather result from either NWS or Open-Meteo.
#[derive(Debug, Clone)]
pub struct WeatherResult {
    pub forecast: Forecast,
    pub cached_grid: Option<GridInfo>,
}

// --- NWS API response types ---

#[derive(Debug, Clone, Deserialize)]
pub struct PointsResponse {
    pub properties: PointsProperties,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PointsProperties {
    pub grid_id: String,
    pub grid_x: u32,
    pub grid_y: u32,
    pub relative_location: Option<RelativeLocation>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RelativeLocation {
    pub properties: RelativeLocationProperties,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RelativeLocationProperties {
    pub city: String,
    pub state: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ForecastResponse {
    pub properties: ForecastProperties,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ForecastProperties {
    pub periods: Vec<ForecastPeriod>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct ForecastPeriod {
    pub name: String,
    pub temperature: i32,
    pub temperature_unit: String,
    pub wind_speed: String,
    pub wind_direction: String,
    pub short_forecast: String,
    pub detailed_forecast: String,
    pub is_daytime: bool,
    pub probability_of_precipitation: Option<PrecipValue>,
    #[serde(default)]
    pub start_time: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PrecipValue {
    pub value: Option<f64>,
}

// --- App domain types ---

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedLocation {
    pub name: String,
    pub lat: String,
    pub lon: String,
    #[serde(default)]
    pub cached_grid: Option<GridInfo>,
    #[serde(default)]
    pub source: WeatherSource,
    #[serde(default)]
    pub country_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GridInfo {
    pub office: String,
    pub grid_x: u32,
    pub grid_y: u32,
}

#[derive(Debug, Clone)]
pub struct Forecast {
    pub location_name: String,
    pub periods: Vec<ForecastPeriod>,
    pub hourly_periods: Vec<ForecastPeriod>,
}

#[derive(Debug, Clone)]
pub enum FetchState {
    Idle,
    Loading,
    Loaded,
    Error(String),
}

// --- View helper types ---

#[derive(Debug, Clone)]
pub struct DaySummary {
    pub name: String,
    pub high: Option<i32>,
    pub low: Option<i32>,
    pub unit: String,
    pub short_forecast: String,
    pub is_daytime: bool,
}

pub fn pair_daily_periods(periods: &[ForecastPeriod]) -> Vec<DaySummary> {
    let mut summaries = Vec::new();
    let mut i = 0;

    while i < periods.len() {
        let period = &periods[i];

        if period.is_daytime {
            // Day period — look ahead for matching night
            let low = periods.get(i + 1).and_then(|night| {
                if !night.is_daytime {
                    Some(night.temperature)
                } else {
                    None
                }
            });
            summaries.push(DaySummary {
                name: period.name.clone(),
                high: Some(period.temperature),
                low,
                unit: period.temperature_unit.clone(),
                short_forecast: period.short_forecast.clone(),
                is_daytime: true,
            });
            i += if low.is_some() { 2 } else { 1 };
        } else {
            // Night-only period (e.g., first period is tonight)
            summaries.push(DaySummary {
                name: period.name.clone(),
                high: None,
                low: Some(period.temperature),
                unit: period.temperature_unit.clone(),
                short_forecast: period.short_forecast.clone(),
                is_daytime: false,
            });
            i += 1;
        }
    }

    summaries
}

/// Extract a display hour like "3 PM" from an ISO 8601 string.
///
/// Input format: "2026-02-28T14:00:00-08:00"
/// The 'T' separator is at index 10, hour digits are at 11..13.
pub fn format_hour(start_time: &str) -> String {
    if let Some(t_pos) = start_time.find('T') {
        if let Ok(hour) = start_time[t_pos + 1..t_pos + 3].parse::<u32>() {
            return match hour {
                0 => "12 AM".to_string(),
                1..=11 => format!("{hour} AM"),
                12 => "12 PM".to_string(),
                _ => format!("{} PM", hour - 12),
            };
        }
    }
    start_time.to_string()
}

// --- Geocoding types ---

#[derive(Debug, Clone, Deserialize)]
pub struct SearchResult {
    pub display_name: String,
    pub lat: String,
    pub lon: String,
    #[serde(default)]
    pub address: Option<NominatimAddress>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NominatimAddress {
    #[serde(default)]
    pub country_code: Option<String>,
}

/// Extract a short "City, State" name from a Nominatim display_name.
///
/// Nominatim US results look like:
///   "Denver, City and County of Denver, Colorado, United States"
///   "Chicago, Cook County, Illinois, United States"
///
/// We take the first part (city) and second-to-last part (state),
/// falling back to the full string if the format is unexpected.
pub fn short_location_name(display_name: &str) -> String {
    let parts: Vec<&str> = display_name.split(", ").collect();
    if parts.len() >= 3 {
        let city = parts[0];
        let state = parts[parts.len() - 2];
        format!("{city}, {state}")
    } else {
        display_name.to_string()
    }
}
