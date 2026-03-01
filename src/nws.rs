use crate::types::{
    self, AlertProperties, AlertSeverity, AlertsResponse, CurrentObservation, Forecast,
    ForecastPeriod, ForecastResponse, GridInfo, ObservationResponse, PointsResponse,
    StationsResponse, WeatherAlert,
};

const BASE_URL: &str = "https://api.weather.gov";
const USER_AGENT: &str = "cosmic-ext-whether/0.1.0 (https://github.com/nwxnw/cosmic-ext-whether)";

#[derive(Debug, Clone)]
pub enum NwsError {
    Network(String),
    Api(String),
    Parse(String),
}

impl std::fmt::Display for NwsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NwsError::Network(msg) => write!(f, "Network error: {msg}"),
            NwsError::Api(msg) => write!(f, "API error: {msg}"),
            NwsError::Parse(msg) => write!(f, "Parse error: {msg}"),
        }
    }
}

fn client() -> Result<reqwest::Client, NwsError> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| NwsError::Network(e.to_string()))
}

/// Look up the NWS grid for a lat/lon and extract the nearest city name.
pub async fn fetch_points(lat: &str, lon: &str) -> Result<(GridInfo, String), NwsError> {
    let url = format!("{BASE_URL}/points/{lat},{lon}");
    let resp = client()?
        .get(&url)
        .send()
        .await
        .map_err(|e| NwsError::Network(e.to_string()))?;

    if !resp.status().is_success() {
        return Err(NwsError::Api(format!("Points API returned {}", resp.status())));
    }

    let points: PointsResponse = resp
        .json()
        .await
        .map_err(|e| NwsError::Parse(e.to_string()))?;

    let props = &points.properties;
    let grid = GridInfo {
        office: props.grid_id.clone(),
        grid_x: props.grid_x,
        grid_y: props.grid_y,
        nearest_station: None,
    };

    let location_name = props
        .relative_location
        .as_ref()
        .map(|rl| format!("{}, {}", rl.properties.city, rl.properties.state))
        .unwrap_or_default();

    Ok((grid, location_name))
}

/// Fetch the 7-day forecast for a given grid.
pub async fn fetch_forecast(
    grid: &GridInfo,
    use_fahrenheit: bool,
) -> Result<Vec<ForecastPeriod>, NwsError> {
    let units = if use_fahrenheit { "us" } else { "si" };
    let url = format!(
        "{BASE_URL}/gridpoints/{}/{},{}/forecast?units={units}",
        grid.office, grid.grid_x, grid.grid_y
    );

    let resp = client()?
        .get(&url)
        .send()
        .await
        .map_err(|e| NwsError::Network(e.to_string()))?;

    if !resp.status().is_success() {
        return Err(NwsError::Api(format!(
            "Forecast API returned {}",
            resp.status()
        )));
    }

    let forecast: ForecastResponse = resp
        .json()
        .await
        .map_err(|e| NwsError::Parse(e.to_string()))?;

    Ok(forecast.properties.periods)
}

/// Fetch the hourly forecast for a given grid (first 24 hours).
pub async fn fetch_forecast_hourly(
    grid: &GridInfo,
    use_fahrenheit: bool,
) -> Result<Vec<ForecastPeriod>, NwsError> {
    let units = if use_fahrenheit { "us" } else { "si" };
    let url = format!(
        "{BASE_URL}/gridpoints/{}/{},{}/forecast/hourly?units={units}",
        grid.office, grid.grid_x, grid.grid_y
    );

    let resp = client()?
        .get(&url)
        .send()
        .await
        .map_err(|e| NwsError::Network(e.to_string()))?;

    if !resp.status().is_success() {
        return Err(NwsError::Api(format!(
            "Hourly forecast API returned {}",
            resp.status()
        )));
    }

    let forecast: ForecastResponse = resp
        .json()
        .await
        .map_err(|e| NwsError::Parse(e.to_string()))?;

    // Return only the first 24 hourly periods
    Ok(forecast.properties.periods.into_iter().take(24).collect())
}

fn parse_severity(s: &str) -> AlertSeverity {
    match s {
        "Extreme" => AlertSeverity::Extreme,
        "Severe" => AlertSeverity::Severe,
        "Moderate" => AlertSeverity::Moderate,
        "Minor" => AlertSeverity::Minor,
        _ => AlertSeverity::Unknown,
    }
}

fn parse_alert(props: AlertProperties) -> WeatherAlert {
    WeatherAlert {
        headline: props.headline.unwrap_or_else(|| props.event.clone()),
        severity: parse_severity(&props.severity),
        event: props.event,
    }
}

/// Fetch active weather alerts for a lat/lon.
pub async fn fetch_alerts(lat: &str, lon: &str) -> Result<Vec<WeatherAlert>, NwsError> {
    let url = format!("{BASE_URL}/alerts/active?point={lat},{lon}");
    let resp = client()?
        .get(&url)
        .send()
        .await
        .map_err(|e| NwsError::Network(e.to_string()))?;

    if !resp.status().is_success() {
        return Err(NwsError::Api(format!("Alerts API returned {}", resp.status())));
    }

    let alerts: AlertsResponse = resp
        .json()
        .await
        .map_err(|e| NwsError::Parse(e.to_string()))?;

    Ok(alerts.features.into_iter().map(|f| parse_alert(f.properties)).collect())
}

/// Fetch the nearest observation station for a grid point.
async fn fetch_nearest_station(grid: &GridInfo) -> Result<String, NwsError> {
    let url = format!(
        "{BASE_URL}/gridpoints/{}/{},{}/stations",
        grid.office, grid.grid_x, grid.grid_y
    );
    let resp = client()?
        .get(&url)
        .send()
        .await
        .map_err(|e| NwsError::Network(e.to_string()))?;

    if !resp.status().is_success() {
        return Err(NwsError::Api(format!(
            "Stations API returned {}",
            resp.status()
        )));
    }

    let stations: StationsResponse = resp
        .json()
        .await
        .map_err(|e| NwsError::Parse(e.to_string()))?;

    stations
        .features
        .first()
        .map(|f| f.properties.station_identifier.clone())
        .ok_or_else(|| NwsError::Api("No stations found".into()))
}

/// Use cached station ID or fetch a fresh one.
async fn resolve_station(grid: &GridInfo) -> Result<String, NwsError> {
    if let Some(ref id) = grid.nearest_station {
        Ok(id.clone())
    } else {
        fetch_nearest_station(grid).await
    }
}

/// Fetch the latest observation from a station.
async fn fetch_observation(
    station_id: &str,
    use_fahrenheit: bool,
) -> Result<CurrentObservation, NwsError> {
    let url = format!("{BASE_URL}/stations/{station_id}/observations/latest");
    let resp = client()?
        .get(&url)
        .send()
        .await
        .map_err(|e| NwsError::Network(e.to_string()))?;

    if !resp.status().is_success() {
        return Err(NwsError::Api(format!(
            "Observation API returned {}",
            resp.status()
        )));
    }

    let obs: ObservationResponse = resp
        .json()
        .await
        .map_err(|e| NwsError::Parse(e.to_string()))?;

    let props = obs.properties;

    // Staleness check: discard observations older than 2 hours
    if let Some(ref ts) = props.timestamp {
        if let Ok(obs_time) = chrono::DateTime::parse_from_rfc3339(ts) {
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            if let Some(now) = chrono::DateTime::from_timestamp(now_secs, 0) {
                let age = now.signed_duration_since(obs_time);
                if age.num_hours() >= 2 {
                    return Err(NwsError::Api("Observation too stale".into()));
                }
            }
        }
    }

    // NWS observations are always SI (Celsius, km/h, degrees)
    let temperature = props
        .temperature
        .and_then(|q| q.value)
        .map(|c| {
            if use_fahrenheit {
                (c * 9.0 / 5.0 + 32.0).round() as i32
            } else {
                c.round() as i32
            }
        });

    let temperature_unit = if use_fahrenheit { "F" } else { "C" }.to_string();

    let speed_unit = if use_fahrenheit { "mph" } else { "km/h" };
    let wind_speed = props.wind_speed.and_then(|q| q.value).map(|kmh| {
        let speed = if use_fahrenheit {
            (kmh * 0.621371).round()
        } else {
            kmh.round()
        };
        format!("{speed:.0} {speed_unit}")
    });

    let wind_direction = props
        .wind_direction
        .and_then(|q| q.value)
        .map(types::degrees_to_cardinal);

    let humidity = props
        .relative_humidity
        .and_then(|q| q.value)
        .map(|v| v.round() as i32);

    Ok(CurrentObservation {
        temperature,
        temperature_unit,
        condition: props.text_description,
        wind_speed,
        wind_direction,
        humidity,
        is_daytime: true,
    })
}

/// Combined fetch: resolve grid (using cache if available), then get the forecast.
pub async fn fetch_weather(
    lat: String,
    lon: String,
    cached_grid: Option<GridInfo>,
    use_fahrenheit: bool,
) -> Result<(Forecast, GridInfo, Vec<WeatherAlert>, Option<CurrentObservation>), NwsError> {
    let (mut grid, location_name) = if let Some(grid) = cached_grid {
        // Try the forecast with the cached grid; if it fails, re-resolve.
        match fetch_forecast(&grid, use_fahrenheit).await {
            Ok(periods) => {
                // Parallelize: location name, hourly, alerts, station
                let (name_result, hourly_result, alerts_result, station_result) = tokio::join!(
                    fetch_points(&lat, &lon),
                    fetch_forecast_hourly(&grid, use_fahrenheit),
                    fetch_alerts(&lat, &lon),
                    resolve_station(&grid),
                );
                let name = name_result.map(|(_, n)| n).unwrap_or_default();
                let hourly_periods = hourly_result.unwrap_or_default();
                let alerts = alerts_result.unwrap_or_default();

                let mut grid = grid;
                let observation = if let Ok(station_id) = station_result {
                    grid.nearest_station = Some(station_id.clone());
                    fetch_observation(&station_id, use_fahrenheit).await.ok()
                } else {
                    None
                };

                return Ok((
                    Forecast {
                        location_name: name,
                        periods,
                        hourly_periods,
                    },
                    grid,
                    alerts,
                    observation,
                ));
            }
            Err(_) => {
                // Grid may be stale, re-resolve
                fetch_points(&lat, &lon).await?
            }
        }
    } else {
        fetch_points(&lat, &lon).await?
    };

    // Parallelize independent fetches after grid resolution
    let (forecast_result, hourly_result, alerts_result, station_result) = tokio::join!(
        fetch_forecast(&grid, use_fahrenheit),
        fetch_forecast_hourly(&grid, use_fahrenheit),
        fetch_alerts(&lat, &lon),
        resolve_station(&grid),
    );

    let periods = forecast_result?;
    let hourly_periods = hourly_result.unwrap_or_default();
    let alerts = alerts_result.unwrap_or_default();

    let observation = if let Ok(station_id) = station_result {
        grid.nearest_station = Some(station_id.clone());
        fetch_observation(&station_id, use_fahrenheit).await.ok()
    } else {
        None
    };

    Ok((
        Forecast {
            location_name,
            periods,
            hourly_periods,
        },
        grid,
        alerts,
        observation,
    ))
}
