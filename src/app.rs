use std::sync::LazyLock;

use cosmic::app::{Core, Task};
use cosmic::iced::core::window;
use cosmic::iced::window::Id;
use cosmic::iced::{Alignment, Color, Length, Rectangle, Subscription};
use cosmic::surface::action::{app_popup, destroy_popup};
use cosmic::widget;
use cosmic::Element;

static AUTOSIZE_MAIN_ID: LazyLock<widget::Id> = LazyLock::new(|| widget::Id::new("autosize-main"));

use crate::backend;
use crate::config::{self, WhetherConfig, APP_ID};
use crate::fl;
use crate::geocoding;
use crate::types::{
    condition_icon, format_hour, pair_daily_periods, short_location_name, AirQuality,
    CurrentObservation, FetchState, Forecast, ForecastPeriod, SavedLocation, SearchResult,
    WeatherAlert, WeatherResult,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Page {
    Main,
    Setup,
    Locations,
}

pub struct AppModel {
    core: Core,
    popup: Option<Id>,
    air_quality: Option<AirQuality>,
    current_expanded: bool,
    config: WhetherConfig,
    config_handle: Option<cosmic::cosmic_config::Config>,
    forecast: Option<Forecast>,
    observation: Option<CurrentObservation>,
    fetch_state: FetchState,
    page: Page,
    search_input: String,
    search_results: Vec<SearchResult>,
    searching: bool,
    search_error: Option<String>,
    search_done: bool,
    hourly_offset: usize,
    expanded_day: Option<usize>,
    last_updated: Option<std::time::Instant>,
    location_names: Vec<String>,
    alerts: Vec<WeatherAlert>,
}

/// Number of hourly columns visible at once between the arrow buttons.
const HOURLY_PAGE_SIZE: usize = 6;

impl Default for AppModel {
    fn default() -> Self {
        Self {
            core: Core::default(),
            popup: None,
            air_quality: None,
            current_expanded: false,
            config: WhetherConfig::default(),
            config_handle: None,
            forecast: None,
            observation: None,
            fetch_state: FetchState::Idle,
            page: Page::Setup,
            search_input: String::new(),
            search_results: Vec::new(),
            searching: false,
            search_error: None,
            search_done: false,
            hourly_offset: 0,
            expanded_day: None,
            last_updated: None,
            location_names: Vec::new(),
            alerts: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Message {
    PopupClosed(Id),
    Surface(cosmic::surface::Action),
    FetchWeather,
    WeatherFetched(Box<Result<WeatherResult, String>>),
    Tick(()),
    SearchInput(String),
    SearchSubmit,
    SearchResults(Result<Vec<SearchResult>, String>),
    SelectLocation(usize),

    ActivateLocation(usize),

    AddLocation,
    RemoveLocation(usize),
    BackToMain,
    ToggleUnits,
    HourlyPrev,
    HourlyNext,
    ToggleDay(usize),
    ToggleCurrentMore,
    ConfigChanged(WhetherConfig),
}

impl cosmic::Application for AppModel {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Message>) {
        let (config, config_handle) = config::load_config();
        let page = if config.locations.is_empty() {
            Page::Setup
        } else {
            Page::Main
        };

        let location_names = config.locations.iter().map(|l| l.name.clone()).collect();

        let mut app = Self {
            core,
            config,
            config_handle,
            page,
            location_names,
            ..Default::default()
        };

        let task = if app.config.active_location().is_some() {
            app.fetch_state = FetchState::Loading;
            fetch_weather_task(&app.config)
        } else {
            Task::none()
        };

        (app, task)
    }

    fn view(&self) -> Element<'_, Message> {
        let have_popup = self.popup;
        let icon_name = self.weather_icon_name();
        let suggested_size = self.core.applet.suggested_size(true);

        let icon = widget::icon::from_name(icon_name)
            .symbolic(true)
            .size(suggested_size.0)
            .into();

        let content: Element<'_, Message> = if let Some(temp) = self.current_temp_text() {
            let temp_widget = widget::text::body(temp).width(Length::Shrink);
            if self.core.applet.is_horizontal() {
                cosmic::iced::widget::row![icon, temp_widget]
                    .spacing(4)
                    .align_y(Alignment::Center)
                    .into()
            } else {
                cosmic::iced::widget::column![icon, temp_widget]
                    .spacing(2)
                    .align_x(Alignment::Center)
                    .into()
            }
        } else {
            icon
        };

        let horizontal = self.core.applet.is_horizontal();
        let pad = self.core.applet.suggested_padding(true).0;

        let button = cosmic::widget::button::custom(content)
            .padding(if horizontal { [0, pad] } else { [pad, 0] })
            .class(cosmic::theme::Button::AppletIcon)
            .on_press_with_rectangle(move |offset, bounds| {
                if let Some(id) = have_popup {
                    Message::Surface(destroy_popup(id))
                } else {
                    Message::Surface(app_popup::<AppModel>(
                        move |state: &mut AppModel| {
                            let new_id = Id::unique();
                            state.popup = Some(new_id);
                            let mut popup_settings = state.core.applet.get_popup_settings(
                                state.core.main_window_id().unwrap(),
                                new_id,
                                None,
                                None,
                                None,
                            );
                            popup_settings.positioner.anchor_rect = Rectangle {
                                x: (bounds.x - offset.x) as i32,
                                y: (bounds.y - offset.y) as i32,
                                width: bounds.width as i32,
                                height: bounds.height as i32,
                            };
                            popup_settings
                        },
                        None,
                    ))
                }
            });

        widget::autosize::autosize(button, AUTOSIZE_MAIN_ID.clone()).into()
    }

    fn view_window(&self, _id: Id) -> Element<'_, Message> {
        let content = match &self.page {
            Page::Setup => self.view_setup(),
            Page::Main => self.view_main(),
            Page::Locations => self.view_locations(),
        };
        self.core.applet.popup_container(content).into()
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Surface(a) => {
                return cosmic::task::message(cosmic::Action::Cosmic(
                    cosmic::app::Action::Surface(a),
                ));
            }
            Message::PopupClosed(id) => {
                if self.popup.as_ref() == Some(&id) {
                    self.popup = None;
                }
            }
            Message::FetchWeather => {
                self.fetch_state = FetchState::Loading;
                return fetch_weather_task(&self.config);
            }
            Message::WeatherFetched(result) => match *result {
                Ok(result) => {
                    self.fetch_state = FetchState::Loaded;
                    self.last_updated = Some(std::time::Instant::now());

                    let old_config = self.config.clone();
                    let idx = self.config.active_location_index;
                    if let Some(loc) = self.config.locations.get_mut(idx) {
                        if let Some(grid) = result.cached_grid {
                            loc.cached_grid = Some(grid);
                        }
                        if loc.name.is_empty() && !result.forecast.location_name.is_empty() {
                            loc.name = result.forecast.location_name.clone();
                        }
                    }
                    self.location_names = self
                        .config
                        .locations
                        .iter()
                        .map(|l| l.name.clone())
                        .collect();

                    self.alerts = result.alerts;
                    self.observation = result.observation;
                    self.air_quality = result.air_quality;
                    self.hourly_offset = 0;
                    self.expanded_day = None;
                    self.forecast = Some(result.forecast);
                    if self.config != old_config {
                        config::save_config(&self.config_handle, &self.config);
                    }
                }
                Err(e) => {
                    self.fetch_state = FetchState::Error(e);
                }
            },
            Message::Tick(_) => {
                if self.config.active_location().is_some()
                    && !matches!(self.fetch_state, FetchState::Loading)
                {
                    self.fetch_state = FetchState::Loading;
                    return fetch_weather_task(&self.config);
                }
            }
            Message::SearchInput(input) => {
                if input.is_empty() {
                    self.search_results.clear();
                }
                self.search_input = input;
                self.search_done = false;
            }
            Message::SearchSubmit => {
                let query = self.search_input.clone();
                if !query.is_empty() {
                    self.searching = true;
                    self.search_error = None;
                    return Task::perform(geocoding::search_location(query), |result| {
                        cosmic::Action::App(Message::SearchResults(
                            result.map_err(|e| e.to_string()),
                        ))
                    });
                }
            }
            Message::SearchResults(Ok(results)) => {
                self.searching = false;
                self.search_error = None;
                self.search_done = true;
                self.search_results = results;
            }
            Message::SearchResults(Err(e)) => {
                self.searching = false;
                self.search_done = true;
                self.search_results.clear();
                self.search_error = Some(e);
            }
            Message::SelectLocation(i) => {
                if let Some(result) = self.search_results.get(i) {
                    let country_code = result.address.as_ref().and_then(|a| a.country_code.clone());
                    let location = SavedLocation {
                        name: short_location_name(&result.display_name),
                        lat: result.lat.clone(),
                        lon: result.lon.clone(),
                        cached_grid: None,
                        country_code,
                    };
                    self.config.locations.push(location);
                    self.config.active_location_index = self.config.locations.len() - 1;
                    self.location_names = self
                        .config
                        .locations
                        .iter()
                        .map(|l| l.name.clone())
                        .collect();
                    config::save_config(&self.config_handle, &self.config);
                    self.page = Page::Main;
                    self.search_results.clear();
                    self.search_input.clear();
                    self.search_done = false;
                    self.current_expanded = false;
                    self.fetch_state = FetchState::Loading;
                    return fetch_weather_task(&self.config);
                }
            }

            Message::ActivateLocation(idx) => {
                if idx < self.config.locations.len() && idx != self.config.active_location_index {
                    self.config.active_location_index = idx;
                    self.location_names = self
                        .config
                        .locations
                        .iter()
                        .map(|l| l.name.clone())
                        .collect();
                    config::save_config(&self.config_handle, &self.config);
                    self.page = Page::Main;
                    self.forecast = None;
                    self.observation = None;
                    self.air_quality = None;
                    self.current_expanded = false;
                    self.alerts.clear();
                    self.fetch_state = FetchState::Loading;
                    return fetch_weather_task(&self.config);
                }
            }
            Message::AddLocation => {
                self.page = Page::Locations;
                self.search_results.clear();
                self.search_input.clear();
                self.current_expanded = false;
                self.search_done = false;
                self.search_error = None;
            }
            Message::RemoveLocation(idx) => {
                if idx < self.config.locations.len() {
                    self.config.locations.remove(idx);

                    if self.config.locations.is_empty() {
                        self.config.active_location_index = 0;
                        self.forecast = None;
                        self.observation = None;
                        self.alerts.clear();
                        self.fetch_state = FetchState::Idle;
                        self.page = Page::Setup;
                    } else if idx == self.config.active_location_index {
                        self.config.active_location_index = 0;
                        self.forecast = None;
                        self.observation = None;
                        self.air_quality = None;
                        self.current_expanded = false;
                        self.alerts.clear();
                        self.fetch_state = FetchState::Loading;
                        self.location_names = self
                            .config
                            .locations
                            .iter()
                            .map(|l| l.name.clone())
                            .collect();
                        config::save_config(&self.config_handle, &self.config);
                        return fetch_weather_task(&self.config);
                    } else if idx < self.config.active_location_index {
                        self.config.active_location_index -= 1;
                    }

                    self.location_names = self
                        .config
                        .locations
                        .iter()
                        .map(|l| l.name.clone())
                        .collect();
                    config::save_config(&self.config_handle, &self.config);
                }
            }
            Message::HourlyPrev => {
                self.hourly_offset = self.hourly_offset.saturating_sub(HOURLY_PAGE_SIZE);
            }
            Message::HourlyNext => {
                let total = self
                    .forecast
                    .as_ref()
                    .map(|f| f.hourly_periods.len())
                    .unwrap_or(0);
                let max_offset = total.saturating_sub(HOURLY_PAGE_SIZE);
                self.hourly_offset = (self.hourly_offset + HOURLY_PAGE_SIZE).min(max_offset);
            }
            Message::ToggleCurrentMore => {
                self.current_expanded = !self.current_expanded;
            }
            Message::ToggleDay(idx) => {
                if self.expanded_day == Some(idx) {
                    self.expanded_day = None;
                } else {
                    self.expanded_day = Some(idx);
                }
            }
            Message::ToggleUnits => {
                self.config.use_fahrenheit = !self.config.use_fahrenheit;
                config::save_config(&self.config_handle, &self.config);
                if self.config.active_location().is_some() {
                    self.fetch_state = FetchState::Loading;
                    return fetch_weather_task(&self.config);
                }
            }
            Message::BackToMain => {
                self.page = if self.config.locations.is_empty() {
                    Page::Setup
                } else {
                    Page::Main
                };
                self.search_results.clear();
                self.search_input.clear();
                self.search_done = false;
                self.search_error = None;
            }
            Message::ConfigChanged(new_config) => {
                let location_changed =
                    self.config.active_location() != new_config.active_location();
                let units_changed = self.config.use_fahrenheit != new_config.use_fahrenheit;
                self.config = new_config;
                if (location_changed || units_changed) && self.config.active_location().is_some() {
                    self.fetch_state = FetchState::Loading;
                    return fetch_weather_task(&self.config);
                }
            }
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        let config_sub = self
            .core()
            .watch_config::<WhetherConfig>(APP_ID)
            .map(|update| Message::ConfigChanged(update.config));

        let timer_sub = cosmic::iced::time::every(std::time::Duration::from_secs(
            self.config.refresh_interval_minutes as u64 * 60,
        ))
        .map(|_| Message::Tick(()));

        Subscription::batch(vec![config_sub, timer_sub])
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}

fn fetch_weather_task(config: &WhetherConfig) -> Task<Message> {
    if let Some(loc) = config.active_location() {
        let lat = loc.lat.clone();
        let lon = loc.lon.clone();
        let use_fahrenheit = config.use_fahrenheit;
        let country_code = loc.country_code.clone();
        let cached_grid = loc.cached_grid.clone();
        let name = loc.name.clone();

        // Single adapter entry point: weathervane + US-only NWS shim, all mapped
        // into WeatherResult inside backend::fetch_weather.
        Task::perform(
            backend::fetch_weather(lat, lon, use_fahrenheit, country_code, cached_grid, name),
            |result| cosmic::Action::App(Message::WeatherFetched(Box::new(result))),
        )
    } else {
        Task::none()
    }
}

fn weather_icon_for_period(period: &ForecastPeriod) -> &'static str {
    condition_icon(&period.short_forecast, period.is_daytime)
}

impl AppModel {
    fn current_temp_text(&self) -> Option<String> {
        // Prefer observation temperature, fall back to first forecast period
        if let Some(obs) = &self.observation {
            if let Some(temp) = obs.temperature {
                return Some(format!("{}°{}", temp, obs.temperature_unit));
            }
        }
        self.forecast.as_ref().and_then(|f| {
            f.periods.first().map(|p| {
                let unit = if p.temperature_unit == "F" { "F" } else { "C" };
                format!("{}°{unit}", p.temperature)
            })
        })
    }

    fn weather_icon_name(&self) -> &str {
        if !self.alerts.is_empty() {
            return "weather-severe-alert-symbolic";
        }
        // Prefer observation condition for panel icon
        if let Some(obs) = &self.observation {
            if let Some(ref cond) = obs.condition {
                return condition_icon(cond, obs.is_daytime);
            }
        }
        self.forecast
            .as_ref()
            .and_then(|f| f.periods.first())
            .map(weather_icon_for_period)
            .unwrap_or("weather-clear-symbolic")
    }

    fn view_setup(&self) -> Element<'_, Message> {
        let title = fl!("setup-title");
        let placeholder = fl!("search-placeholder");

        let search = widget::text_input(placeholder, &self.search_input)
            .on_input(Message::SearchInput)
            .on_submit(|_| Message::SearchSubmit);

        let search_label = fl!("search-button");
        let search_btn = widget::button::suggested(search_label).on_press_maybe(
            if self.search_input.is_empty() {
                None
            } else {
                Some(Message::SearchSubmit)
            },
        );

        let search_row = cosmic::iced::widget::row![search, search_btn]
            .spacing(8)
            .align_y(Alignment::Center);

        let mut col = cosmic::iced::widget::column![widget::text::title3(title), search_row,]
            .spacing(12)
            .padding(16)
            .width(Length::Fixed(360.0));

        if self.searching {
            let text = fl!("searching");
            col = col.push(widget::text::body(text));
        } else if let Some(e) = &self.search_error {
            let text = fl!("search-error", error = e.as_str());
            col = col.push(widget::text::body(text));
        } else if !self.search_results.is_empty() {
            for (i, result) in self.search_results.iter().enumerate() {
                let btn =
                    widget::button::text(&result.display_name).on_press(Message::SelectLocation(i));
                col = col.push(btn);
            }
        } else if self.search_done {
            let text = fl!("no-results");
            col = col.push(widget::text::body(text));
        }

        col.into()
    }

    fn view_locations(&self) -> Element<'_, Message> {
        let title = fl!("manage-locations");
        let back_btn =
            widget::button::icon(widget::icon::from_name("go-previous-symbolic").symbolic(true))
                .on_press(Message::BackToMain);

        let title_row =
            cosmic::iced::widget::row![widget::text::title3(title).width(Length::Fill), back_btn,]
                .align_y(Alignment::Center)
                .spacing(8);

        let mut col = cosmic::iced::widget::column![title_row]
            .spacing(12)
            .padding(16)
            .width(Length::Fixed(360.0));

        // Saved locations list
        if self.config.locations.is_empty() {
            let text = fl!("no-saved-locations");
            col = col.push(widget::text::body(text));
        } else {
            let mut list = cosmic::iced::widget::column![].spacing(0);
            for (i, loc) in self.config.locations.iter().enumerate() {
                if i > 0 {
                    list = list.push(widget::divider::horizontal::light());
                }

                let is_active = i == self.config.active_location_index;

                let label_col =
                    cosmic::iced::widget::column![widget::text::body(loc.name.clone()),].spacing(2);

                let selected = if is_active { Some(i) } else { None };
                let location_radio =
                    widget::radio(label_col, i, selected, Message::ActivateLocation)
                        .width(Length::Fill);

                let delete_btn = widget::button::icon(
                    widget::icon::from_name("edit-delete-symbolic").symbolic(true),
                )
                .on_press(Message::RemoveLocation(i));

                let row = cosmic::iced::widget::row![location_radio, delete_btn,]
                    .spacing(8)
                    .align_y(Alignment::Center)
                    .padding([6, 4]);

                list = list.push(row);
            }
            col = col.push(list);
        }

        col = col.push(widget::divider::horizontal::default());

        // Search section (reuses setup pattern)
        let placeholder = fl!("search-placeholder");
        let search = widget::text_input(placeholder, &self.search_input)
            .on_input(Message::SearchInput)
            .on_submit(|_| Message::SearchSubmit);

        let search_label = fl!("search-button");
        let search_btn = widget::button::suggested(search_label).on_press_maybe(
            if self.search_input.is_empty() {
                None
            } else {
                Some(Message::SearchSubmit)
            },
        );

        let search_row = cosmic::iced::widget::row![search, search_btn]
            .spacing(8)
            .align_y(Alignment::Center);
        col = col.push(search_row);

        if self.searching {
            let text = fl!("searching");
            col = col.push(widget::text::body(text));
        } else if let Some(e) = &self.search_error {
            let text = fl!("search-error", error = e.as_str());
            col = col.push(widget::text::body(text));
        } else if !self.search_results.is_empty() {
            for (i, result) in self.search_results.iter().enumerate() {
                let btn =
                    widget::button::text(&result.display_name).on_press(Message::SelectLocation(i));
                col = col.push(btn);
            }
        } else if self.search_done {
            let text = fl!("no-results");
            col = col.push(widget::text::body(text));
        }

        col.into()
    }

    fn view_main(&self) -> Element<'_, Message> {
        let mut col = cosmic::iced::widget::column![]
            .spacing(12)
            .padding(16)
            .width(Length::Fixed(360.0));

        // --- Header: location name heading + chevron + refresh ---
        let location_name = self
            .config
            .locations
            .get(self.config.active_location_index)
            .map(|loc| loc.name.clone())
            .unwrap_or_else(|| fl!("default-heading"));
        let heading = widget::text::title3(location_name).width(Length::Fill);

        let chevron_btn =
            widget::button::icon(widget::icon::from_name("go-next-symbolic").symbolic(true))
                .on_press(Message::AddLocation);

        let refresh_btn =
            widget::button::icon(widget::icon::from_name("view-refresh-symbolic").symbolic(true))
                .on_press(Message::FetchWeather);

        let header_row = cosmic::iced::widget::row![heading, chevron_btn, refresh_btn]
            .align_y(Alignment::Center)
            .spacing(8);
        col = col.push(header_row);

        // Alert banner
        if !self.alerts.is_empty() {
            let alert_icon = widget::icon::from_name("weather-severe-alert-symbolic")
                .symbolic(true)
                .size(24);

            let mut alert_col = cosmic::iced::widget::column![].spacing(4);
            let heading_text = fl!("alerts-heading");
            alert_col = alert_col.push(widget::text::body(heading_text));
            for alert in &self.alerts {
                alert_col = alert_col.push(widget::text::caption(alert.headline.clone()));
            }

            let alert_row = cosmic::iced::widget::row![alert_icon, alert_col]
                .spacing(8)
                .align_y(Alignment::Start)
                .padding(12)
                .width(Length::Fill);

            let alert_banner = widget::layer_container(alert_row)
                .layer(cosmic::cosmic_theme::Layer::Secondary)
                .width(Length::Fill);
            col = col.push(alert_banner);
        }

        // Error / loading states
        match &self.fetch_state {
            FetchState::Loading if self.forecast.is_none() => {
                let text = fl!("loading");
                col = col.push(widget::text::body(text));
                return col.into();
            }
            FetchState::Error(e) => {
                let text = fl!("fetch-error", error = e.as_str());
                col = col.push(widget::text::body(text));
                if self.forecast.is_some() {
                    let stale = fl!("stale-data");
                    col = col.push(widget::text::caption(stale));
                }
            }
            _ => {}
        }

        if let Some(forecast) = &self.forecast {
            // --- Hero section ---
            if let Some(current) = forecast.periods.first() {
                // Prefer observation data when available, fall back to forecast period
                let (
                    hero_temp,
                    hero_unit,
                    hero_condition,
                    hero_icon_name,
                    hero_wind,
                    hero_humidity,
                    hero_feels_like,
                ) = if let Some(obs) = &self.observation {
                    let temp = obs.temperature.unwrap_or(current.temperature);
                    let unit = &obs.temperature_unit;
                    let cond = obs
                        .condition
                        .clone()
                        .unwrap_or_else(|| current.short_forecast.clone());
                    let icon = obs
                        .condition
                        .as_deref()
                        .map(|c| condition_icon(c, obs.is_daytime))
                        .unwrap_or_else(|| weather_icon_for_period(current));
                    let wind = match (&obs.wind_speed, &obs.wind_direction) {
                        (Some(speed), Some(dir)) => Some(fl!(
                            "wind-info",
                            speed = speed.as_str(),
                            direction = dir.as_str()
                        )),
                        _ => None,
                    };
                    (
                        temp,
                        unit.clone(),
                        cond,
                        icon,
                        wind,
                        obs.humidity,
                        obs.feels_like,
                    )
                } else {
                    let wind = fl!(
                        "wind-info",
                        speed = current.wind_speed.as_str(),
                        direction = current.wind_direction.as_str()
                    );
                    (
                        current.temperature,
                        current.temperature_unit.clone(),
                        current.short_forecast.clone(),
                        weather_icon_for_period(current),
                        Some(wind),
                        None, // humidity
                        None, // feels_like
                    )
                };

                let icon = widget::icon::from_name(hero_icon_name)
                    .symbolic(true)
                    .size(28);

                let temp_label = format!("{}°{hero_unit}", hero_temp);
                let temp_btn = widget::button::custom(widget::text::title3(temp_label))
                    .class(cosmic::widget::button::ButtonClass::Link)
                    .on_press(Message::ToggleUnits);

                let icon_temp_row = cosmic::iced::widget::row![icon, temp_btn]
                    .spacing(12)
                    .align_y(Alignment::Center);

                let hero_uv_index = self.observation.as_ref().and_then(|o| o.uv_index);
                let hero_wind_gusts = self.observation.as_ref().and_then(|o| o.wind_gusts.clone());
                let hero_wind_speed = self.observation.as_ref().and_then(|o| o.wind_speed.clone());
                let hero_wind_dir = self
                    .observation
                    .as_ref()
                    .and_then(|o| o.wind_direction.clone());

                // Muted ink for labels + separators (theme text color at 0.7 alpha).
                // Values render at full strength; the receding labels carry the hierarchy
                // with no bold anywhere (MG2). Escalations still override: AQI color pill
                // at Unhealthy+, bold UV word at Extreme.
                // TODO(i18n): hero labels are literal English here — route through fl! in
                //   the sweep (orphans: feels-like, wind-gusting, precip-info,
                //   humidity-info, uv-info; ", gusting to" is inline).
                let mut muted: Color = cosmic::theme::active().cosmic().background.on.into();
                muted.a = 0.7;

                let mut hero_content = cosmic::iced::widget::column![icon_temp_row]
                    .spacing(2)
                    .padding(12)
                    .width(Length::Fill);

                // Condition · Feels like
                let mut cond = vec![(String::new(), hero_condition)];
                if let Some(f) = hero_feels_like {
//                    cond.push(("Feels like".to_string(), format!("{f}°{hero_unit}")));
                    cond.push((String::new(), format!("Feels like {f}°{hero_unit}")))
                }
                hero_content = hero_content.push(stat_line(muted, cond));




                // Wind (+ optional gusts)
                if let (Some(speed), Some(dir)) = (&hero_wind_speed, &hero_wind_dir) {
                    let mut value = format!("{speed} {dir}");
                    if let Some(g) = &hero_wind_gusts {
                        value.push_str(&format!(", gusting to {g}"));
                    }
                    hero_content =
                        hero_content.push(stat_line(muted, vec![("Wind".to_string(), value)]));
                } else if let Some(wind) = hero_wind {
                    // Fallback (no obs wind components): show the pre-formatted string as-is
                    hero_content = hero_content.push(stat_line(muted, vec![(String::new(), wind)]));
                }

                // Precipitation · Humidity
                let mut ph: Vec<(String, String)> = Vec::new();
                if let Some(p) = &current.probability_of_precipitation {
                    let chance = (p.value.unwrap_or(0.0) as i32).to_string();
                    ph.push(("Precipitation".to_string(), format!("{chance}%")));
                }
                if let Some(h) = hero_humidity {
                    ph.push(("Humidity".to_string(), format!("{h}%")));
                }
                if !ph.is_empty() {
                    hero_content = hero_content.push(stat_line(muted, ph));
                }

                // AQI + UV (health line). Muted labels + values like the other lines;
                // escalations override: colored pill at Unhealthy+ (forces a row), bold
                // value at Extreme UV (a bold span — no row needed).
                let uv: Option<(String, bool)> = hero_uv_index
                    .filter(|u| *u >= 3.0)
                    .map(|u| (format!("{} {}", u.round() as i32, uv_level(u)), u >= 11.0));
                let aqi = self.air_quality.as_ref();

                if aqi.is_some_and(|a| a.severity >= 3) {
                    let a = aqi.unwrap();
                    let sev = a.severity;
                    let label = format!("AQI: {} {}", a.aqi, a.category);
                    let pill: Element<'_, Message> = widget::container(widget::text::body(label))
                        .padding([2, 8])
                        .class(cosmic::theme::Container::custom(move |theme| {
                            let (bg, fg) = aqi_style(sev, theme);
                            cosmic::widget::container::Style {
                                icon_color: None,
                                text_color: Some(fg),
                                background: Some(cosmic::iced::Background::Color(bg)),
                                border: cosmic::iced::Border {
                                    radius: theme.cosmic().radius_s().into(),
                                    ..Default::default()
                                },
                                shadow: cosmic::iced::Shadow::default(),
                                snap: true,
                            }
                        }))
                        .into();
                    let mut health = cosmic::iced::widget::row![pill]
                        .spacing(8)
                        .align_y(Alignment::Center);
                    if let Some((uvs, extreme)) = uv {
                        let mut uv_span = cosmic::iced::widget::span::<(), _>(uvs);
                        if extreme {
                            uv_span = uv_span.font(cosmic::font::bold());
                        }
                        let uv_el: Element<'_, Message> = cosmic::iced::widget::rich_text([
                            cosmic::iced::widget::span::<(), _>("·  UV  ").color(muted),
                            uv_span,
                        ])
                        .into();
                        health = health.push(uv_el);
                    }
                    let health: Element<'_, Message> = health.into();
                    hero_content = hero_content.push(health);
                } else {
                    let mut spans = Vec::new();
                    if let Some(a) = aqi {
                        spans.push(cosmic::iced::widget::span::<(), _>("AQI  ").color(muted));
                        spans.push(cosmic::iced::widget::span::<(), _>(format!(
                            "{} {}",
                            a.aqi, a.category
                        )));
                    }
                    if let Some((uvs, extreme)) = uv {
                        if !spans.is_empty() {
                            spans.push(cosmic::iced::widget::span::<(), _>("  ·  ").color(muted));
                        }
                        spans.push(cosmic::iced::widget::span::<(), _>("UV  ").color(muted));
                        let mut uv_span = cosmic::iced::widget::span::<(), _>(uvs);
                        if extreme {
                            uv_span = uv_span.font(cosmic::font::bold());
                        }
                        spans.push(uv_span);
                    }
                    if !spans.is_empty() {
                        let line: Element<'_, Message> =
                            cosmic::iced::widget::rich_text(spans).into();
                        hero_content = hero_content.push(line);
                    }
                }

                let hero = widget::layer_container(hero_content)
                    .layer(cosmic::cosmic_theme::Layer::Secondary)
                    .width(Length::Fill);

                col = col.push(hero);
            }

            // --- Hourly forecast (paged with arrow buttons) ---
            if !forecast.hourly_periods.is_empty() {
                let total = forecast.hourly_periods.len();
                let offset = self
                    .hourly_offset
                    .min(total.saturating_sub(HOURLY_PAGE_SIZE));
                let end = (offset + HOURLY_PAGE_SIZE).min(total);
                let can_prev = offset > 0;
                let can_next = end < total;

                let prev_arrow: Element<'_, Message> = if can_prev {
                    widget::button::icon(
                        widget::icon::from_name("go-previous-symbolic")
                            .symbolic(true)
                            .size(16),
                    )
                    .on_press(Message::HourlyPrev)
                    .into()
                } else {
                    widget::Space::new().width(Length::Fixed(24.0)).into()
                };

                let mut hourly_row = cosmic::iced::widget::row![].spacing(0);
                for i in offset..end {
                    let period = &forecast.hourly_periods[i];
                    let hour_label = if i == 0 {
                        "Now".to_string()
                    } else {
                        period
                            .start_time
                            .as_deref()
                            .map(format_hour)
                            .unwrap_or_default()
                    };

                    let icon_name = weather_icon_for_period(period);
                    let icon = widget::icon::from_name(icon_name).symbolic(true).size(24);

                    let temp = widget::text::body(format!("{}°", period.temperature));

                    let mut hour_col = cosmic::iced::widget::column![
                        widget::text::caption(hour_label),
                        icon,
                        temp,
                    ]
                    .spacing(4)
                    .align_x(Alignment::Center)
                    .width(Length::Fill);

                    let has_precip_icon = icon_name.contains("showers")
                        || icon_name.contains("storm")
                        || icon_name.contains("snow");
                    if let Some(precip) = period
                        .probability_of_precipitation
                        .as_ref()
                        .and_then(|p| p.value)
                    {
                        let pct = precip as u32;
                        if has_precip_icon || pct >= 20 {
                            hour_col = hour_col.push(widget::text::caption(format!("{}%", pct)));
                        }
                    }

                    hourly_row = hourly_row.push(hour_col);
                }

                let next_arrow: Element<'_, Message> = if can_next {
                    widget::button::icon(
                        widget::icon::from_name("go-next-symbolic")
                            .symbolic(true)
                            .size(16),
                    )
                    .on_press(Message::HourlyNext)
                    .into()
                } else {
                    widget::Space::new().width(Length::Fixed(24.0)).into()
                };

                let paged_row = cosmic::iced::widget::row![prev_arrow, hourly_row, next_arrow]
                    .spacing(4)
                    .align_y(Alignment::Center)
                    .width(Length::Fill);
                col = col.push(paged_row);
            }

            col = col.push(widget::divider::horizontal::default());

            // --- Daily forecast (clickable rows with inline expansion) ---
            {
                let summaries = pair_daily_periods(&forecast.periods);
                let mut rows = cosmic::iced::widget::column![].spacing(0);

                for (i, day) in summaries.iter().enumerate() {
                    if i > 0 {
                        rows = rows.push(widget::divider::horizontal::light());
                    }

                    let is_expanded = self.expanded_day == Some(i);

                    let icon_name = forecast_icon_for_summary(day);
                    let icon = widget::icon::from_name(icon_name).symbolic(true).size(24);

                    let name_text = widget::text::body(day.name.clone()).width(Length::Fill);

                    let temp_str = match (day.high, day.low) {
                        (Some(h), Some(l)) => {
                            format!("{}° / {}°", h, l)
                        }
                        (Some(h), None) => format!("{}°", h),
                        (None, Some(l)) => format!("— / {}°", l),
                        (None, None) => "—".to_string(),
                    };
                    let temp_text = widget::text::body(temp_str);

                    let row_content = cosmic::iced::widget::row![icon, name_text, temp_text]
                        .spacing(8)
                        .align_y(Alignment::Center)
                        .padding([6, 4]);

                    let row_btn = widget::button::custom(row_content)
                        .on_press(Message::ToggleDay(i))
                        .width(Length::Fill)
                        .class(cosmic::theme::Button::Custom {
                            active: Box::new(|_focused, _theme| cosmic::widget::button::Style {
                                background: None,
                                border_width: 0.0,
                                border_color: cosmic::iced::Color::TRANSPARENT,
                                outline_width: 0.0,
                                outline_color: cosmic::iced::Color::TRANSPARENT,
                                icon_color: None,
                                text_color: None,
                                overlay: None,
                                shadow_offset: Default::default(),
                                border_radius: Default::default(),
                            }),
                            disabled: Box::new(|_theme| cosmic::widget::button::Style {
                                background: None,
                                border_width: 0.0,
                                border_color: cosmic::iced::Color::TRANSPARENT,
                                outline_width: 0.0,
                                outline_color: cosmic::iced::Color::TRANSPARENT,
                                icon_color: None,
                                text_color: None,
                                overlay: None,
                                shadow_offset: Default::default(),
                                border_radius: Default::default(),
                            }),
                            hovered: Box::new(|_focused, theme| {
                                let cosmic = theme.cosmic();
                                cosmic::widget::button::Style {
                                    background: Some(cosmic::iced::Background::Color(
                                        cosmic.background.component.hover.into(),
                                    )),
                                    overlay: None,
                                    border_width: 0.0,
                                    border_color: cosmic::iced::Color::TRANSPARENT,
                                    outline_width: 0.0,
                                    outline_color: cosmic::iced::Color::TRANSPARENT,
                                    icon_color: None,
                                    text_color: None,
                                    shadow_offset: Default::default(),
                                    border_radius: cosmic.radius_s().into(),
                                }
                            }),
                            pressed: Box::new(|_focused, theme| {
                                let cosmic = theme.cosmic();
                                cosmic::widget::button::Style {
                                    background: Some(cosmic::iced::Background::Color(
                                        cosmic.background.component.pressed.into(),
                                    )),
                                    overlay: None,
                                    border_width: 0.0,
                                    border_color: cosmic::iced::Color::TRANSPARENT,
                                    outline_width: 0.0,
                                    outline_color: cosmic::iced::Color::TRANSPARENT,
                                    icon_color: None,
                                    text_color: None,
                                    shadow_offset: Default::default(),
                                    border_radius: cosmic.radius_s().into(),
                                }
                            }),
                        });

                    rows = rows.push(row_btn);

                    if is_expanded {
                        let mut detail_col = cosmic::iced::widget::column![].spacing(4);

                        let wind = fl!(
                            "wind-info",
                            speed = day.wind_speed.as_str(),
                            direction = day.wind_direction.as_str()
                        );
                        detail_col = detail_col.push(widget::text::body(wind));

                        if let Some(chance) = day.precip_chance {
                            let chance_str = chance.to_string();
                            let precip = fl!("precip-info", chance = chance_str.as_str());
                            detail_col = detail_col.push(widget::text::body(precip));
                        }

                        if !day.detailed_forecast.is_empty()
                            && day.detailed_forecast != day.short_forecast
                        {
                            detail_col =
                                detail_col.push(widget::text::body(day.detailed_forecast.clone()));
                        }

                        let detail = widget::layer_container(detail_col.padding([4, 16, 8, 36]))
                            .layer(cosmic::cosmic_theme::Layer::Secondary)
                            .width(Length::Fill);

                        rows = rows.push(detail);
                    }
                }

                col = col.push(rows);
            }

            // --- Footer: "Updated X min ago" ---
            if let Some(updated) = self.last_updated {
                let elapsed = updated.elapsed().as_secs() / 60;
                let time_text = if elapsed == 0 {
                    fl!("updated-now")
                } else {
                    let mins = elapsed.to_string();
                    fl!("updated-ago", minutes = mins.as_str())
                };
                col = col.push(widget::text::caption(time_text));
            }
        } else if matches!(self.fetch_state, FetchState::Idle) {
            let text = fl!("no-location");
            col = col.push(widget::text::body(text));
        }

        col.into()
    }
}

fn forecast_icon_for_summary(day: &crate::types::DaySummary) -> &'static str {
    condition_icon(&day.short_forecast, day.is_daytime)
}

// AQI loud tier (bands 3-5) - (background, text). The quiet tier (0-2) blends into
// the card's Secondary surface via the theme (see aqi_style), so it reads as plain
// text and only "grows in" a colored pill as severity rises.
const AQI_FILL_LIGHT: [(Color, Color); 3] = [
    (
        Color::from_rgb8(0xd2, 0x44, 0x44),
        Color::from_rgb8(0xff, 0xff, 0xff),
    ), // 3 Unhealthy/Poor
    (
        Color::from_rgb8(0x8f, 0x3f, 0x97),
        Color::from_rgb8(0xff, 0xff, 0xff),
    ), // 4 Very Unhealthy
    (
        Color::from_rgb8(0x72, 0x2a, 0x35),
        Color::from_rgb8(0xff, 0xff, 0xff),
    ), // 5 Hazardous
];

const AQI_FILL_DARK: [(Color, Color); 3] = [
    (
        Color::from_rgb8(0xb8, 0x3f, 0x3f),
        Color::from_rgb8(0xff, 0xff, 0xff),
    ),
    (
        Color::from_rgb8(0x94, 0x4a, 0x9c),
        Color::from_rgb8(0xff, 0xff, 0xff),
    ),
    (
        Color::from_rgb8(0x8a, 0x3a, 0x48),
        Color::from_rgb8(0xff, 0xff, 0xff),
    ),
];

fn aqi_style(severity: u8, theme: &cosmic::Theme) -> (Color, Color) {
    let cosmic = theme.cosmic();
    if severity >= 3 {
        let table = if cosmic.is_dark {
            &AQI_FILL_DARK
        } else {
            &AQI_FILL_LIGHT
        };
        table[(severity as usize - 3).min(2)]
    } else {
        // Quiet tier: blend into the Secondary surface + normal text -> plain-text look
        (cosmic.secondary.base.into(), cosmic.secondary.on.into())
    }
}

/// One hero detail line from (label, value) pairs: muted labels + full-strength
/// values, with a muted "  ·  " separator between pairs. An empty label emits the
/// value alone (e.g. the weather condition). Spans own their strings → 'static.
fn stat_line(muted: Color, pairs: Vec<(String, String)>) -> Element<'static, Message> {
    use cosmic::iced::widget::{rich_text, span};
    let mut spans = Vec::new();
    for (i, (label, value)) in pairs.into_iter().enumerate() {
        if i > 0 {
            spans.push(span::<(), _>("  ·  ").color(muted));
        }
        if !label.is_empty() {
            spans.push(span::<(), _>(format!("{label}  ")).color(muted));
        }
        spans.push(span::<(), _>(value));
    }
    rich_text(spans).into()
}

// TODO(i18n): route uv levels through fl! (uv-level-* keys)
fn uv_level(uv: f32) -> &'static str {
    if uv < 3.0 {
        "Low" // not reached (caller gates at >=3.0), but keeps the fn total
    } else if uv < 6.0 {
        "Moderate"
    } else if uv < 8.0 {
        "High"
    } else if uv < 11.0 {
        "Very High"
    } else {
        "Extreme"
    }
}
