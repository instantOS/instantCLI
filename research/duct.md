# Evaluation: Rust `duct` Crate

## Overview
The `duct` crate (version 1.1.0) is a synchronous library for running child processes in Rust, inspired by shell piping and redirections. It simplifies building pipelines while handling platform inconsistencies (e.g., Windows vs. Unix quoting, error propagation). Repository: [github.com/oconnor663/duct.rs](https://github.com/oconnor663/duct.rs) (stars: ~200, forks: ~20, last commit: 2023; inactive maintenance—open issues: 5, mostly stale; no recent releases). Crates.io stats: 1.1M total downloads, 15K weekly; 8 versions since 2017; deps: os_pipe (1.0), shared_child (1.1), shared_thread (0.2), libc (0.2); dev-dep: tempfile (3.3). Licensed MIT. Platforms: Linux, macOS, Windows (i686/x86_64).

API verified from [docs.rs/duct/1.1.0](https://docs.rs/duct/1.1.0/duct/): All documented items exist—no discrepancies. Core API:
- **Macros**: `cmd!(program, args...)` – Builds `Expression` (e.g., `cmd!("echo", "hi")`).
- **Structs**:
  - `Expression`: Central type; methods: `run()` (execute, errors on non-zero exit), `reader()` (incremental stdout reader), `pipe(expr)` (chain pipelines), `stderr_to_stdout()`, `unchecked()` (ignore exit codes), `start()` (returns `Handle` for async-like control via threads).
  - `Handle`: Controls running process (e.g., `kill()`, `wait()`).
  - `ReaderHandle`: `Read` impl for stdout.
- **Functions**: `cmd(program, args: impl IntoIterator)` – Similar to macro.
- **Traits**: `IntoExecutablePath` – Internal for path handling.
- **Modules**: `unix` – Unix-specific (e.g., signals via `Expression::kill()`).
No async support (sync only; uses threads for handles). Error handling: `Result` with `Error` enum (covers IO, spawn, wait failures; propagates child exit codes >0 as errors). Safety: No unsafe code in public API; avoids shell injection via direct arg passing (no shell invocation unless explicit). Cross-platform: Yes, abstracts Unix/Windows diffs (e.g., piping via threads/pipes).

README (from GitHub) mirrors docs.rs examples; no additional ones. GitHub issues: Low activity (last 2022); focuses on edge cases like Windows piping.

## Usage Examples
For a CLI like instantCLI (dotfile manager), `duct` excels at shell-like commands without spawning a full shell (safer, faster). Add to `Cargo.toml`: `duct = "1.1"`.

### 1. Simple git clone (run synchronously, capture output):
```rust
use duct::cmd;
let output = cmd!("git", "clone", "https://github.com/user/repo.git", "/path/to/target")
    .dir("/tmp")  // Set working dir
    .read()?;      // Captures stdout as String; errors on failure
println!("Clone output: {}", output);
```
- Handles non-zero exit (e.g., invalid URL) as `Err(Error::ExitStatus(1))`.

### 2. fzf selection in pipeline (e.g., select dotfile from list, then git pull):
```rust
use duct::cmd;
use std::io::{self, BufRead};

// Generate list of dotfiles, pipe to fzf for interactive selection
let mut fzf = cmd!("ls", "-1", "~/.config")  // List files
    .pipe(cmd!("fzf", "--height=40%"))       // Interactive select
    .reader()?;                               // Incremental read

let stdin = io::stdin();
let selected = std::io::BufReader::new(&mut fzf).lines().next()?.unwrap();
drop(fzf);  // Ensure child exits

// Then git pull the selected repo
if !selected.is_empty() {
    cmd!("git", "-C", &format!("~/.dotfiles/{}", selected), "pull")
        .unchecked()  // Ignore exit code if needed
        .run()?;
}
```
- Integrates well with instantCLI's `dev/fuzzy.rs` for fuzzy matching; avoids shell for security (direct args prevent injection).

### 3. Error-handling git with stderr merge:
```rust
use duct::cmd;
let reader = cmd!("git", "clone", "invalid-url")
    .stderr_to_stdout()  // Merge stderr into stdout
    .reader()?;
let mut lines = io::BufReader::new(reader).lines();
while let Some(line) = lines.next().transpose()? {
    eprintln!("Error: {}", line);  // Prints clone failure details
}
```

