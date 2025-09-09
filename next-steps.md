# Custom Directory Configuration Analysis and Improvement Plan

## Current State Analysis

### 1. CLI Argument Handling
**Current Implementation:**
- Config directory: `--config` flag (`src/main.rs:20`)
- Database path: `--database` flag (`src/main.rs:24`) - TO BE REMOVED
- No CLI flag for repos directory (configured via config file) - FINAL STATE

**Target Architecture:**
- **Config directory**: CLI-only (as requested)
- **Database directory**: Config-only (mandatory PathBuf with auto tilde handling)
- **Repos directory**: Config-only (mandatory PathBuf with auto tilde handling)
- **All path access**: Direct config field access as PathBuf

### 2. Configuration Management
**Current Structure** (`src/dot/config.rs`):
```rust
pub struct Config {
    pub repos_dir: Option<String>,  // ❌ Should be PathBuf with custom serialization
    // ❌ Missing: database_dir field
}
```

**Current Path Resolution:**
- `config_file_path(custom_path)` - handles custom config path ✅
- `db_path(custom_path)` - only uses CLI argument, ignores config ❌ (TO BE REMOVED)
- `repos_dir(custom_path)` - uses config field `repos_dir` ❌ (TO BE: direct PathBuf access)

### 3. Directory Usage Patterns

**Database Path:**
- `src/main.rs:136`: `Database::new(dot::config::db_path(cli.database.as_deref())?)`
- `src/main.rs:158`: Passed to `dot::add_repo()` as `db_path` parameter
- `src/dot/git.rs:78`: Uses `crate::dot::config::db_path(db_path)?`

**Repos Directory:**
- `src/dot/localrepo.rs:87`: `config::repos_dir(cfg.repos_dir.as_deref())?`
- `src/dot/git.rs:15`: `config::repos_dir(config_manager.config.repos_dir.as_deref())?`
- `src/dot/localrepo.rs:82`: Bug comment indicates should use config but doesn't have access

### 4. Current Defaults
From `src/dot/config.rs`:
- Database default: `dirs::data_dir()?.join("instantos").join("instant.db")`
- Repos default: `dirs::data_dir()?.join("instantos").join("dots")`

## Issues Identified

### 1. String Instead of Path Types
- Current config uses `String` for paths instead of proper `PathBuf`
- Manual tilde expansion required throughout the codebase

### 2. Unnecessary Path Resolution Functions
- `db_path()` and `repos_dir()` functions add unnecessary complexity
- Direct field access with automatic tilde handling would be simpler

### 3. Missing Custom Serialization
- No automatic tilde replacement during serialization
- No automatic tilde expansion during deserialization

### 4. Optional Directory Configuration
- `repos_dir` is `Option<String>` - should be mandatory `PathBuf`
- Missing `database_dir` field entirely

## Improvement Plan

### Phase 1: Create Custom Path Serialization Module

**1. Create Path Serialization Module** (`src/dot/path_serde.rs`)
```rust
use anyhow::Result;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::path::{Path, PathBuf};
use shellexpand;

/// A PathBuf that automatically handles tilde expansion/compression
#[derive(Debug, Clone, PartialEq)]
pub struct TildePath(PathBuf);

impl TildePath {
    pub fn new(path: PathBuf) -> Self {
        TildePath(path)
    }
    
    pub fn as_path(&self) -> &Path {
        &self.0
    }
    
    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }
    
    /// Create from a string with tilde expansion
    pub fn from_str(s: &str) -> Result<Self> {
        let expanded = shellexpand::tilde(s).to_string();
        let path = PathBuf::from(expanded);
        Ok(TildePath(path))
    }
    
    /// Convert to string with tilde compression (replace home dir with ~)
    pub fn to_tilde_string(&self) -> Result<String> {
        let home_dir = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
        
        if let Ok(relative) = self.0.strip_prefix(&home_dir) {
            if relative.as_os_str().is_empty() {
                return Ok("~".to_string());
            }
            return Ok(format!("~{}", relative.display()));
        }
        
        Ok(self.0.to_string_lossy().to_string())
    }
}

impl From<TildePath> for PathBuf {
    fn from(tilde_path: TildePath) -> Self {
        tilde_path.0
    }
}

impl AsRef<Path> for TildePath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Serialize for TildePath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let tilde_string = self.to_tilde_string()
            .map_err(serde::ser::Error::custom)?;
        tilde_string.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for TildePath {
    fn deserialize<D>(deserializer: D) -> Result<TildePath, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        TildePath::from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl Default for TildePath {
    fn default() -> Self {
        TildePath(PathBuf::new())
    }
}
```

### Phase 2: Update Config Structure with PathBuf Fields

**1. Update Config Struct** (`src/dot/config.rs:28`)
```rust
use crate::dot::path_serde::TildePath;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    #[serde(default)]
    pub repos: Vec<Repo>,
    #[serde(default = "default_clone_depth")]
    pub clone_depth: u32,
    #[serde(default = "default_hash_cleanup_days")]
    pub hash_cleanup_days: u32,
    #[serde(default = "default_repos_dir")]
    pub repos_dir: TildePath,  // CHANGED: Option<String> -> TildePath
    #[serde(default = "default_database_dir")]
    pub database_dir: TildePath,  // NEW: mandatory TildePath field
}
```

**2. Add Default Functions** (`src/dot/config.rs`)
```rust
fn default_repos_dir() -> TildePath {
    let default_path = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("instantos")
        .join("dots");
    TildePath::new(default_path)
}

fn default_database_dir() -> TildePath {
    let default_path = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("instantos")
        .join("instant.db");
    TildePath::new(default_path)
}
```

**3. Update Default Implementation** (`src/dot/config.rs:40`)
```rust
impl Default for Config {
    fn default() -> Self {
        Config {
            repos: Vec::new(),
            clone_depth: default_clone_depth(),
            hash_cleanup_days: default_hash_cleanup_days(),
            repos_dir: default_repos_dir(),
            database_dir: default_database_dir(),
        }
    }
}
```

### Phase 3: Add Helper Methods to Config

**1. Add Convenience Methods** (`src/dot/config.rs`)
```rust
impl Config {
    // ... existing methods ...
    
    /// Get the database path as a PathBuf
    pub fn database_path(&self) -> &Path {
        self.database_dir.as_path()
    }
    
    /// Get the repos directory as a PathBuf
    pub fn repos_path(&self) -> &Path {
        self.repos_dir.as_path()
    }
    
    /// Ensure all directory paths exist
    pub fn ensure_directories(&self) -> Result<()> {
        if let Some(parent) = self.database_path().parent() {
            fs::create_dir_all(parent).context("creating database directory")?;
        }
        
        fs::create_dir_all(self.repos_path()).context("creating repos directory")?;
        Ok(())
    }
}
```

### Phase 4: Remove Path Resolution Functions

**1. Remove db_path() Function** (`src/dot/config.rs:176`)
- Delete entire `db_path()` function
- Replace all usage with direct `config.database_path()` access

**2. Remove repos_dir() Function** (`src/dot/config.rs:193`)
- Delete entire `repos_dir()` function  
- Replace all usage with direct `config.repos_path()` access

### Phase 5: Update Usage Throughout Codebase

**1. Update Main Function** (`src/main.rs:136`)
```rust
// Remove CLI database path, use config directly
config_manager.config.ensure_directories()?;
let db = match Database::new(config_manager.config.database_path().to_path_buf()) {
    Ok(db) => db,
    Err(e) => {
        eprintln!(
            "{}: {}",
            "Error opening database".red(),
            e.to_string().red()
        );
        return Err(e);
    }
};
```

**2. Update LocalRepo Methods** (`src/dot/localrepo.rs:81`)
```rust
pub fn local_path(&self, cfg: &Config) -> Result<PathBuf> {
    Ok(cfg.repos_path().join(&self.name))
}
```

**3. Update Git Operations** (`src/dot/git.rs:15`)
```rust
let base = config_manager.config.repos_path();
```

**4. Remove Legacy CLI Database Flag**
- Remove `database: Option<String>` from `Cli` struct (`src/main.rs:24`)
- Remove all `cli.database.as_deref()` calls
- Remove `db_path` parameter from all function signatures

### Phase 6: Update Tests

**1. Modify Test Config** (`tests/scripts/test_utils.sh:28`)
```bash
# Create initial config with portable directory paths
cat > "$CONFIG_FILE" << EOF
repos_dir = "~/instant-test/repos"
database_dir = "~/instant-test/instant.db"
clone_depth = 1
EOF
```

**2. Update Test Execution** (`tests/scripts/test_utils.sh:110`)
```bash
# Remove --database flag, use config instead
HOME="$HOME_DIR" "$binary_path" --config "$CONFIG_FILE" "$@"
```

### Phase 7: Add Module Integration

**1. Update Module Exports** (`src/dot/mod.rs`)
```rust
mod path_serde;
pub use path_serde::TildePath;
```

**2. Add Unit Tests for Path Serialization** (`src/dot/path_serde.rs`)
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    
    #[test]
    fn test_tilde_expansion() {
        let tilde_path = TildePath::from_str("~/test/path").unwrap();
        let expanded = tilde_path.as_path();
        assert!(expanded.to_string_lossy().contains(std::env::var("HOME").unwrap()));
    }
    
    #[test]
    fn test_tilde_compression() {
        let home = dirs::home_dir().unwrap();
        let test_path = home.join("test").join("path");
        let tilde_path = TildePath::new(test_path);
        let compressed = tilde_path.to_tilde_string().unwrap();
        assert_eq!(compressed, "~/test/path");
    }
    
    #[test]
    fn test_serialization_roundtrip() {
        let original = TildePath::from_str("~/test/path").unwrap();
        let serialized = serde_json::to_string(&original).unwrap();
        let deserialized: TildePath = serde_json::from_str(&serialized).unwrap();
        assert_eq!(original.as_path(), deserialized.as_path());
    }
}
```

## Implementation Priority

1. **High**: Create TildePath serialization module with tests
2. **High**: Update Config struct to use TildePath fields
3. **High**: Add convenience methods to Config
4. **High**: Remove path resolution functions and update direct usage
5. **Medium**: Remove CLI database flag and update all function calls
6. **Medium**: Update tests to use portable tilde-based config
7. **Low**: Add directory creation helpers and validation

## Testing Strategy

1. **Unit Tests**: Test TildePath serialization/deserialization thoroughly
2. **Integration Tests**: Test full workflow with portable config files
3. **Roundtrip Tests**: Ensure serialize/deserialize preserves paths correctly
4. **Cross-platform Tests**: Test tilde handling on different platforms
5. **Migration Tests**: Test upgrading from old config format

## Benefits of This Approach

1. **Type Safety**: PathBuf fields instead of strings with compile-time path safety
2. **Transparency**: Automatic tilde handling is invisible to code using the config
3. **Portability**: Config files use `~` and work across different machines
4. **Simplicity**: Direct field access, no manual path expansion needed
5. **Maintainability**: Custom serialization logic is isolated in one module
6. **User Experience**: Config files are easily shareable and human-readable
7. **Clean Architecture**: Zero abstraction overhead for path operations