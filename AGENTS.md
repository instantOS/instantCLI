# AGENTS.md

Guidance for automated agents in this repository.
- Build/check after changes: `cargo check` (preferred) or `cargo build` (debug only; avoid `--release`).
- Single binary build uses `cargo build --bin ins`; jobs pinned by `.cargo/config.toml`.
- Lint/format: `cargo fmt`, `cargo clippy` (fixes via `cargo clippy --fix --allow-dirty`), `shfmt -w *.sh`, `yamlfmt .github`.
- Tests default to 1 thread (`.cargo/config.toml`); run `cargo test` for unit/integration.
- Run shell E2E suite with `just test` (calls `./tests/run_all.sh`).
- Run a single Rust test with `cargo test <module_or_case>` (respects test-threads=1).
- Avoid test flakiness by using prepared `INS_BIN` helpers in `tests/helpers.sh`.
- Rust style: edition 2024, prefer `anyhow::Result` or typed errors via `thiserror`; use `?` over `unwrap`/`expect`.
- Imports: group std/prelude, third-party, crate; avoid unused `pub` exposures; keep `use crate::...` paths concise.
- Error handling: bubble errors with context (`anyhow::Context`); log user-facing messages, avoid silent failures.
- Concurrency: respect single-thread test config; avoid spawning unnecessary threads.
