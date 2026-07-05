mod app;
pub mod backend;
mod config;
mod geocoding;
mod i18n;
mod nws;
mod types;

fn main() -> cosmic::iced::Result {
    let requested_languages = i18n_embed::DesktopLanguageRequester::requested_languages();
    i18n::init(&requested_languages);
    cosmic::applet::run::<app::AppModel>(())
}
