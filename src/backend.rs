use crate::nws;
use crate::types::{
    AirQuality, AlertSeverity, Alerts, CurrentObservation, DailySun, Forecast, ForecastPeriod,
    GridInfo, PrecipValue, WeatherAlert, WeatherResult,
};
use std::collections::HashSet;
use weathervane::{MeasurementSystem, TemperatureUnit};

pub async fn fetch_weather(
    lat: String,
    lon: String,
    use_fahrenheit: bool,
    country_code: Option<String>, //"us" gates the shim
    cached_grid: Option<GridInfo>,
    location_name: String, // from the saved location (geocoding)
) -> Result<WeatherResult, String> {
    let temp_unit = if use_fahrenheit {
        TemperatureUnit::Fahrenheit
    } else {
        TemperatureUnit::Celsius
    };
    let measurement = if use_fahrenheit {
        MeasurementSystem::Imperial
    } else {
        MeasurementSystem::Metric
    };
    let latf: f64 = lat.parse().map_err(|_| "bad latitude".to_string())?;
    let lonf: f64 = lon.parse().map_err(|_| "bad longitude".to_string())?;

    let is_us = country_code.as_deref() == Some("us");

    // shim future: US-only, best-effort. Runs concurrently with weathervane calls
    let shim_fut = async {
        if !is_us {
            return None;
        }
        // reuse cached grid if present, else /points
        let from_cache = cached_grid.is_some();
        let grid = match &cached_grid {
            Some(g) => g.clone(),
            None => match nws::fetch_points(&lat, &lon).await {
                Ok((g, _name)) => g,
                Err(_) => return None,
            },
        };
        let (periods, geometry) = nws::fetch_forecast(&grid, use_fahrenheit).await.ok()?;
        // A cached grid can predate the v0.3.0 write fix and belong to a
        // different location; the response polygon is the only evidence of
        // that. A grid just derived from /points needs no check.
        if from_cache && !nws::grid_matches(geometry.as_ref(), latf, lonf) {
            let (fresh, _name) = nws::fetch_points(&lat, &lon).await.ok()?;
            let (periods, _) = nws::fetch_forecast(&fresh, use_fahrenheit).await.ok()?;
            // Accept whatever this yields - /points is authoritative for these
            // coordinates whatever the polygon says.
            return Some((fresh, periods));
        }
        Some((grid, periods))
    };
    let (weather_res, aq_res, alerts_res, shim) = tokio::join!(
        weathervane::fetch_weather(latf, lonf, temp_unit, measurement),
        weathervane::fetch_air_quality(latf, lonf, None),
        weathervane::fetch_alerts_detailed(latf, lonf),
        shim_fut,
    );

    let weather = weather_res.map_err(|e| e.to_string())?;
    let air_quality = aq_res.ok().map(map_air_quality);
    let alerts = alerts_from_result(alerts_res);

    let unit_str = if use_fahrenheit { "F" } else { "C" };
    let speed_unit = measurement.wind_speed_unit();

    // Only daily periods + cached grid depend on the shim.
    let (daily_periods, cached_grid) = match shim {
        Some((grid, periods)) => (periods, Some(grid)),
        None => (
            build_daily_periods(&weather.forecast, unit_str, speed_unit),
            None,
        ),
    };

    // Everything else always comes from weathervane.
    let sun_times = weather
        .forecast
        .iter()
        .map(|d| DailySun {
            date: d.date.clone(),
            sunrise: d.sunrise.clone(),
            sunset: d.sunset.clone(),
        })
        .collect();

    let hourly_periods =
        build_hourly_periods(&weather.hourly, &weather.forecast, unit_str, speed_unit);

    let observation = Some(build_observation(&weather, unit_str, speed_unit));

    Ok(WeatherResult {
        forecast: Forecast {
            location_name,
            periods: daily_periods,
            hourly_periods,
            sun_times,
            utc_offset_seconds: weather.utc_offset_seconds,
        },
        cached_grid,
        alerts,
        observation,
        air_quality,
    })
}

/// Classifies an alert fetch. Pure, so it is testable without a network.
/// An `Err` must surface as `Unavailable`: swallowing it into an empty list
/// made a network failure indistinguishable from a quiet day.
fn alerts_from_result(res: weathervane::Result<weathervane::AlertReport>) -> Alerts {
    match res {
        Ok(report) => {
            let mut seen = HashSet::new();
            let list: Vec<WeatherAlert> = report
                .alerts
                .into_iter()
                .map(map_alert)
                .filter(|a| seen.insert(a.key()))
                .collect();
            if report.region_filtered {
                Alerts::Local(list)
            } else {
                Alerts::National(list)
            }
        }
        Err(e) => {
            eprintln!("whether: alert fetch failed: {e}");
            Alerts::Unavailable(e.to_string())
        }
    }
}

fn map_alert(entry: weathervane::AlertEntry) -> WeatherAlert {
    let a = entry.alert;
    WeatherAlert {
        id: a.id,
        event: a.event,
        headline: a.headline,
        description: a.description,
        severity: map_severity(a.severity),
        expires: a.expires,
        area_desc: entry.area_desc,
    }
}

fn build_daily_periods(
    daily: &[weathervane::DailyForecast],
    unit: &str,
    speed_unit: &str,
) -> Vec<ForecastPeriod> {
    let mut periods = Vec::with_capacity(daily.len() * 2);
    for d in daily {
        let wind_dir = d.compass_direction.as_str().to_string();
        let wind_speed = format!("{:.0} {speed_unit}", d.windspeed_max);
        let precip = d.precipitation_probability_max.map(|v| v as f64);

        // Day period (high)
        periods.push(ForecastPeriod {
            name: String::new(),
            condition: Some(d.condition),
            temperature: d.temp_max.round() as i32,
            temperature_unit: unit.to_string(),
            wind_speed: wind_speed.clone(),
            wind_direction: wind_dir.clone(),
            compass: Some(d.compass_direction),
            short_forecast: String::new(),
            detailed_forecast: String::new(),
            is_daytime: true,
            probability_of_precipitation: Some(PrecipValue { value: precip }),
            start_time: Some(format!("{}T06:00:00", d.date)),
        });

        periods.push(ForecastPeriod {
            name: String::new(),
            condition: Some(d.condition),
            temperature: d.temp_min.round() as i32,
            temperature_unit: unit.to_string(),
            wind_speed,
            wind_direction: wind_dir,
            compass: Some(d.compass_direction),
            short_forecast: String::new(),
            detailed_forecast: String::new(),
            is_daytime: false,
            probability_of_precipitation: Some(PrecipValue { value: precip }),
            start_time: Some(format!("{}T18:00:00", d.date)),
        });
    }
    periods
}

fn build_hourly_periods(
    hourly: &[weathervane::HourlyForecast],
    daily: &[weathervane::DailyForecast],
    unit: &str,
    speed_unit: &str,
) -> Vec<ForecastPeriod> {
    hourly
        .iter()
        .map(|h| {
            ForecastPeriod {
                name: String::new(),
                condition: Some(h.condition),
                temperature: h.temperature.round() as i32,
                temperature_unit: unit.to_string(),
                wind_speed: format!("{:.0} {speed_unit}", h.windspeed),
                wind_direction: String::new(), //HourlyForecast carries no wind direction
                compass: None,
                short_forecast: String::new(),
                detailed_forecast: String::new(),
                is_daytime: hour_is_daytime(&h.time, daily),
                probability_of_precipitation: Some(PrecipValue {
                    value: Some(h.precipitation_probability as f64),
                }),
                start_time: Some(h.time.clone()),
            }
        })
        .collect()
}

fn build_observation(
    w: &weathervane::WeatherData,
    unit: &str,
    speed_unit: &str,
) -> CurrentObservation {
    let cur = &w.current;
    let is_daytime = w
        .forecast
        .first()
        .map(|d| !weathervane::is_night_time(&d.sunrise, &d.sunset, w.utc_offset_seconds))
        .unwrap_or(true);

    CurrentObservation {
        temperature: Some(cur.temperature.round() as i32),
        temperature_unit: unit.to_string(),
        condition: Some(cur.condition),
        wind_speed: Some(format!("{:.0} {speed_unit}", cur.windspeed)),
        compass: Some(cur.compass_direction),
        humidity: Some(cur.humidity),
        is_daytime,
        feels_like: Some(cur.feels_like.round() as i32),
        dew_point: Some(cur.dew_point.round() as i32),
        uv_index: Some(cur.uv_index),
        pressure: Some(cur.pressure), // hPa raw; PressureUnit conversion is T5
        cloud_cover: Some(cur.cloud_cover),
        wind_gusts: Some(format!("{:.0} {speed_unit}", cur.wind_gusts)),
        visibility: Some(cur.visibility), // meters raw; convert in T4
    }
}

/// Which day's sunrise/sunset brackets this hour. All times are local-frame ISO
/// string, so a lexigcographic compare is valid.
fn hour_is_daytime(time: &str, daily: &[weathervane::DailyForecast]) -> bool {
    let date = time.get(..10).unwrap_or(time);
    daily
        .iter()
        .find(|d| d.date == date)
        .map(|d| time >= d.sunrise.as_str() && time < d.sunset.as_str())
        .unwrap_or(true)
}

fn map_severity(s: weathervane::AlertSeverity) -> AlertSeverity {
    use weathervane::AlertSeverity as S;
    match s {
        S::Extreme => AlertSeverity::Extreme,
        S::Severe => AlertSeverity::Severe,
        S::Moderate => AlertSeverity::Moderate,
        S::Minor => AlertSeverity::Minor,
        S::Unknown => AlertSeverity::Unknown,
    }
}

fn map_air_quality(a: weathervane::AirQualityData) -> AirQuality {
    AirQuality {
        aqi: a.aqi,
        category: a.category,
        pm2_5: a.pm2_5,
        pm10: a.pm10,
        ozone: a.ozone,
        no2: a.nitrogen_dioxide,
        co: a.carbon_monoxide,
        severity: aqi_severity_index(&a.category),
    }
}

fn aqi_severity_index(c: &weathervane::AqiCategory) -> u8 {
    use weathervane::{AqiCategory, EuAqiCategory as Eu, UsAqiCategory as Us};
    match c {
        AqiCategory::Us(Us::Good) | AqiCategory::Eu(Eu::Good) => 0,
        AqiCategory::Us(Us::Moderate) | AqiCategory::Eu(Eu::Moderate) => 1,
        AqiCategory::Us(Us::UnhealthySensitive) | AqiCategory::Eu(Eu::Fair) => 2,
        AqiCategory::Us(Us::Unhealthy) | AqiCategory::Eu(Eu::Poor) => 3,
        AqiCategory::Us(Us::VeryUnhealthy) | AqiCategory::Eu(Eu::VeryPoor) => 4,
        AqiCategory::Us(Us::Hazardous) | AqiCategory::Eu(Eu::ExtremelyPoor) => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use weathervane::{Alert, AlertEntry, AlertReport};

    fn entry(id: &str) -> AlertEntry {
        AlertEntry {
            alert: Alert {
                id: id.into(),
                event: "Wind Advisory".into(),
                severity: weathervane::AlertSeverity::Minor,
                headline: String::new(),
                description: String::new(),
                expires: chrono::DateTime::UNIX_EPOCH,
            },
            area_desc: "ClarkCounty".into(),
        }
    }

    #[test]
    fn filtered_report_is_local() {
        let report = AlertReport {
            alerts: vec![entry("a")],
            region_filtered: true,
        };
        let alerts = alerts_from_result(Ok(report));
        assert!(matches!(&alerts, Alerts::Local(a) if a.len() == 1 && a[0].id == "a"));
        assert_eq!(alerts.list()[0].area_desc, "ClarkCounty");
    }

    #[test]
    fn unfiltered_report_is_national() {
        let report = AlertReport {
            alerts: vec![entry("a"), entry("b")],
            region_filtered: false,
        };
        let alerts = alerts_from_result(Ok(report));
        assert!(matches!(alerts, Alerts::National(a) if a.len() == 2));
    }

    #[test]
    fn fetch_error_is_unavailable_not_a_quiet_day() {
        let alerts = alerts_from_result(Err(weathervane::Error::Timeout));
        assert!(matches!(alerts, Alerts::Unavailable(_)));
        assert!(alerts.list().is_empty());
    }

    fn entry_in(id: &str, area: &str) -> AlertEntry {
        let mut e = entry(id);
        e.area_desc = area.into();
        e
    }

    #[test]
    fn polygon_duplicates_collapse_to_one_row() {
        // The legacy atom feed emits one CAP alert twice for the same area,
        // index_polygon=1 and =0, otherwise identical. Stockholm, 2026-09-02
        let report = AlertReport {
            alerts: vec![entry("a"), entry("a"), entry("b")],
            region_filtered: false,
        };
        let alerts = alerts_from_result(Ok(report));
        assert!(matches!(&alerts, Alerts::National(a) if a.len() ==2));
        assert_eq!(alerts.list()[0].id, "a");
        assert_eq!(alerts.list()[1].id, "b");
    }

    #[test]
    fn same_identifier_area_stays_two_rows() {
        let report = AlertReport {
            alerts: vec![entry_in("a", "Berlin"), entry_in("a", "Brandenburg")],
            region_filtered: false,
        };
        let alerts = alerts_from_result(Ok(report));
        assert!(matches!(&alerts, Alerts::National(a) if a.len() == 2));
    }
}
