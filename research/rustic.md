# rustic_core Research

## Overview

`rustic_core` is a Rust library that provides fast, encrypted, deduplicated backups. It reads and writes the `restic` repository format, making it compatible with the restic backup ecosystem. The library powers the `rustic-rs` backup tool and is currently in early development stage with APIs subject to change.

## Key Features

- **Fast, encrypted, deduplicated backups**
- **Restic-compatible repository format**
- **Repository initialization and management**
- **Snapshot creation and management**
- **Backup and restore operations**
- **Repository checking and repair**
- **Multiple backend support** (local filesystem, cloud storage, etc.)
- **Content-addressable storage**

## Crate Dependencies

```toml
[dependencies]
rustic_core = "0.4"  # Current version in project
# Also requires rustic_backend for backend operations
rustic_backend = "0.4"
```

## Core API Components

### Main Types

#### `Repository` - Primary Repository Interface
The main type that represents a repository in different states and allows various operations:

```rust
use rustic_core::{Repository, RepositoryOptions};
use rustic_backend::BackendOptions;

// Create repository options
let repo_opts = RepositoryOptions::default()
    .password("your_password");

// Initialize backends
let backends = BackendOptions::default()
    .repository("/path/to/repo")
    .to_backends()?;

// Create repository instance
let repo = Repository::new(&repo_opts, &backends)?;
```

#### Repository States
The `Repository` can be in different states, each enabling different operations:

- **Open**: Password verified, decryption key available
- **IndexedTree**: Tree blobs indexed
- **IndexedIds**: Tree and data blob IDs indexed
- **IndexedFull**: All blob information fully indexed

```rust
// Open existing repository
let open_repo = Repository::new(&repo_opts, &backends)?.open()?;

// Convert to indexed state for backup operations
let indexed_repo = open_repo.to_indexed_ids()?;
```

### Configuration Options

#### `RepositoryOptions`
```rust
let opts = RepositoryOptions::default()
    .password("password")           // Repository password
    .repository("/path/to/repo");   // Repository path
```

#### `BackendOptions`
```rust
let backends = BackendOptions::default()
    .repository("/path/to/repo")      // Main repository
    .repo_hot("/path/to/hot")         // Hot storage (optional)
    .to_backends()?;
```

#### `SnapshotOptions`
```rust
let snapshot = SnapshotOptions::default()
    .add_tags("game_saves,important")?  // Add tags
    .host("my-gaming-pc")?             // Set hostname
    .to_snapshot()?;
```

## Key Operations

### 1. Repository Initialization

```rust
use rustic_core::{ConfigOptions, KeyOptions, Repository, RepositoryOptions};

fn init_repo() -> Result<(), Box<dyn std::error::Error>> {
    let repo_opts = RepositoryOptions::default()
        .repository("/tmp/repo")
        .password("your_password");

    let backends = BackendOptions::default()
        .repository("/tmp/repo")
        .to_backends()?;

    let key_opts = KeyOptions::default();
    let config_opts = ConfigOptions::default();

    let repo = Repository::new(&repo_opts, &backends)?
        .init(&key_opts, &config_opts)?;

    println!("Repository initialized successfully!");
    Ok(())
}
```

### 2. Creating Snapshots (Backups)

```rust
use rustic_core::{BackupOptions, PathList, Repository, RepositoryOptions, SnapshotOptions};

fn create_backup() -> Result<(), Box<dyn std::error::Error>> {
    // Open repository
    let repo_opts = RepositoryOptions::default()
        .repository("/tmp/repo")
        .password("your_password");

    let backends = BackendOptions::default()
        .repository("/tmp/repo")
        .to_backends()?;

    let repo = Repository::new(&repo_opts, &backends)?
        .open()?
        .to_indexed_ids()?;  // Required for backup operations

    // Configure backup options
    let backup_opts = BackupOptions::default();

    // Specify source paths (game save directories)
    let source = PathList::from_string("~/Documents/MyGame/saves,~/.config/mygame")?
        .sanitize()?;

    // Configure snapshot metadata
    let snap = SnapshotOptions::default()
        .add_tags("game_saves,mygame")?
        .host("gaming-pc")?
        .to_snapshot()?;

    // Create the backup
    let snapshot = repo.backup(&backup_opts, &source, snap)?;
    println!("Backup created: {:?}", snapshot.id());

    Ok(())
}
```

### 3. Listing Snapshots

```rust
fn list_snapshots() -> Result<(), Box<dyn std::error::Error>> {
    let repo = open_repository()?;  // Helper to open repo

    let snapshots = repo.get_all_snapshots()?;

    for snap in snapshots {
        println!("Snapshot ID: {}", snap.id());
        println!("  Time: {}", snap.time());
        println!("  Host: {}", snap.host());
        println!("  Tags: {:?}", snap.tags());
        println!("  Paths: {:?}", snap.paths());
        println!();
    }

    Ok(())
}
```

### 4. Restoring Snapshots

```rust
use rustic_core::{LocalDestination, LsOptions, Repository, RepositoryOptions, RestoreOptions};

fn restore_snapshot() -> Result<(), Box<dyn std::error::Error>> {
    let repo_opts = RepositoryOptions::default()
        .repository("/tmp/repo")
        .password("your_password");

    let backends = BackendOptions::default()
        .repository("/tmp/repo")
        .to_backends()?;

    let repo = Repository::new(&repo_opts, &backends)?
        .open()?
        .to_indexed()?;

    // Get node from snapshot (use "latest" for most recent)
    let node = repo.node_from_snapshot_path("latest", |_| true)?;

    // List snapshot contents
    let ls_opts = LsOptions::default();
    let file_list = repo.ls(&node, &ls_opts)?;

    // Configure restore destination
    let destination = "~/Documents/RestoredGameSaves/";
    let dest = LocalDestination::new(destination, true, !node.is_dir())?;

    // Prepare restore
    let restore_opts = RestoreOptions::default();
    let restore_infos = repo.prepare_restore(&restore_opts, file_list.clone(), &dest, false)?;

    // Perform restore
    repo.restore(restore_infos, &restore_opts, file_list, &dest)?;

    println!("Restore completed successfully!");
    Ok(())
}
```

### 5. Repository Checking and Maintenance

```rust
use rustic_core::{CheckOptions, PruneOptions, Repository, RepositoryOptions};

fn check_repository() -> Result<(), Box<dyn std::error::Error>> {
    let repo = open_repository()?;

    // Check repository integrity
    let check_opts = CheckOptions::default()
        .trust_cache(true)  // Skip cache checks for speed
        .read_data_subset(); // Optional: limit data reading

    repo.check(check_opts)?;
    println!("Repository check completed successfully!");

    Ok(())
}

fn prune_repository() -> Result<(), Box<dyn std::error::Error>> {
    let repo = open_repository()?;

    let prune_opts = PruneOptions::default()
        .max_unused(10);  // Keep up to 10% unused data

    let plan = repo.prune_plan(prune_opts)?;
    println!("Prune plan: {:?}", plan.stats());

    // Execute the prune plan
    repo.prune(prune_opts, plan)?;
    println!("Repository pruning completed!");

    Ok(())
}
```

## Game Save Manager Implementation Considerations

### 1. Repository Structure
```
~/.local/share/instantos/games/repo/    # Main restic repository
├── config                                # Repository config
├── data/                                # Encrypted data blobs
├── index/                               # Index files
├── keys/                                # Encryption keys
├── snapshots/                           # Snapshot metadata
└── locks/                               # Repository locks
```

### 2. Snapshot Organization
For game saves, organize snapshots using:
- **Tags**: `game_saves`, `<game_name>`, `manual`, `auto`
- **Host**: Gaming PC identifier
- **Paths**: Specific game save directories
- **Parent snapshots**: For incremental backup support

### 3. Game-Specific Operations

#### Per-Game Snapshots
```rust
fn backup_game_saves(game_name: &str, save_paths: &[&str]) -> Result<SnapshotFile, RusticError> {
    let snap_opts = SnapshotOptions::default()
        .add_tags(&format!("game_saves,{}", game_name))?
        .to_snapshot()?;

    let path_list = PathList::from_strings(save_paths)?.sanitize()?;

    repo.backup(&BackupOptions::default(), &path_list, snap_opts)
}
```

#### Selective Restore by Game
```rust
fn restore_game_saves(game_name: &str, target_dir: &str) -> Result<(), RusticError> {
    // Find snapshots with specific game tag
    let snapshots: Vec<SnapshotFile> = repo.get_all_snapshots()?
        .into_iter()
        .filter(|snap| snap.tags().contains(&game_name.to_string()))
        .collect();

    // Use most recent snapshot for that game
    if let Some(latest_snapshot) = snapshots.first() {
        let node = repo.node_from_snapshot(&latest_snapshot.id)?;
        // ... restore logic
    }

    Ok(())
}
```

### 4. Error Handling
```rust
use rustic_core::RusticError;

pub enum GameSaveError {
    RepositoryError(RusticError),
    GameNotConfigured(String),
    SavePathNotFound(String),
    // ... other error types
}

impl From<RusticError> for GameSaveError {
    fn from(err: RusticError) -> Self {
        GameSaveError::RepositoryError(err)
    }
}
```

## Performance Considerations

1. **Indexing**: Use `to_indexed_ids()` for backup operations to improve performance
2. **Caching**: Repository maintains caches for faster operations
3. **Parallel operations**: Some operations support parallel processing
4. **Hot storage**: Optional hot storage backend for frequently accessed data

## Crate Features

- **cli**: Enables CLI features (disabled by default)
- **merge**: Enables config merging capabilities
- **clap**: Command-line argument parsing
- **webdav**: WebDAV server support

## Minimum Requirements

- **Rust version**: 1.68.2+
- **Dependencies**: See Cargo.toml for full dependency tree
- **Backend**: At least one storage backend (local filesystem included)

## License

Dual-licensed under:
- Apache License, Version 2.0
- MIT License

## Current Status

**Note**: `rustic_core` is in early development. APIs are subject to change in future releases. The library is actively maintained and has a growing community around the rustic backup tool.

## Useful Links

- [GitHub Repository](https://github.com/rustic-rs/rustic_core)
- [API Documentation](https://docs.rs/rustic_core)
- [Main Project](https://rustic.cli.rs/)
- [Discussions](https://github.com/rustic-rs/rustic/discussions)