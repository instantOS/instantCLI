# Plan: Redesigned Doctor CLI System with Privilege Management

## Overview
Complete redesign of the doctor CLI system with granular commands and robust privilege escalation using the `sudo` crate. The new system will support:
- `instant doctor` - run all checks
- `instant doctor run <name>` - run specific check
- `instant doctor fix <name>` - fix specific check with appropriate privileges
- Generic system accommodating many checks with privilege requirements

## New CLI Design

### Command Structure
```
instant doctor [SUBCOMMAND]

Subcommands:
    (default)           Run all health checks
    run <name>          Run a specific health check
    fix <name>          Apply fix for a specific health check
```

### Implementation in main.rs
```rust
#[derive(Subcommand, Debug)]
enum Commands {
    // ... existing commands ...
    
    /// System diagnostics and fixes
    Doctor {
        #[command(subcommand)]
        command: Option<DoctorCommands>,
    },
}

#[derive(Subcommand, Debug)]
enum DoctorCommands {
    /// Run a specific health check
    Run {
        /// Name of the check to run
        name: String,
    },
    /// Apply fix for a specific health check
    Fix {
        /// Name of the check to fix
        name: String,
    },
}
```

## Redesigned Doctor System Architecture

### 0. Core Privilege Management System

**Using `sudo` crate for Process Restarting:**
- Each check declares its privilege requirement (`PrivilegeLevel`)
- If a fix needs root and we're not root, use `sudo::escalate_if_needed()` to restart CLI as root
- If running as root and check needs normal user, throw error
- CLI arguments passed: `instant doctor fix <name> --internal-privileged-mode`

### 1. Enhanced DoctorCheck Trait with Fixable Status

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PrivilegeLevel {
    User,    // Must run as normal user
    Root,    // Must run as root
    Any,     // Can run as either
}

#[derive(Debug, Clone)]
pub enum CheckStatus {
    Pass(String),
    Fail { message: String, fixable: bool },
    Warning { message: String, fixable: bool },
}

impl CheckStatus {
    pub fn message(&self) -> &String {
        match self {
            CheckStatus::Pass(msg) => msg,
            CheckStatus::Fail { message, .. } => message,
            CheckStatus::Warning { message, .. } => message,
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(self, CheckStatus::Pass(_))
    }
    
    pub fn is_fixable(&self) -> bool {
        match self {
            CheckStatus::Pass(_) => false,
            CheckStatus::Fail { fixable, .. } => *fixable,
            CheckStatus::Warning { fixable, .. } => *fixable,
        }
    }
    
    pub fn needs_fix(&self) -> bool {
        !self.is_success()
    }

    pub fn color_status(&self) -> impl std::fmt::Display {
        match self {
            CheckStatus::Pass(_) => "PASS".green(),
            CheckStatus::Fail { .. } => "FAIL".red(),
            CheckStatus::Warning { .. } => "WARN".yellow(),
        }
    }
    
    pub fn fixable_indicator(&self) -> &'static str {
        match self {
            CheckStatus::Pass(_) => "",
            CheckStatus::Fail { fixable: true, .. } => " (fixable)",
            CheckStatus::Fail { fixable: false, .. } => " (not fixable)",
            CheckStatus::Warning { fixable: true, .. } => " (fixable)",
            CheckStatus::Warning { fixable: false, .. } => " (not fixable)",
        }
    }
}

#[async_trait::async_trait]
pub trait DoctorCheck: Send + Sync {
    fn name(&self) -> &'static str;
    fn id(&self) -> &'static str; // Machine-readable identifier
    
    async fn execute(&self) -> CheckStatus;
    
    fn fix_message(&self) -> Option<String> { None }
    async fn fix(&self) -> Result<()> { Ok(()) }
    
    // NEW: Privilege requirements
    fn check_privilege_level(&self) -> PrivilegeLevel { PrivilegeLevel::Any }
    fn fix_privilege_level(&self) -> PrivilegeLevel { PrivilegeLevel::Any }
}
```

### 2. Check Registry System

**File:** `src/doctor/registry.rs`
```rust
use std::collections::HashMap;
use super::checks::*;

pub type CheckFactory = fn() -> Box<dyn DoctorCheck + Send + Sync>;

pub struct CheckRegistry {
    checks: HashMap<&'static str, CheckFactory>,
}

impl CheckRegistry {
    pub fn new() -> Self {
        let mut registry = CheckRegistry {
            checks: HashMap::new(),
        };
        
        // Register all checks
        registry.register::<InternetCheck>("internet");
        registry.register::<InstantRepoCheck>("instant-repo");
        // Easy to add more checks here
        
        registry
    }
    
    fn register<T: DoctorCheck + Default + Send + Sync + 'static>(&mut self, id: &'static str) {
        self.checks.insert(id, || Box::new(T::default()));
    }
    
    pub fn create_check(&self, id: &str) -> Option<Box<dyn DoctorCheck + Send + Sync>> {
        self.checks.get(id).map(|factory| factory())
    }
    
    pub fn all_check_ids(&self) -> Vec<&'static str> {
        self.checks.keys().copied().collect()
    }
    
    pub fn all_checks(&self) -> Vec<Box<dyn DoctorCheck + Send + Sync>> {
        self.checks.values().map(|factory| factory()).collect()
    }
}

// Global registry instance
lazy_static::lazy_static! {
    pub static ref REGISTRY: CheckRegistry = CheckRegistry::new();
}
```

### 3. Privilege Management Logic

**File:** `src/doctor/privileges.rs`
```rust
use sudo::RunningAs;

pub fn check_privilege_requirements(
    check: &dyn DoctorCheck, 
    is_fix: bool
) -> Result<(), PrivilegeError> {
    let required = if is_fix {
        check.fix_privilege_level()
    } else {
        check.check_privilege_level()
    };
    
    let current = sudo::check();
    
    match (required, current) {
        (PrivilegeLevel::Root, RunningAs::User) => {
            Err(PrivilegeError::NeedRoot)
        }
        (PrivilegeLevel::User, RunningAs::Root) => {
            Err(PrivilegeError::MustNotBeRoot)
        }
        _ => Ok(())
    }
}

pub fn escalate_for_fix(check_id: &str) -> Result<(), sudo::Error> {
    // Use sudo crate to restart with privileges
    sudo::with_env(&["RUST_BACKTRACE"])
        .arg("doctor")
        .arg("fix")
        .arg(check_id)
        .arg("--internal-privileged-mode")
        .escalate_if_needed()
}

#[derive(Debug, thiserror::Error)]
pub enum PrivilegeError {
    #[error("This operation requires root privileges")]
    NeedRoot,
    #[error("This operation must not run as root for security reasons")]
    MustNotBeRoot,
}
```

### 4. Doctor Command Handler

**File:** `src/doctor/command.rs`
```rust
pub async fn handle_doctor_command(command: Option<DoctorCommands>) -> Result<()> {
    match command {
        None => run_all_checks().await,
        Some(DoctorCommands::Run { name }) => run_single_check(&name).await,
        Some(DoctorCommands::Fix { name }) => fix_single_check(&name).await,
    }
}

async fn run_all_checks() -> Result<()> {
    let checks = REGISTRY.all_checks();
    let results = run_checks_concurrent(checks).await;
    print_results(&results);
    
    // Show available fixes (only for fixable failures)
    show_available_fixes(&results);
    Ok(())
}

fn print_results(results: &[CheckResult]) {
    let header = format!(
        "{: <30} [{}] {}",
        "Check".bold(),
        "Status".bold(),
        "Message".bold()
    );
    println!("{header}");

    for result in results {
        let status_str = result.status.color_status();
        let fixable_str = result.status.fixable_indicator();
        let line = format!(
            "{: <30} [{}] {}{}",
            result.name,
            status_str,
            result.status.message(),
            fixable_str
        );
        println!("{line}");
    }
}

fn show_available_fixes(results: &[CheckResult]) {
    let fixable_failures: Vec<_> = results.iter()
        .filter(|result| result.status.needs_fix() && result.status.is_fixable())
        .collect();
    
    if !fixable_failures.is_empty() {
        let fixes_msg = "\nAvailable fixes:".bold().yellow();
        println!("{fixes_msg}");
        for result in &fixable_failures {
            if let Some(ref msg) = result.fix_message {
                println!("  - {}: {}", result.name, msg);
                println!("    Run: instant doctor fix {}", result.check_id);
            }
        }
    }
    
    let non_fixable_failures: Vec<_> = results.iter()
        .filter(|result| result.status.needs_fix() && !result.status.is_fixable())
        .collect();
        
    if !non_fixable_failures.is_empty() {
        let manual_msg = "\nRequires manual intervention:".bold().red();
        println!("{manual_msg}");
        for result in &non_fixable_failures {
            println!("  - {}: {}", result.name, result.status.message());
        }
    }
}

async fn run_single_check(check_id: &str) -> Result<()> {
    let check = REGISTRY.create_check(check_id)
        .ok_or_else(|| anyhow!("Unknown check: {}", check_id))?;
    
    // Verify privilege requirements for check
    if let Err(e) = check_privilege_requirements(check.as_ref(), false) {
        return Err(anyhow!("Privilege error: {}", e));
    }
    
    let result = execute_single_check(check).await;
    print_single_result(&result);
    Ok(())
}

async fn fix_single_check(check_id: &str) -> Result<()> {
    let check = REGISTRY.create_check(check_id)
        .ok_or_else(|| anyhow!("Unknown check: {}", check_id))?;
    
    // STEP 1: Always run the check first to determine current state
    println!("Checking current state for '{}'...", check.name());
    let check_result = check.execute().await;
    
    // STEP 2: Determine if fix is needed based on check result
    if check_result.is_success() {
        println!("✓ {}: {}", check.name(), check_result.message());
        println!("No fix needed - check already passes.");
        return Ok(());
    }
    
    if !check_result.is_fixable() {
        println!("✗ {}: {}", check.name(), check_result.message());
        return Err(anyhow!(
            "Check '{}' failed but is not fixable. Manual intervention required.", 
            check.name()
        ));
    }
    
    // STEP 3: Check is failing and fixable, proceed with fix
    println!("⚠ {}: {}", check.name(), check_result.message());
    println!("Fix is available and will be applied.");
    
    // Check if we have the right privileges for the fix
    match check_privilege_requirements(check.as_ref(), true) {
        Ok(()) => {
            // We have correct privileges, run the fix
            apply_fix(check).await
        }
        Err(PrivilegeError::NeedRoot) => {
            // Need to escalate privileges
            println!("Fix for '{}' requires administrator privileges.", check.name());
            
            if should_escalate(check.as_ref())? {
                escalate_for_fix(check_id)?;
                // This won't return - process will be restarted with sudo
                unreachable!()
            } else {
                println!("Fix cancelled by user.");
                Ok(())
            }
        }
        Err(PrivilegeError::MustNotBeRoot) => {
            Err(anyhow!("Fix for '{}' cannot run as root", check.name()))
        }
    }
}

fn should_escalate(check: &dyn DoctorCheck) -> Result<bool> {
    use dialoguer::Confirm;
    
    let message = format!(
        "Apply fix for '{}'? This requires administrator privileges.\nFix: {}",
        check.name(),
        check.fix_message().unwrap_or_default()
    );
    
    Ok(Confirm::new()
        .with_prompt(message)
        .default(false)
        .interact()?)
}
```

### 5. Check Implementation Examples

**File:** `src/doctor/checks.rs`
```rust
#[derive(Default)]
pub struct InternetCheck;

impl DoctorCheck for InternetCheck {
    fn name(&self) -> &'static str { "Internet Connectivity" }
    fn id(&self) -> &'static str { "internet" }
    
    fn check_privilege_level(&self) -> PrivilegeLevel { 
        PrivilegeLevel::Any 
    }
    fn fix_privilege_level(&self) -> PrivilegeLevel { 
        PrivilegeLevel::User  // nmtui should run as user
    }
    
    // ... existing implementation
}

#[derive(Default)]
pub struct InstantRepoCheck;

impl DoctorCheck for InstantRepoCheck {
    fn name(&self) -> &'static str { "InstantOS Repository Configuration" }
    fn id(&self) -> &'static str { "instant-repo" }
    
    fn check_privilege_level(&self) -> PrivilegeLevel { 
        PrivilegeLevel::Any  // Can read config as any user
    }
    fn fix_privilege_level(&self) -> PrivilegeLevel { 
        PrivilegeLevel::Root  // Modifying /etc/pacman.conf requires root
    }
    
    async fn execute(&self) -> CheckStatus {
        // Check if /etc/pacman.conf contains [instant] section
        match tokio::fs::read_to_string("/etc/pacman.conf").await {
            Ok(content) => {
                if content.contains("[instant]") && 
                   content.contains("/etc/pacman.d/instantmirrorlist") {
                    CheckStatus::Pass("InstantOS repository is configured".to_string())
                } else {
                    CheckStatus::Fail { 
                        message: "InstantOS repository not found in pacman.conf".to_string(),
                        fixable: true  // We can add the repository configuration
                    }
                }
            }
            Err(_) => CheckStatus::Fail { 
                message: "Could not read /etc/pacman.conf".to_string(),
                fixable: false  // If we can't read the file, we probably can't fix it either
            }
        }
    }
    
    fn fix_message(&self) -> Option<String> {
        Some("Add InstantOS repository configuration to /etc/pacman.conf".to_string())
    }
    
    async fn fix(&self) -> Result<()> {
        use tokio::fs::OpenOptions;
        use tokio::io::AsyncWriteExt;
        
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open("/etc/pacman.conf")
            .await?;
            
        file.write_all(b"\n[instant]\nInclude = /etc/pacman.d/instantmirrorlist\n").await?;
        
        println!("Added InstantOS repository to /etc/pacman.conf");
        Ok(())
    }
}
```

## Implementation Steps

### Phase 1: Core Architecture (PRIORITY)

1. **Step 1**: Add `sudo = "0.6"` to Cargo.toml
2. **Step 2**: Update CheckStatus enum to include fixable field
3. **Step 3**: Create new CLI structure in main.rs with DoctorCommands
4. **Step 4**: Create `src/doctor/privileges.rs` with privilege checking logic
5. **Step 5**: Create `src/doctor/registry.rs` with check registration system
6. **Step 6**: Create `src/doctor/command.rs` with new command handlers including check-first logic
7. **Step 7**: Update existing InternetCheck to use new trait methods and fixable status
8. **Step 8**: Test new system with existing check

### Phase 2: InstantRepo Check Implementation

9. **Step 9**: Implement InstantRepoCheck with privilege requirements and fixable status
10. **Step 10**: Register new check in registry system
11. **Step 11**: Test privilege escalation flow
12. **Step 12**: Test error handling for privilege mismatches
13. **Step 13**: Test check-first logic in fix command

### Phase 3: Polish and Testing

14. **Step 14**: Add comprehensive error handling for fixable status edge cases
15. **Step 15**: Improve user feedback and messaging with fixable indicators
16. **Step 16**: Add validation for internal privileged mode flag
17. **Step 17**: Test on various system configurations
18. **Step 18**: Test scenarios where checks become non-fixable

## Benefits of This Design

### Scalability:
- **Easy Registration**: New checks added with single line in registry
- **Automatic Discovery**: Registry automatically manages all checks
- **Consistent Interface**: All checks use same privilege system

### User Experience:
- **Granular Control**: Users can run/fix specific checks
- **Clear Feedback**: Explicit privilege requirements and prompts
- **Safety**: Can't accidentally run privileged operations

### Security:
- **Least Privilege**: Each operation declares minimum required privileges
- **User Consent**: No silent privilege escalation
- **Process Isolation**: Privileged operations run in separate process instance

### Maintainability:
- **Type Safety**: No string-based check matching
- **Clear Separation**: Privilege logic isolated in dedicated module
- **Easy Testing**: Each check can be tested independently

## Dependencies

**Update Cargo.toml:**
```toml
[dependencies]
sudo = "0.6"                # Privilege escalation via process restart
lazy_static = "1.4"         # Global registry instance
thiserror = "1.0"          # Error handling
dialoguer = "0.11"         # User prompts (already in use)
tokio = { version = "1.0", features = ["fs"] }  # File operations
```

## Migration Path

**Break backwards compatibility**: InternetCheck should be ported to the new
   system, but no leftover functions or anything from old legacy implementations
   needs to be kept

