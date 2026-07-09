use crate::types::{ForecastPeriod, ForecastResponse, GridInfo, PointsResponse};

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
        return Err(NwsError::Api(format!(
            "Points API returned {}",
            resp.status()
        )));
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
