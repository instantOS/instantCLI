install:
    cargo build
    install ./target/debug/ins ~/.local/bin/
    install ./target/debug/ins ~/.local/bin/i

rootinstall:
    cargo build
    sudo install ./target/debug/ins /usr/local/bin/

test:
    ./tests/run_all.sh

appimage:
    ./utils/build_appimage.sh

stuff:
    cargo build --profile upload
    rsync ./target/upload/ins ubuntu@stuff.paperbenni.xyz:/data/stuff/ins
    rsync ./scripts/archinstall.sh ubuntu@stuff.paperbenni.xyz:/data/stuff/install
    rsync ./scripts/dev.sh ubuntu@stuff.paperbenni.xyz:/data/stuff/dev

format:
    yamlfmt .github
    cargo clippy --fix --allow-dirty
    cargo fmt
    find . -name "*.sh" -exec shfmt -w {} \;
