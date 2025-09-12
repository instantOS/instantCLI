# Plan: Add InstantOS Pacman Repository Doctor Check

## Overview
Add a new doctor check to verify that the InstantOS pacman repository is properly configured in the system. This check will ensure users have access to InstantOS-specific packages.

## Implementation Plan

### 0. Refactor doctor system (PRIORITY)

**Issues identified:**
- Doctor logic is embedded in main.rs (lines 216-250+)
- String-based check matching is fragile and duplicated
- Need proper architecture for check registration and execution
- Privilege escalation handling needed for fixes requiring root access

**Refactoring Steps:**

1. **Create doctor command module** (`src/doctor/command.rs`):
   - Move all doctor logic from main.rs
   - Create `DoctorCommand` struct to encapsulate functionality
   - Implement clean separation of concerns

2. **Improve check registration system:**
   - Replace string matching with enum or ID-based system
   - Create check registry that maps check types to implementations
   - Eliminate duplicate check name strings across codebase

3. **Add privilege escalation support:**
   - Research `sudo-rs` or similar crates for privilege escalation
   - Consider `xshell` for improved shell command execution
   - Implement secure privilege escalation for fixes requiring root

4. **Create doctor module structure:**
   ```
   src/doctor/
   ├── mod.rs          (traits and core types)
   ├── command.rs      (main command logic, moved from main.rs)
   ├── checks.rs       (individual check implementations)
   └── registry.rs     (check registration and management)
   ```

**Dependencies to evaluate:**
- `xshell` for shell command execution (mentioned in research/xshell.md)
- `sudo-rs` or similar for privilege escalation
- Consider `dialoguer` improvements for better user interaction 

### 1. Create New Doctor Check Structure
**File:** `src/doctor/checks.rs`

Add a new `InstantRepoCheck` struct that implements the `DoctorCheck` trait:

```rust
pub struct InstantRepoCheck;

#[async_trait]
impl DoctorCheck for InstantRepoCheck {
    fn name(&self) -> &'static str {
        "InstantOS Repository Configuration"
    }

    async fn execute(&self) -> CheckStatus {
        // Check implementation details below
    }

    fn fix_message(&self) -> Option<String> {
        // Fix suggestion implementation
    }

    async fn fix(&self) -> Result<()> {
        // Automatic fix implementation
    }
}
```

### 2. Check Implementation Logic
The `execute()` method should verify:

1. **Primary Check**: `/etc/pacman.conf` contains `[instant]` section
2. **Secondary Check**: Mirror list file `/etc/pacman.d/instantmirrorlist` exists
4. **Optional**: Test connectivity to primary repository server

**Check Logic Flow:**
```
1. Read /etc/pacman.conf
2. Search for [instant] section
3. If found, verify Include = /etc/pacman.d/instantmirrorlist
4. Check if mirror list file exists and is readable
5. Verify mirror list contains uncommented Server entries
6. Return appropriate CheckStatus (Pass/Fail/Warning)
```

### 3. Fix Implementation
**Automatic Fix Options:**
1. **Conservative Fix**: Display instructions for manual configuration
2. **Advanced Fix**: Automatically add repository configuration (requires root)

**Fix Message:** 
"Add InstantOS repository to /etc/pacman.conf and ensure mirror list is configured"

**Potential Fix Commands:**
```bash
# Add to /etc/pacman.conf (requires sudo)
echo -e "\n[instant]\nInclude = /etc/pacman.d/instantmirrorlist" >> /etc/pacman.conf

# Create mirror list if missing
sudo cp /usr/share/instantos/instantmirrorlist /etc/pacman.d/
```

### 4. Integration Points (UPDATED for refactored architecture)

**After refactoring**, integration will be through the new registry system:

**File:** `src/doctor/registry.rs`
```rust
use super::checks::{InternetCheck, InstantRepoCheck};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CheckId {
    Internet,
    InstantRepo,
}

pub fn create_check(id: CheckId) -> Box<dyn DoctorCheck + Send + Sync> {
    match id {
        CheckId::Internet => Box::new(InternetCheck),
        CheckId::InstantRepo => Box::new(InstantRepoCheck),
    }
}

pub fn all_checks() -> Vec<CheckId> {
    vec![CheckId::Internet, CheckId::InstantRepo]
}
```

**File:** `src/doctor/command.rs`
```rust
pub async fn run_doctor_command() -> Result<()> {
    let check_ids = registry::all_checks();
    let checks: Vec<_> = check_ids.iter()
        .map(|&id| registry::create_check(id))
        .collect();
    
    let results = run_all_checks(checks).await;
    // Handle results and fixes with proper check ID mapping
}
```

This eliminates the fragile string matching identified in the TODO comment.

### 5. Implementation Steps (UPDATED - Refactor First Approach)

**Phase 1: Doctor System Refactoring (REQUIRED FIRST)**
1. **Step 1**: Create `src/doctor/command.rs` and move logic from main.rs
2. **Step 2**: Create `src/doctor/registry.rs` with CheckId enum system
3. **Step 3**: Update main.rs to use new DoctorCommand
4. **Step 4**: Test refactored system with existing InternetCheck
5. **Step 5**: Research and implement privilege escalation solution

**Phase 2: InstantRepo Check Implementation**
6. **Step 6**: Add `InstantRepoCheck` struct to `src/doctor/checks.rs`
7. **Step 7**: Implement file reading and parsing logic
8. **Step 8**: Add comprehensive error handling and status reporting
9. **Step 9**: Implement fix message and fix logic with sudo support
10. **Step 10**: Register new check in registry system
12. **Step 11**: Add appropriate imports and dependencies if needed

**Edge Cases to Handle:**
- Missing `/etc/pacman.conf` file
- Permission denied reading configuration files
- Malformed configuration syntax
- Empty or corrupted mirror lists
- Network connectivity issues (for optional server testing)

### 7. Error Handling

**Robust Error Handling:**
- File I/O errors (permissions, missing files)
- Parsing errors for configuration files
- Network timeouts (if connectivity testing is implemented)
- Graceful degradation for non-InstantOS systems

### 8. Dependencies (UPDATED)

**For Refactoring:**
- `xshell` for improved shell command execution (already researched - see research/xshell.md)
- `sudo-rs` or alternative for privilege escalation
- Consider `anyhow` for better error handling (already in use)

**For InstantRepo Check:**
- `tokio::fs` for async file operations
- `std::fs` for synchronous file operations
- Potentially `regex` for configuration parsing

**Update Cargo.toml:**
```toml
[dependencies]
xshell = "0.3"       # Better shell commands
sudo = "0.6"         # Privilege escalation (evaluate alternatives)
regex = "1.0"        # If complex parsing is needed
```

**Research needed:**
- Compare `sudo-rs`, `elevate`, or other privilege escalation crates
- Evaluate security implications of automated sudo prompts

### 9. Documentation

**Code Documentation:**
- Add inline comments explaining the check logic
- Document expected file formats and locations
- Add examples of valid/invalid configurations

## Completion Criteria

**Phase 1 (Refactoring):**
- [ ] Doctor logic moved out of main.rs into dedicated command module
- [ ] String-based check matching replaced with enum/ID system
- [ ] Registry system implemented for check management
- [ ] Privilege escalation support added for fixes requiring root
- [ ] Existing InternetCheck works with refactored architecture
- [ ] Code is cleaner, more maintainable, and follows better patterns

**Phase 2 (InstantRepo Check):**
- [ ] New check correctly identifies InstantOS repository configuration
- [ ] Provides helpful error messages for different failure scenarios
- [ ] Offers practical fix suggestions with sudo support
- [ ] Integrates seamlessly with refactored doctor command structure
- [ ] Handles edge cases gracefully
- [ ] Tested on multiple system configurations

