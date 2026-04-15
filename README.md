# Whether

A weather applet for the [COSMIC](https://github.com/pop-os/cosmic-epoch) desktop panel.

![Whether applet screenshot](screenshots/whether.png)

## Features

- Current conditions, hourly forecast, and 7-day daily forecast
- Click a day to expand wind, precipitation, and detailed forecast info
- Dual weather sources:
  - **NWS** (National Weather Service) — US locations, free, no API key
  - **Open-Meteo** — worldwide, free ([CC BY 4.0](https://open-meteo.com/en/license))
- Location search powered by [Nominatim](https://nominatim.openstreetmap.org/) (OpenStreetMap)
- Multiple saved locations with per-location source toggle
- Temperature unit toggle (click the hero temperature to switch between °F and °C)
- Panel displays weather icon and current temperature

## Installation

Download the latest release from the [Releases](https://github.com/nwxnw/cosmic-ext-whether/releases) page.

### Debian/Ubuntu (.deb)

```sh
sudo apt install ./cosmic-ext-whether_0.2.0_amd64.deb
```

### Flatpak

```sh
flatpak install --user ./cosmic-ext-whether_0.2.0.flatpak
```

### From source

Requires Rust, [just](https://github.com/casey/just), and system dependencies:

```sh
sudo apt install libxkbcommon-dev wayland-protocols libwayland-dev
```

```sh
just build-release
sudo just install
```

Then add **Whether** to your COSMIC panel via Settings > Desktop > Panel > Applets.

### Uninstall

```sh
sudo just uninstall
```

## License

GPL-3.0-only
