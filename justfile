install:
    cargo build
    install ./target/debug/instant ~/.local/bin/
    install ./target/debug/instant ~/.local/bin/i

rootinstall:
    cargo build
    sudo install ./target/debug/instant /usr/local/bin/

test:
    DEBUG=1 ./tests/scripts/run_all.sh

