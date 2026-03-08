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

// --- Weather alert types ---

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlertSeverity {
    Extreme,
    Severe,
    Moderate,
    Minor,
    Unknown,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct WeatherAlert {
    pub event: String,
    pub headline: String,
    pub severity: AlertSeverity,
}

#[derive(Deserialize)]
pub struct AlertsResponse {
    pub features: Vec<AlertFeature>,
}

#[derive(Deserialize)]
pub struct AlertFeature {
    pub properties: AlertProperties,
}

#[derive(Deserialize)]
pub struct AlertProperties {
    pub event: String,
    pub headline: Option<String>,
    pub severity: String,
}

/// Unified weather result from either NWS or Open-Meteo.
#[derive(Debug, Clone)]
pub struct WeatherResult {
    pub forecast: Forecast,
    pub cached_grid: Option<GridInfo>,
    pub alerts: Vec<WeatherAlert>,
    pub observation: Option<CurrentObservation>,
}

/// Real-time observation data from the nearest station (NWS) or current block (Open-Meteo).
#[derive(Debug, Clone)]
pub struct CurrentObservation {
    pub temperature: Option<i32>,
    pub temperature_unit: String,
    pub condition: Option<String>,
    pub wind_speed: Option<String>,
    pub wind_direction: Option<String>,
    pub humidity: Option<i32>,
    pub is_daytime: bool,
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

// --- NWS station / observation types ---

#[derive(Deserialize)]
pub struct StationsResponse {
    pub features: Vec<StationFeature>,
}

#[derive(Deserialize)]
pub struct StationFeature {
    pub properties: StationProperties,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StationProperties {
    pub station_identifier: String,
}

#[derive(Deserialize)]
pub struct ObservationResponse {
    pub properties: ObservationProperties,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservationProperties {
    pub temperature: Option<QuantitativeValue>,
    pub wind_speed: Option<QuantitativeValue>,
    pub wind_direction: Option<QuantitativeValue>,
    pub relative_humidity: Option<QuantitativeValue>,
    pub text_description: Option<String>,
    pub timestamp: Option<String>,
}

#[derive(Deserialize)]
pub struct QuantitativeValue {
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
    #[serde(default)]
    pub nearest_station: Option<String>,
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
    pub short_forecast: String,
    pub is_daytime: bool,
    pub wind_speed: String,
    pub wind_direction: String,
    pub precip_chance: Option<i32>,
    pub detailed_forecast: String,
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
                short_forecast: period.short_forecast.clone(),
                is_daytime: true,
                wind_speed: period.wind_speed.clone(),
                wind_direction: period.wind_direction.clone(),
                precip_chance: period
                    .probability_of_precipitation
                    .as_ref()
                    .and_then(|p| p.value)
                    .map(|v| v as i32),
                detailed_forecast: period.detailed_forecast.clone(),
            });
            i += if low.is_some() { 2 } else { 1 };
        } else {
            // Night-only period (e.g., first period is tonight)
            summaries.push(DaySummary {
                name: period.name.clone(),
                high: None,
                low: Some(period.temperature),
                short_forecast: period.short_forecast.clone(),
                is_daytime: false,
                wind_speed: period.wind_speed.clone(),
                wind_direction: period.wind_direction.clone(),
                precip_chance: period
                    .probability_of_precipitation
                    .as_ref()
                    .and_then(|p| p.value)
                    .map(|v| v as i32),
                detailed_forecast: period.detailed_forecast.clone(),
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

/// Convert wind direction in degrees to a cardinal direction string.
pub fn degrees_to_cardinal(degrees: f64) -> String {
    const DIRECTIONS: &[&str] = &[
        "N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW",
        "NW", "NNW",
    ];
    let idx = ((degrees + 11.25) / 22.5) as usize % 16;
    DIRECTIONS[idx].to_string()
}

/// Map a weather condition string and day/night flag to a symbolic icon name.
pub fn condition_icon(condition: &str, is_daytime: bool) -> &'static str {
    let s = condition.to_lowercase();
    let night = !is_daytime;

    if s.contains("thunder") || s.contains("storm") {
        return "weather-storm-symbolic";
    }
    if s.contains("snow") || s.contains("flurr") || s.contains("blizzard") {
        return "weather-snow-symbolic";
    }
    if s.contains("rain") || s.contains("shower") || s.contains("drizzle") {
        return if s.contains("scattered") {
            "weather-showers-scattered-symbolic"
        } else {
            "weather-showers-symbolic"
        };
    }
    if s.contains("fog") || s.contains("mist") || s.contains("haz") {
        return "weather-fog-symbolic";
    }
    if s.contains("mostly cloudy") || s.contains("overcast") {
        return "weather-overcast-symbolic";
    }
    if s.contains("partly") && s.contains("cloud") {
        return if night {
            "weather-few-clouds-night-symbolic"
        } else {
            "weather-few-clouds-symbolic"
        };
    }
    if s.contains("mostly sunny") || s.contains("mostly clear") {
        return if night {
            "weather-few-clouds-night-symbolic"
        } else {
            "weather-few-clouds-symbolic"
        };
    }
    if s.contains("sunny") || s.contains("clear") {
        return if night {
            "weather-clear-night-symbolic"
        } else {
            "weather-clear-symbolic"
        };
    }
    if s.contains("cloud") {
        return "weather-overcast-symbolic";
    }

    if night {
        "weather-clear-night-symbolic"
    } else {
        "weather-clear-symbolic"
    }
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
