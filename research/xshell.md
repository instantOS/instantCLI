# xshell Crate Research

## Overview
xshell is a Rust crate designed for writing cross-platform shell scripts in Rust. It reimplements parts of a scripting environment to provide ergonomics similar to bash or Python glue code, but with Rust's safety guarantees. Key features include:
- No shell injection risks.
- Automatic handling of directories, environment variables, and command execution.
- Fast compile times and minimal dependencies.
- Inspired by Julia's shelling out philosophy.

Repository: [matklad/xshell](https://github.com/matklad/xshell)  
Latest Version: 0.2.7  
License: MIT/Apache-2.0  
Docs: [docs.rs/xshell](https://docs.rs/xshell/latest/xshell/)

## Usage
xshell centers around the `Shell` struct and the `cmd!` macro.

### Basic Setup
```rust
use xshell::{Shell, cmd};
use anyhow::Result;

fn main() -> Result<()> {
    let sh = Shell::new()?;
    // Use sh for operations
    Ok(())
}
```

### Running Commands
The `cmd!` macro interpolates variables safely:
```rust
let sh = Shell::new()?;
let user = "matklad";
let repo = "xshell";
cmd!(sh, "git clone https://github.com/{user}/{repo}.git").run()?;
```

- `run()` executes the command and prints output.
- Use `.quiet()` to suppress echoing.
- Supports splat interpolation for iterables: `{args...}`.

### Directory and Environment Management
```rust
sh.change_dir("repo_dir");  // Change working directory
let _guard = sh.push_dir("temp");  // RAII for temporary dir change
let manifest = sh.read_file("Cargo.toml")?;  // Read file relative to current dir
sh.write_file("output.txt", "content")?;  // Write file, creates parents if needed
```

- `push_dir()` and `push_env()` return RAII guards to restore state.
- `TempDir` for temporary directories.

### Example: Clone, Test, and Publish
From docs:
```rust
use xshell::{cmd, Shell};

fn main() -> anyhow::Result<()> {
    let sh = Shell::new()?;
    let user = "matklad";
    let repo = "xshell";
    cmd!(sh, "git clone https://github.com/{user}/{repo}.git").run()?;
    sh.change_dir(repo);

    let test_args = ["-Zunstable-options", "--report-time"];
    cmd!(sh, "cargo test -- {test_args...}").run()?;

    let manifest = sh.read_file("Cargo.toml")?;
    let version = // parse version from manifest
    cmd!(sh, "git tag {version}").run()?;

    let dry_run = if sh.var("CI").is_ok() { None } else { Some("--dry-run") };
    cmd!(sh, "cargo publish {dry_run...}").run()?;
    Ok(())
}
```

## Relevance to instantCLI Project
instantCLI appears to be a CLI tool for managing dotfiles, git repositories, and system configurations (based on src/dot/, src/doctor/, and tests/scripts/). xshell could enhance this project by:

- **Scripting Automation**: Replace or augment bash scripts in `tests/scripts/` (e.g., `run_all.sh`, `test_init.sh`) with Rust-based scripts using xshell for reliable cross-platform execution. This avoids shell-specific issues and improves error handling.

- **Git and File Operations**: In modules like `src/dot/repo/git.rs` or `src/dot/utils.rs`, use xshell for safe command execution (e.g., git clones, tags) without raw `std::process::Command`, reducing injection risks and improving portability.

- **Testing and CI**: Integrate into test suites for dynamic environment setup/teardown (e.g., temporary dirs for `test_bug_identical_files.sh`). Could simplify `justfile` tasks or add Rust-based build scripts.

- **Doctor Checks**: In `src/doctor/checks.rs`, xshell could run system commands (e.g., checking git status) with better isolation and output capture.

Adding xshell as a dependency in `Cargo.toml` would enable these without external shell reliance, aligning with Rust's safety focus. Potential drawback: Adds a dependency, but it's lightweight (~10KB binary size increase per docs).

## Recommendations
- Evaluate integration in a proof-of-concept script for one test (e.g., `test_utils.sh`).
- MSRV: 1.63.0 (compatible with current Rust).
- Related: Compare with `duct` for advanced process piping if needed.