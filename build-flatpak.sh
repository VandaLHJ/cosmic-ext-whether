#!/bin/bash
set -e

# Get metadata from Cargo.toml
NAME=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].name')
VERSION=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].version')
APPID=com.github.nwxnw.cosmic-ext-whether

# Build release binary (also needed for cargo vendor to resolve all deps)
cargo build --release

# Generate vendored crates and a matching cargo config
mkdir -p .cargo
cargo vendor > .cargo/config.toml

# Verify offline metadata works
cargo metadata --offline --format-version 1 >/dev/null

# Uninstall old version if present
flatpak uninstall $APPID -y 2>/dev/null || true

# Build with flatpak-builder
flatpak-builder --force-clean --repo=repo build-dir $APPID.json

# Bundle into a single .flatpak file
flatpak build-bundle repo ${NAME}_${VERSION}.flatpak $APPID

echo "Created ${NAME}_${VERSION}.flatpak"
