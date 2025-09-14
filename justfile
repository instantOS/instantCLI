install:
    cargo build
    install ./target/debug/instant ~/.local/bin/
    install ./target/debug/instant ~/.local/bin/i

test:
    DEBUG=1 ./tests/scripts/run_all.sh

