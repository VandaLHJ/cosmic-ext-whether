name := 'cosmic-ext-whether'
export APPID := 'com.github.nwxnw.cosmic-ext-whether'

rootdir := ''
prefix := '/usr'

base-dir := absolute_path(clean(rootdir / prefix))
export INSTALL_DIR := base-dir / 'share'

bin-src := 'target' / 'release' / name

# Configurable paths (overridden by env vars during flatpak builds)
bin_dir := env_var_or_default("BIN_DIR", base-dir / 'bin')
app_dir := env_var_or_default("APP_DIR", INSTALL_DIR / 'applications')
metainfo_dir := env_var_or_default("METAINFO_DIR", INSTALL_DIR / 'metainfo')
icon_dir := env_var_or_default("ICON_DIR", INSTALL_DIR / 'icons' / 'hicolor' / 'scalable' / 'apps')

desktop := APPID + '.desktop'
desktop-src := 'data' / desktop
metainfo-src := 'data' / APPID + '.metainfo.xml'
wrapper-src := 'data' / APPID + '.sh'
icon-src := 'data' / APPID + '-symbolic.svg'

default: build-release

build-release *args:
    cargo build --release {{args}}

build-debug *args:
    cargo build {{args}}

run *args:
    cargo build --release {{args}} && RUST_BACKTRACE=1 {{bin-src}}

install:
    install -Dm0755 {{bin-src}} {{bin_dir}}/{{name}}
    install -Dm0755 {{wrapper-src}} {{bin_dir}}/{{name}}.sh
    install -Dm0644 {{desktop-src}} {{app_dir}}/{{APPID}}.desktop
    install -Dm0644 {{metainfo-src}} {{metainfo_dir}}/{{APPID}}.metainfo.xml
    install -Dm0644 {{icon-src}} {{icon_dir}}/{{APPID}}-symbolic.svg

uninstall:
    rm -f {{bin_dir}}/{{name}}
    rm -f {{bin_dir}}/{{name}}.sh
    rm -f {{app_dir}}/{{APPID}}.desktop
    rm -f {{metainfo_dir}}/{{APPID}}.metainfo.xml
    rm -f {{icon_dir}}/{{APPID}}-symbolic.svg

clean:
    cargo clean

check:
    cargo clippy --all-features
