use crate::types::{
    Forecast, ForecastPeriod, ForecastResponse, GridInfo, PointsResponse,
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

/// Combined fetch: resolve grid (using cache if available), then get the forecast.
pub async fn fetch_weather(
    lat: String,
    lon: String,
    cached_grid: Option<GridInfo>,
    use_fahrenheit: bool,
) -> Result<(Forecast, GridInfo), NwsError> {
    let (grid, location_name) = if let Some(grid) = cached_grid {
        // Still need the location name, but we can try the forecast directly.
        // If forecast fails we'll re-resolve the grid.
        match fetch_forecast(&grid, use_fahrenheit).await {
            Ok(periods) => {
                // We don't have the location name cached, fetch it.
                let name = fetch_points(&lat, &lon)
                    .await
                    .map(|(_, name)| name)
                    .unwrap_or_default();
                let hourly_periods = fetch_forecast_hourly(&grid, use_fahrenheit)
                    .await
                    .unwrap_or_default();
                return Ok((
                    Forecast {
                        location_name: name,
                        periods,
                        hourly_periods,
                    },
                    grid,
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

    let periods = fetch_forecast(&grid, use_fahrenheit).await?;
    let hourly_periods = fetch_forecast_hourly(&grid, use_fahrenheit)
        .await
        .unwrap_or_default();
    Ok((
        Forecast {
            location_name,
            periods,
            hourly_periods,
        },
        grid,
    ))
}
