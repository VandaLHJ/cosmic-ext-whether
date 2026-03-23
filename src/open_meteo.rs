use crate::types::{self, CurrentObservation, Forecast, ForecastPeriod, PrecipValue};

const BASE_URL: &str = "https://api.open-meteo.com/v1/forecast";

#[derive(Debug, Clone)]
pub enum OpenMeteoError {
    Network(String),
    Api(String),
    Parse(String),
}

impl std::fmt::Display for OpenMeteoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenMeteoError::Network(msg) => write!(f, "Network error: {msg}"),
            OpenMeteoError::Api(msg) => write!(f, "API error: {msg}"),
            OpenMeteoError::Parse(msg) => write!(f, "Parse error: {msg}"),
        }
    }
}

// --- Open-Meteo API response types ---

#[derive(Debug, serde::Deserialize)]
struct OmResponse {
    #[serde(default)]
    utc_offset_seconds: i32,
    hourly: Option<OmHourly>,
    daily: Option<OmDaily>,
    current: Option<OmCurrent>,
}

#[derive(Debug, serde::Deserialize)]
struct OmCurrent {
    temperature_2m: Option<f64>,
    weather_code: Option<u32>,
    wind_speed_10m: Option<f64>,
    wind_direction_10m: Option<f64>,
    is_day: Option<u8>,
    relative_humidity_2m: Option<f64>,
}

#[derive(Debug, serde::Deserialize)]
struct OmHourly {
    time: Vec<String>,
    temperature_2m: Vec<f64>,
    weather_code: Vec<u32>,
    wind_speed_10m: Vec<f64>,
    wind_direction_10m: Vec<f64>,
    precipitation_probability: Vec<Option<f64>>,
    is_day: Vec<u8>,
}

#[derive(Debug, serde::Deserialize)]
struct OmDaily {
    time: Vec<String>,
    weather_code: Vec<u32>,
    temperature_2m_max: Vec<f64>,
    temperature_2m_min: Vec<f64>,
    wind_speed_10m_max: Vec<f64>,
    wind_direction_10m_dominant: Vec<f64>,
    precipitation_probability_max: Vec<Option<f64>>,
}

/// Fetch weather data from Open-Meteo and convert to shared Forecast type.
pub async fn fetch_weather(
    lat: String,
    lon: String,
    location_name: String,
    use_fahrenheit: bool,
) -> Result<(Forecast, Option<CurrentObservation>), OpenMeteoError> {
    let temp_unit = if use_fahrenheit {
        "fahrenheit"
    } else {
        "celsius"
    };
    let wind_unit = if use_fahrenheit { "mph" } else { "kmh" };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| OpenMeteoError::Network(e.to_string()))?;

    let resp = client
        .get(BASE_URL)
        .query(&[
            ("latitude", lat.as_str()),
            ("longitude", lon.as_str()),
            ("timezone", "auto"),
            ("temperature_unit", temp_unit),
            ("wind_speed_unit", wind_unit),
            (
                "hourly",
                "temperature_2m,weather_code,wind_speed_10m,wind_direction_10m,precipitation_probability,is_day",
            ),
            (
                "daily",
                "weather_code,temperature_2m_max,temperature_2m_min,wind_speed_10m_max,wind_direction_10m_dominant,precipitation_probability_max",
            ),
            (
                "current",
                "temperature_2m,weather_code,wind_speed_10m,wind_direction_10m,is_day,relative_humidity_2m",
            ),
            ("forecast_days", "7"),
        ])
        .send()
        .await
        .map_err(|e| OpenMeteoError::Network(e.to_string()))?;

    if !resp.status().is_success() {
        return Err(OpenMeteoError::Api(format!(
            "Open-Meteo returned {}",
            resp.status()
        )));
    }

    let data: OmResponse = resp
        .json()
        .await
        .map_err(|e| OpenMeteoError::Parse(e.to_string()))?;

    let unit_str = if use_fahrenheit { "F" } else { "C" };
    let speed_unit = if use_fahrenheit { "mph" } else { "km/h" };

    // Determine timezone offset. If timezone=auto worked, utc_offset_seconds
    // will be the correct local offset (e.g., -25200 for MST) and hourly times
    // are already in local time. If auto silently fell back to GMT,
    // utc_offset_seconds will be 0 and times are UTC — in that case, compute
    // an approximate offset from longitude and adjust the time strings.
    let utc_offset = data.utc_offset_seconds;
    let lon_f: f64 = lon.parse().unwrap_or(0.0);
    let expected_offset = (lon_f / 15.0).round() as i32 * 3600;
    let needs_adjustment = utc_offset == 0 && expected_offset.abs() > 1800;
    let effective_offset = if needs_adjustment {
        expected_offset
    } else {
        utc_offset
    };

    // Build daily forecast periods (day/night pairs like NWS)
    let periods = if let Some(daily) = &data.daily {
        build_daily_periods(daily, unit_str, speed_unit)
    } else {
        Vec::new()
    };

    // Build hourly forecast periods starting from the current hour
    let hourly_periods = if let Some(hourly) = &data.hourly {
        build_hourly_periods(hourly, unit_str, speed_unit, effective_offset, needs_adjustment)
    } else {
        Vec::new()
    };

    // Build current observation from the `current` block
    let observation = data.current.map(|cur| {
        let condition = cur
            .weather_code
            .map(|code| wmo_code_description(code, cur.is_day.unwrap_or(1) == 1));
        let wind_direction = cur.wind_direction_10m.map(types::degrees_to_cardinal);
        let wind_speed = cur.wind_speed_10m.map(|s| format!("{:.0} {speed_unit}", s));

        CurrentObservation {
            temperature: cur.temperature_2m.map(|t| t.round() as i32),
            temperature_unit: unit_str.to_string(),
            condition,
            wind_speed,
            wind_direction,
            humidity: cur.relative_humidity_2m.map(|h| h.round() as i32),
            is_daytime: cur.is_day.unwrap_or(1) == 1,
        }
    });

    Ok((
        Forecast {
            location_name,
            periods,
            hourly_periods,
        },
        observation,
    ))
}

fn build_daily_periods(daily: &OmDaily, unit: &str, speed_unit: &str) -> Vec<ForecastPeriod> {
    let mut periods = Vec::new();

    for i in 0..daily.time.len() {
        let day_name = date_to_day_name(&daily.time[i], i == 0);
        let description = wmo_code_description(daily.weather_code[i], true);
        let wind_dir = types::degrees_to_cardinal(daily.wind_direction_10m_dominant[i]);
        let wind_speed = format!("{:.0} {speed_unit}", daily.wind_speed_10m_max[i]);
        let precip = daily
            .precipitation_probability_max
            .get(i)
            .and_then(|v| *v);

        // Day period
        periods.push(ForecastPeriod {
            name: day_name.clone(),
            temperature: daily.temperature_2m_max[i].round() as i32,
            temperature_unit: unit.to_string(),
            wind_speed: wind_speed.clone(),
            wind_direction: wind_dir.clone(),
            short_forecast: description.clone(),
            detailed_forecast: description.clone(),
            is_daytime: true,
            probability_of_precipitation: Some(PrecipValue {
                value: precip,
            }),
            start_time: Some(format!("{}T12:00:00", daily.time[i])),
        });

        // Night period
        let night_name = if i == 0 {
            "Tonight".to_string()
        } else {
            format!("{day_name} Night")
        };
        let night_description = wmo_code_description(daily.weather_code[i], false);
        periods.push(ForecastPeriod {
            name: night_name,
            temperature: daily.temperature_2m_min[i].round() as i32,
            temperature_unit: unit.to_string(),
            wind_speed,
            wind_direction: wind_dir,
            short_forecast: night_description.clone(),
            detailed_forecast: night_description,
            is_daytime: false,
            probability_of_precipitation: Some(PrecipValue {
                value: precip,
            }),
            start_time: Some(format!("{}T00:00:00", daily.time[i])),
        });
    }

    periods
}

fn build_hourly_periods(
    hourly: &OmHourly,
    unit: &str,
    speed_unit: &str,
    offset_secs: i32,
    needs_adjustment: bool,
) -> Vec<ForecastPeriod> {
    // Compute current local time string for skipping past entries.
    // We add the offset to the UTC epoch seconds then format — this gives us
    // a local-time string we can compare lexicographically with ISO 8601 times.
    let now_utc_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let local_secs = now_utc_secs + offset_secs as i64;
    let now_local_str = chrono::DateTime::from_timestamp(local_secs, 0)
        .map(|dt| dt.format("%Y-%m-%dT%H:%M").to_string())
        .unwrap_or_default();

    // Find first hourly entry at or after the current local time
    let start = hourly
        .time
        .iter()
        .position(|t| {
            let effective = if needs_adjustment {
                adjust_time_string(t, offset_secs)
            } else {
                t.clone()
            };
            effective >= now_local_str
        })
        .unwrap_or(0);

    let end = (start + 24).min(hourly.time.len());
    let mut periods = Vec::with_capacity(end - start);

    for i in start..end {
        let time_str = if needs_adjustment {
            adjust_time_string(&hourly.time[i], offset_secs)
        } else {
            hourly.time[i].clone()
        };
        let is_day = hourly.is_day[i] == 1;
        let description = wmo_code_description(hourly.weather_code[i], is_day);
        let wind_dir = types::degrees_to_cardinal(hourly.wind_direction_10m[i]);
        let wind_speed = format!("{:.0} {speed_unit}", hourly.wind_speed_10m[i]);
        let precip = hourly
            .precipitation_probability
            .get(i)
            .and_then(|v| *v);

        periods.push(ForecastPeriod {
            name: String::new(),
            temperature: hourly.temperature_2m[i].round() as i32,
            temperature_unit: unit.to_string(),
            wind_speed,
            wind_direction: wind_dir,
            short_forecast: description.clone(),
            detailed_forecast: description,
            is_daytime: is_day,
            probability_of_precipitation: Some(PrecipValue {
                value: precip,
            }),
            start_time: Some(time_str),
        });
    }

    periods
}

/// Shift an ISO time string ("2026-02-28T21:00") by offset_secs.
/// Used to convert UTC times to approximate local time when timezone=auto
/// falls back to GMT.
fn adjust_time_string(time_str: &str, offset_secs: i32) -> String {
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(time_str, "%Y-%m-%dT%H:%M") {
        let adjusted = dt + chrono::TimeDelta::seconds(offset_secs as i64);
        adjusted.format("%Y-%m-%dT%H:%M").to_string()
    } else {
        time_str.to_string()
    }
}

/// Convert an ISO date string ("2026-03-01") to a day name ("Sunday").
/// Returns "Today" for the first day.
fn date_to_day_name(date_str: &str, is_first: bool) -> String {
    if is_first {
        return "Today".to_string();
    }
    // Parse YYYY-MM-DD
    if let Ok(date) = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
        return date.format("%A").to_string();
    }
    date_str.to_string()
}


/// Map WMO weather interpretation codes to description strings.
/// Descriptions are chosen so that `weather_icon_for_period()` in app.rs
/// matches them correctly (contains "rain", "snow", "storm", "fog", "cloud", "clear", etc.).
fn wmo_code_description(code: u32, is_day: bool) -> String {
    match code {
        0 => {
            if is_day {
                "Clear sky"
            } else {
                "Clear"
            }
        }
        1 => "Mostly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 | 48 => "Fog",
        51 => "Light drizzle",
        53 => "Moderate drizzle",
        55 => "Dense drizzle",
        56 | 57 => "Freezing drizzle",
        61 => "Slight rain",
        63 => "Moderate rain",
        65 => "Heavy rain",
        66 | 67 => "Freezing rain",
        71 => "Slight snow fall",
        73 => "Moderate snow fall",
        75 => "Heavy snow fall",
        77 => "Snow grains",
        80 => "Slight rain showers",
        81 => "Moderate rain showers",
        82 => "Violent rain showers",
        85 => "Slight snow showers",
        86 => "Heavy snow showers",
        95 => "Thunderstorm",
        96 | 99 => "Thunderstorm with hail",
        _ => "Unknown",
    }
    .to_string()
}
