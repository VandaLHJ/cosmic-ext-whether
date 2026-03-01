name := 'cosmic-ext-whether'
export APPID := 'com.github.nwxnw.cosmic-ext-whether'

rootdir := ''
prefix := '/usr'

base-dir := absolute_path(clean(rootdir / prefix))
export INSTALL_DIR := base-dir / 'share'

bin-src := 'target' / 'release' / name
bin-dst := base-dir / 'bin' / name

desktop := APPID + '.desktop'
desktop-src := 'data' / desktop
desktop-dst := clean(INSTALL_DIR / 'applications' / desktop)

default: build-release

build-release *args:
    cargo build --release {{args}}

build-debug *args:
    cargo build {{args}}

run *args:
    cargo build --release {{args}} && RUST_BACKTRACE=1 {{bin-src}}

install:
    install -Dm0755 {{bin-src}} {{bin-dst}}
    install -Dm0644 {{desktop-src}} {{desktop-dst}}

uninstall:
    rm -f {{bin-dst}}
    rm -f {{desktop-dst}}

clean:
    cargo clean

check:
    cargo clippy --all-features
