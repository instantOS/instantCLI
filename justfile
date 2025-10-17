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

format:
    yamlfmt .github
