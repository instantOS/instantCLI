# Immediate Next Steps for InstantCLI Development

## Current State Analysis

The InstantCLI project has a solid foundation with the following components implemented:

### ✅ Completed Core Features
- **CLI Interface**: Full command structure with clap (src/main.rs)
- **Configuration Management**: TOML-based config system (src/dot/config.rs)
- **Database Layer**: SQLite with proper schema for hash tracking (src/dot/db.rs)
- **Dotfile Operations**: Core apply/fetch/reset functionality (src/dot/dotfile.rs)
- **Repository Management**: Git operations and local repo handling (src/dot/git.rs, src/dot/localrepo.rs)
- **Metadata System**: Repository initialization and validation (src/dot/meta.rs)
- **All Basic Commands**: clone, apply, fetch, reset, update, status, init

### 🔧 Technical Implementation
- Uses Rust with clap for CLI
- SQLite database for hash validation
- SHA256 hashing for file integrity
- Multi-repository overlay system
- User modification protection via hash validation
- Proper error handling with anyhow

### 📋 Key Missing Features (from plan.md)

1. **Hash Optimization Issues** (src/dot/dotfile.rs:52-53)
   - TODO: do not add hash if the current hash is one already in the DB
   - TODO: check if this behavior is already present

2. **Enhanced Hash Management**
   - More sophisticated hash validation logic
   - Better cleanup strategies for old hashes
   - Hash aging and expiration

3. **Enhanced CLI Features**
   - Better progress indicators for long operations
   - More detailed status reporting
   - Interactive prompts for dangerous operations

4. **Repository Management Enhancements**
   - Repository removal commands
   - Configuration editing commands
   - Repository priority management

## Immediate Next Steps Plan

### Phase 1: Hash Optimization (Priority: HIGH)

**1.1 Fix Hash Duplication Issue**
- Location: `src/dot/dotfile.rs` in `get_target_hash()` method
- Problem: Currently adds hash to DB without checking if it already exists
- Impact: Unnecessary database bloat and potential performance issues
- Implementation: Add existence check before inserting hash

**1.2 Implement Hash Caching**
- Location: `src/dot/db.rs` and `src/dot/dotfile.rs`
- Problem: Hash computation is expensive and done repeatedly
- Solution: Cache recent hashes in memory with LRU eviction
- Implementation: Add hash cache structure with TTL

**1.3 Optimize Hash Cleanup** ✅ **COMPLETED**
- Location: `src/dot/db.rs` in `cleanup_hashes()` method
- Problem: Current cleanup is too aggressive and may remove useful hashes
- Solution: Implemented smarter cleanup based on:
  - ✅ Keep all valid hashes (never remove them)
  - ✅ Keep newest invalid hash per file (for rollback capability)
  - ✅ Remove invalid hashes older than 30 days
- Added comprehensive tests to verify cleanup behavior
- Added helper methods: `get_hash_stats()` and `cleanup_all_invalid_hashes()`

### Phase 2: CLI Enhancements (Priority: MEDIUM)

**2.1 Add Progress Indicators**
- Location: `src/main.rs` and command handlers
- Features:
  - Progress bars for repository operations (clone, update)
  - File count indicators for apply/fetch operations
  - Time estimates for long-running operations

**2.2 Implement Verbose Output**
- Location: Command functions in `src/dot/mod.rs`
- Features:
  - Detailed file-by-file operation logging
  - Repository operation details
  - Debug information for troubleshooting

**2.3 Add Interactive Confirmation**
- Location: `src/main.rs` for destructive operations
- Features:
  - Confirmation prompts for reset operations
  - Warnings for overwriting user-modified files
  - Dry-run mode for previewing changes

### Phase 3: Repository Management (Priority: MEDIUM)

**3.1 Add Repository Removal**
- Location: `src/dot/config.rs` and `src/main.rs`
- Features:
  - Remove repository from configuration
  - Optional: Clean up local files
  - Safety checks to prevent accidental removal

**3.2 Implement Repository Prioritization**
- Location: `src/dot/config.rs` and `src/dot/mod.rs`
- Features:
  - Priority field in repository configuration
  - Overlay logic based on priority order
  - Commands to reorder repositories

**3.3 Add Configuration Editing**
- Location: `src/main.rs` and new module
- Features:
  - Interactive config editing
  - Repository list management
  - Global settings adjustment

### Phase 4: Testing and Quality (Priority: HIGH)

**4.1 Expand Test Coverage**
- Location: New test files and existing modules
- Features:
  - Unit tests for hash management
  - Integration tests for CLI commands
  - Mock filesystem for testing
  - Edge case testing (corrupted repos, missing files)

**4.2 Add Error Recovery**
- Location: Throughout the codebase
- Features:
  - Graceful handling of network failures
  - Recovery from corrupted database
  - Fallback strategies for partial failures

**4.3 Documentation and Examples**
- Location: New documentation files
- Features:
  - User guide with examples
  - Repository creation guide
  - Troubleshooting guide

## Technical Debt to Address

### Code Quality Issues
1. **Unused Fields**: `hash` and `target_hash` fields in `Dotfile` struct are never read
2. **Code Duplication**: Hash computation logic appears in multiple places
3. **Error Handling**: Some operations use unwrap() instead of proper error handling

### Performance Considerations
1. **Database Queries**: Some queries could be optimized with better indexing
2. **Filesystem Operations**: Repeated metadata calls could be cached
3. **Memory Usage**: Large file trees could cause memory pressure

### Security Considerations
1. **Input Validation**: Repository URLs and paths need better validation
2. **File Permissions**: Ensure proper handling of sensitive files
3. **Network Security**: Secure git operations with proper validation

## Implementation Strategy

### Order of Operations
1. **Phase 1**: Fix critical hash optimization issues (highest priority)
2. **Phase 4**: Add comprehensive testing (foundational)
3. **Phase 2**: Enhance CLI experience (user-facing improvements)
4. **Phase 3**: Advanced repository management (feature expansion)

### Success Metrics
- No hash duplication in database
- 100% test coverage for core functionality
- User-friendly CLI with clear feedback
- Robust error handling and recovery

### Risk Assessment
- **Low Risk**: CLI enhancements, documentation
- **Medium Risk**: Repository management features
- **High Risk**: Hash optimization changes (affects core logic)

## Next Steps

### ✅ Completed (Phase 1.3)
- **Optimized hash cleanup**: Implemented smarter cleanup that preserves valid hashes and keeps newest invalid hash per file
- **Added comprehensive tests**: Verified cleanup behavior with edge cases
- **Added helper methods**: `get_hash_stats()` and `cleanup_all_invalid_hashes()` for debugging

### ✅ Completed (Multiple Subdirectories Support)
- **Enhanced repository metadata**: Added support for multiple `dots_dirs` in `instantdots.toml`
- **Configurable active subdirectories**: Added `active_subdirs` field in global config per repo
- **Updated core functions**: Modified `get_all_dotfiles()` and `fetch_modified()` to use active subdirectories
- **Enhanced LocalRepo**: Added helper methods for managing multiple subdirectories
- **New CLI commands**: Added `list-subdirs`, `set-subdirs`, and `show-subdirs` commands
- **Backward compatibility**: Defaults to `["dots"]` for existing repositories

### 🔍 In Progress
1. **Phase 1.1**: Fix hash duplication issue in `get_target_hash()` (high priority)
2. **Phase 1.2**: Implement hash caching for performance optimization

### 📋 Upcoming
3. **Phase 4**: Expand test coverage for remaining core functionality
4. **Phase 2**: Enhance CLI experience (progress indicators, verbose output)
5. **Phase 3**: Advanced repository management features

This plan focuses on stabilizing the core functionality first, then expanding features while maintaining code quality and user experience.