# Test isolation

Run Rust tests with `cargo test` and shell integration tests with
`./tests/run_all.sh` (or `just test`). The shell runner builds one debug binary
and uses `jq` to read its path from Cargo. Standalone shell tests prepare it
before changing `HOME`. A failed build stops the tests.

Rust filesystem tests must own a `tempfile::TempDir` and keep every write under
it. Do not reuse fixed paths in the user's home or temporary directory. The
installer configuration tests use a thread-local `TempDir`, destroyed when the
test thread exits, including on panic. Mocking commands alone does not isolate
ordinary filesystem operations.

Tests that override environment variables use `dot::test_util::EnvGuard` to
restore the previous value on success or panic and carry `#[serial_test::serial]`.
The repository also defaults to one test thread. Subprocess-specific overrides
should use `Command::env` so they do not change the test runner's environment.

Shell tests source `helpers.sh`, call `setup_test_env`, and immediately install
`trap cleanup_test_env EXIT`. The helper isolates home, XDG state/config/data/
cache/runtime paths, temporary files, GPG and password stores, and Git's global
configuration. It disables game GUI prompts, clears display detection, and
points D-Bus at nonexistent sockets inside the fixture. Tests exercising desktop
integration explicitly select fake displays and provide stub commands.

Any subprocess started by a test needs cleanup on errors and panics as well as
success. The recorder wrapper test owns an input pipe: closing it ends its
`cat` process even if PID reporting fails.

Ignored integration tests describe their prerequisites. Run them by exact name;
do not enable every ignored test on a development host. Arch CI explicitly runs
the pacman keyring configuration test. The loop-device/mount test needs a
disposable privileged environment, and the bootstrap authentication test downloads
an official Arch archive.
