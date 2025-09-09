install:
    cargo build
    install ./target/debug/instant ~/.local/bin/

test:
    DEBUG=1 ./tests/scripts/run_all.sh

