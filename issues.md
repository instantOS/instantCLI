# Technical Debt Issues for InstantCLI

## Summary
This document outlines the technical debt identified in the InstantCLI codebase. The issues are categorized by priority and include specific file locations and line numbers.

## High Priority Issues (Security & Stability)

### 1. SQL Injection Risk
**File:** `src/dot/db.rs:195-201`
**Issue:** Direct string interpolation in SQL queries creates potential SQL injection vulnerability
```rust
&format!(
    "DELETE FROM hashes WHERE unmodified = 0 AND created < datetime('now', '-{} days')",
    days
)
```
**Fix:** Use parameterized queries instead of string formatting

### 2. Excessive unwrap() Usage
**Files:** Multiple files throughout codebase
**Issue:** Over 20 instances of `unwrap()` calls that could cause panics
**Critical locations:**
- `src/dot/mod.rs:109, 211, 236`
- `src/dot/dotfile.rs` (7 instances)
- `src/dot/db.rs` (6 instances)
- `src/main.rs:196` (expect() call)
**Fix:** Replace with proper error handling using `?` operator or custom error types

### 3. Dead Code - RepositoryManager
**File:** `src/dot/repo/manager.rs`
**Issue:** Entire 200+ line file is unused, containing 10+ unused methods
**Fix:** Remove the entire file or integrate the functionality if needed

### 4. Unused Functions in Core Modules
**File:** `src/dot/mod.rs`
**Issue:** 6 unused functions cluttering the codebase
- `get_repo_active_dirs()` - Line 74
- `list_repo_subdirs()` - Line 258
- `set_repo_active_subdirs()` - Line 265
- `show_repo_active_subdirs()` - Line 291
- `find_repo_by_name()` - Line 300
- `remove_repo()` - Line 415
**Fix:** Remove unused functions or integrate them into the codebase

## Medium Priority Issues (Maintainability)

### 5. Code Duplication
**Files:** `src/dot/config.rs:269` and `src/dot/repo/manager.rs:194`
**Issue:** Identical `extract_repo_name()` functions in two files
**Fix:** Consolidate into a single utility function in a common module

### 6. Missing Documentation
**Files:** Throughout codebase
**Issue:** Most public functions lack Rustdoc comments
**Fix:** Add comprehensive documentation for all public APIs and complex business logic

### 7. Complex Functions Need Refactoring
**File:** `src/dot/mod.rs`
**Issue:** Overly complex functions that handle multiple concerns
- `get_all_dotfiles()` - Lines 92-124
- `group_dotfiles_by_repo()` - Lines 177-195
**Fix:** Break down into smaller, focused functions


## Low Priority Issues (Code Quality)

### 9. Performance Issues
**Files:** `src/dot/db.rs`, `src/dot/dotfile.rs`
**Issues:**
- No database connection pooling
- Repeated hash computations without caching
- Multiple file system operations without batching
**Fix:** Implement caching and optimize database operations

### 11. Redundant Code Patterns
**File:** `src/main.rs:89-218`
**Issue:** Repetitive error handling blocks
**Fix:** Extract into helper functions

