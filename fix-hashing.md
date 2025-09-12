# Hash Refactor Plan

## Current State Analysis

### Current Database Schema (Incorrect)
```sql
CREATE TABLE hashes (
    created TEXT NOT NULL,
    hash TEXT NOT NULL,
    path TEXT NOT NULL,
    unmodified INTEGER NOT NULL,  -- ❌ INCORRECT - doesn't track source vs target
    PRIMARY KEY (hash, path)
)
```

### Current Implementation Problems Found

**Database Issues:**
- `unmodified` field misnamed - should track source vs target origin
- Methods like `unmodified_hash_exists()` and `get_unmodified_hashes()` are misleading
- `cleanup_hashes()` has complex logic trying to preserve "unmodified" status

**Logic Issues in `src/dot/dotfile.rs`:**
1. **Lines 54-71**: `is_modified()` uses complex logic with `get_unmodified_hashes()` that's incorrect
2. **Lines 73-110**: `get_target_hash()` has convoluted logic trying to infer unmodified status
3. **Lines 112-134**: `get_source_hash()` similarly complex with unmodified checking
4. **Lines 179-207**: `apply()` method trusts the broken modification detection
5. **Lines 209-215**: `fetch()` method uses broken modification detection
6. **Lines 220-234**: `reset()` method incorrectly marks targets as "unmodified"

**Incorrect Hash Storage Patterns Found:**
- `src/dot/git.rs:69`: `db.add_hash(&source_hash, &dotfile.source_path, true)` - ❌ Should use `source_file=true`
- `src/dot/git.rs:78`: `db.add_hash(&target_hash, &dotfile.target_path, true)` - ❌ Should use `source_file=false`
- `src/dot/dotfile.rs:204`: `db.add_hash(&source_hash, &self.target_path, true)` - ❌ Should use `source_file=false`
- `src/dot/dotfile.rs:231`: `db.add_hash(&source_hash, &self.target_path, true)` - ❌ Should use `source_file=false`
- `src/dot/dotfile.rs:252-253`: Both marked as `true` - ❌ Target should be `source_file=false`

**Files Using Broken `is_modified()` Logic:**
- `src/dot/mod.rs:211,217,328`: Used in `get_modified_dotfiles()` and `reset_modified()`
- `src/dot/git.rs:453,492,504`: Used in status reporting
- `src/dot/dotfile.rs:180,210`: Used in `apply()` and `fetch()` operations

## Proposed New System

### New Database Schema
```sql
CREATE TABLE file_hashes (
    created TEXT NOT NULL,
    hash TEXT NOT NULL,
    path TEXT NOT NULL,
    source_file INTEGER NOT NULL,  -- ✅ CORRECT - true=source file, false=target file
    PRIMARY KEY (hash, path)
)
```

### Core Concepts

1. **Source Files**: Files in the dotfile repository (`~/.local/share/instantos/dots/`)
2. **Target Files**: Files in the home directory (`~/`)
3. **Hash Tracking**: Each hash entry explicitly tracks whether it came from source or target
4. **Lazy Hash Computation**: Hashes are computed on-demand and cached with timestamp validation

### New Hash Management Logic

#### `get_file_hash(path: &Path, is_source: bool) -> Result<String>`
- Lazy hash computation with caching
- Checks if existing hash is newer than file modification time
- Computes and stores hash if needed or outdated
- Always sets `source_file` field based on parameter

#### `is_target_unmodified(target_path: &Path) -> Result<bool>`
New logic to determine if target file is safe to override:
1. Get target file hash using `get_file_hash(target_path, false)`
2. Check if this hash exists in DB with `source_file = true` (any source file)
3. If found, file is unmodified (created by instantCLI, not touched by user)
4. If not found, get current source file hash using `get_file_hash(source_path, true)`
5. If hashes match, file is unmodified (matches current source)
6. Otherwise, file is modified (user has changed it)

## Precise Implementation Steps

### 1. Database Schema Migration (`src/dot/db.rs`)

**Struct Changes:**
```rust
// OLD:
pub struct DotfileHash {
    pub hash: String,
    pub created: DateTime<Utc>,
    pub path: String,
    pub unmodified: bool,  // REMOVE
}

// NEW:
pub struct FileHash {
    pub hash: String,
    pub created: DateTime<Utc>,
    pub path: String,
    pub source_file: bool,  // true=source, false=target
}
```

**Schema Migration:**
```sql
-- Drop old table and create new one (clean break approach)
DROP TABLE IF EXISTS hashes;
CREATE TABLE file_hashes (
    created TEXT NOT NULL,
    hash TEXT NOT NULL,
    path TEXT NOT NULL,
    source_file INTEGER NOT NULL,
    PRIMARY KEY (hash, path)
);
```

**Method Changes:**
- `add_hash(hash, path, unmodified)` → `add_hash(hash, path, source_file)`
- Remove `unmodified_hash_exists()` and `get_unmodified_hashes()`
- Add `source_hash_exists(hash, path)` and `target_hash_exists(hash, path)`
- Update `get_newest_hash()` to work with new schema
- Simplify `cleanup_hashes()` to remove "unmodified" logic

### 2. Unified Hash Function (`src/dot/dotfile.rs`)

**Replace two functions with one:**
```rust
// OLD: get_source_hash() and get_target_hash()
// NEW: Single unified function
pub fn get_file_hash(path: &Path, is_source: bool, db: &Database) -> Result<String>
```

**Implementation:**
```rust
pub fn get_file_hash(path: &Path, is_source: bool, db: &Database) -> Result<String> {
    if !path.exists() {
        return Err(anyhow::anyhow!("File does not exist: {}", path.display()));
    }

    // Check if cached hash is newer than file modification time
    let file_metadata = fs::metadata(path)?;
    let file_modified = file_metadata.modified()?;
    let file_time = chrono::DateTime::<chrono::Utc>::from(file_modified);

    if let Ok(Some(newest_hash)) = db.get_newest_hash(path) {
        if newest_hash.created >= file_time {
            return Ok(newest_hash.hash);
        }
    }

    // Compute and store new hash
    let hash = Self::compute_hash(path)?;
    db.add_hash(&hash, path, is_source)?;
    Ok(hash)
}
```

### 3. New Modification Detection (`src/dot/dotfile.rs`)

**Replace `is_modified()` with correct logic:**
```rust
pub fn is_target_unmodified(&self, db: &Database) -> Result<bool> {
    if !self.target_path.exists() {
        return Ok(false); // Missing files can't be unmodified
    }

    // Step 1: Get target hash
    let target_hash = self.get_file_hash(&self.target_path, false, db)?;

    // Step 2: Check if target hash matches any source hash in DB
    if db.source_hash_exists(&target_hash, &self.target_path)? {
        return Ok(true); // File was created by instantCLI
    }

    // Step 3: Check if target matches current source
    let source_hash = self.get_file_hash(&self.source_path, true, db)?;
    Ok(target_hash == source_hash)
}
```

### 4. Fix Hash Storage Calls (All files)

**Update ALL `add_hash` calls:**

**`src/dot/git.rs`:**
```rust
// Line 69: OLD
db.add_hash(&source_hash, &dotfile.source_path, true)?;
// Line 69: NEW  
db.add_hash(&source_hash, &dotfile.source_path, true)?;  // source_file=true ✓

// Line 78: OLD
db.add_hash(&target_hash, &dotfile.target_path, true)?;
// Line 78: NEW
db.add_hash(&target_hash, &dotfile.target_path, false)?; // source_file=false ✓
```

**`src/dot/dotfile.rs`:**
```rust
// Line 204: OLD
db.add_hash(&source_hash, &self.target_path, true)?;
// Line 204: NEW
db.add_hash(&source_hash, &self.target_path, false)?; // source_file=false ✓

// Line 231: OLD  
db.add_hash(&source_hash, &self.target_path, true)?;
// Line 231: NEW
db.add_hash(&source_hash, &self.target_path, false)?; // source_file=false ✓

// Lines 252-253: OLD
db.add_hash(&hash, &self.source_path, true)?;
db.add_hash(&hash, &self.target_path, true)?;
// Lines 252-253: NEW
db.add_hash(&hash, &self.source_path, true)?;   // source_file=true ✓
db.add_hash(&hash, &self.target_path, false)?;  // source_file=false ✓
```

### 5. Update Method Calls (`src/dot/dotfile.rs`)

**Replace all hash method calls:**
```rust
// OLD pattern:
let source_hash = self.get_source_hash(db)?;
let target_hash = self.get_target_hash(db)?;

// NEW pattern:
let source_hash = self.get_file_hash(&self.source_path, true, db)?;
let target_hash = self.get_file_hash(&self.target_path, false, db)?;
```

**Update `is_modified()` calls:**
```rust
// OLD:
if self.is_modified(db) {

// NEW:
if !self.is_target_unmodified(db)? {
```

### 6. Update All Call Sites

**Files needing updates:**
- `src/dot/mod.rs`: Replace `dotfile.is_modified(db)` with `!dotfile.is_target_unmodified(db)?`
- `src/dot/git.rs`: Same replacement in status functions
- `src/dot/dotfile.rs`: Update internal method calls

### 7. Update Database Tests (`src/dot/db.rs`)

**Update test in lines 230-250:**
```rust
// OLD:
db.add_hash("test_hash", &test_path, true).unwrap();

// NEW:
db.add_hash("test_hash", &test_path, true).unwrap();  // source file
db.add_hash("target_hash", &test_path, false).unwrap(); // target file
```

### 8. Cleanup Logic Simplification (`src/dot/db.rs`)

**Simplify `cleanup_hashes()`:**
```rust
// Keep newest N hashes per target file (source_file = 0), but always keep all
// source file hashes
```

## Files to Modify

1. **`src/dot/db.rs`**: Complete rewrite of database layer
2. **`src/dot/dotfile.rs`**: Rewrite hash computation and modification detection
3. **`src/dot/mod.rs`**: Update function calls to new hash system
4. **`src/dot/git.rs`**: Update hash registration calls
5. **`CLAUDE.md`**: Update documentation with new hash concepts

## Benefits of New System

1. **Clear Semantics**: `source_file` field explicitly tracks hash origin
2. **Simpler Logic**: Modification detection becomes straightforward 3-step process
3. **Better Performance**: Lazy computation reduces unnecessary hashing
4. **More Accurate**: Proper distinction between source and target hashes
5. **Easier Maintenance**: Clearer code structure and fewer edge cases

## Breaking Changes

1. **Database Schema**: Old databases will need migration or recreation
2. **API Changes**: Some method signatures and behaviors will change
3. **Configuration**: Any hash-related config may need updates

## Migration Strategy

Option 1: Clean Break (Recommended)
- Drop existing database and start fresh
- Users will need to re-run `instant dot apply` after update

