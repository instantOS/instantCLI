# Privilege Escalation Research for Doctor Command

## Overview
Research into using the `sudo` crate for privilege escalation in the InstantCLI doctor command, specifically for fixes that require root access (like modifying `/etc/pacman.conf`).

## Requirements Analysis
For InstantCLI doctor command, we need:
- Safe execution of commands requiring root privileges
- User consent and awareness of privilege escalation
- Process isolation for security
- Integration with existing async architecture
- Clean restart mechanism for privileged operations

## Selected Solution: sudo crate (v0.6.0)

**Repository:** https://gitlab.com/dns2utf8/sudo.rs  
**License:** MIT/Apache-2.0  
**Documentation:** https://docs.rs/sudo/0.6.0

### Why sudo crate?
- **Process Isolation:** Restarts entire application with clean elevated privileges
- **Security:** No in-process privilege elevation, reducing attack surface
- **Simplicity:** Well-tested, mature API with clear semantics
- **Platform Support:** Works on Linux, macOS, and expected to work on *BSD systems
- **User Consent:** Leverages system's native sudo prompt for authentication

### Key Features:
- Privilege detection with `sudo::check()`
- Clean process restart with `sudo::escalate_if_needed()`
- Environment variable preservation with `sudo::with_env()`
- Cross-platform support (Unix-like systems)

## sudo Crate API Reference

### Core Functions

#### `sudo::check()` - Privilege Detection
```rust
use sudo::RunningAs;

match sudo::check() {
    RunningAs::Root => println!("Running as root!"),
    RunningAs::User => println!("Running as a regular user."),
    RunningAs::Unknown => println!("Could not determine privilege level."),
}
```

#### `sudo::escalate_if_needed()` - Basic Privilege Escalation
```rust
use sudo::RunningAs;

fn main() {
    match sudo::check() {
        RunningAs::Root => {
            println!("Successfully running with root privileges.");
            // Your privileged operations here
        }
        RunningAs::User => {
            println!("Not running as root. Attempting to escalate...");
            // This will restart the program with sudo if not root
            sudo::escalate_if_needed().expect("Failed to escalate privileges");
        }
        RunningAs::Unknown => {
            eprintln!("Error: Could not determine privilege level.");
            std::process::exit(1);
        }
    }
}
```

#### `sudo::with_env(&[&str])` - Escalation with Environment Variables
```rust
use sudo::RunningAs;

fn main() {
    match sudo::check() {
        RunningAs::Root => {
            println!("Running as root with preserved environment.");
            // Access preserved environment variables
            if let Ok(backtrace) = std::env::var("RUST_BACKTRACE") {
                println!("RUST_BACKTRACE: {}", backtrace);
            }
        }
        RunningAs::User => {
            println!("Escalating with environment preservation...");
            // Preserve RUST_BACKTRACE and other debug variables
            sudo::with_env(&["RUST_BACKTRACE", "RUST_LOG"])
                .expect("Failed to escalate with environment");
        }
        RunningAs::Unknown => {
            eprintln!("Could not determine privilege level.");
            std::process::exit(1);
        }
    }
}
```

### RunningAs Enum
```rust
#[derive(Debug, PartialEq)]
pub enum RunningAs {
    Root,    // UID == 0 and EUID == 0
    User,    // UID != 0 or EUID != 0  
    Unknown, // Could not determine privilege level
}
```

## Practical Usage Tips

### 1. InstantCLI Integration Pattern
```rust
// In doctor fix command
fn handle_fix_with_privileges(check_id: &str) -> Result<()> {
    match sudo::check() {
        RunningAs::Root => {
            // We're running as root, proceed with fix
            apply_privileged_fix(check_id)
        }
        RunningAs::User => {
            // Need to restart with privileges
            println!("Fix requires administrator privileges.");
            
            // Build arguments for restarted process
            let args = std::env::args().collect::<Vec<_>>();
            let mut new_args = args.clone();
            new_args.push("--internal-privileged-mode".to_string());
            
            // Preserve debugging environment variables
            sudo::with_env(&["RUST_BACKTRACE", "RUST_LOG"])
                .args(&new_args[1..])  // Skip program name
                .escalate_if_needed()
                .expect("Failed to escalate privileges");
                
            // This point is never reached - process restarts
            unreachable!()
        }
        RunningAs::Unknown => {
            Err(anyhow!("Cannot determine privilege level"))
        }
    }
}
```

### 2. Detecting Privileged Mode
```rust
fn is_running_in_privileged_mode() -> bool {
    // Check for internal flag and root privileges
    std::env::args().any(|arg| arg == "--internal-privileged-mode") &&
    matches!(sudo::check(), RunningAs::Root)
}

fn main() -> Result<()> {
    if is_running_in_privileged_mode() {
        // We were restarted by sudo, run only the privileged operation
        handle_privileged_operation()
    } else {
        // Normal operation
        handle_normal_operation()
    }
}
```

### 3. Error Handling for Privilege Escalation
```rust
fn safe_privilege_escalation() -> Result<()> {
    match sudo::check() {
        RunningAs::User => {
            match sudo::escalate_if_needed() {
                Ok(_) => unreachable!(), // Process should restart
                Err(e) => {
                    match e.kind() {
                        // User cancelled sudo prompt
                        ErrorKind::PermissionDenied => {
                            println!("Privilege escalation cancelled by user.");
                            Ok(())
                        }
                        // sudo command not found
                        ErrorKind::NotFound => {
                            Err(anyhow!("sudo command not found. Please install sudo."))
                        }
                        // Other errors
                        _ => Err(anyhow!("Failed to escalate privileges: {}", e))
                    }
                }
            }
        }
        RunningAs::Root => {
            println!("Already running with root privileges.");
            Ok(())
        }
        RunningAs::Unknown => {
            Err(anyhow!("Cannot determine current privilege level"))
        }
    }
}
```

### 4. Preventing Accidental Root Operations
```rust
fn ensure_user_privileges() -> Result<()> {
    match sudo::check() {
        RunningAs::Root => {
            Err(anyhow!("This operation must not run as root for security reasons"))
        }
        RunningAs::User => {
            println!("Running safely as regular user.");
            Ok(())
        }
        RunningAs::Unknown => {
            println!("Warning: Could not verify privilege level.");
            Ok(()) // Proceed with caution
        }
    }
}
```

## Security Best Practices

### Process Isolation Benefits:
- **Clean State:** Each privileged operation starts with a fresh process
- **No Privilege Leakage:** Privileges don't persist beyond the specific operation
- **System Integration:** Uses system's native sudo authentication
- **Audit Trail:** System logs show exactly what commands were run with sudo

### Implementation Guidelines:
1. **Always Check First:** Use `sudo::check()` before any privilege-sensitive operation
2. **Preserve Essential Environment:** Use `with_env()` to keep debugging variables like `RUST_BACKTRACE`
3. **Handle All Cases:** Account for `RunningAs::Unknown` state gracefully
4. **User Communication:** Always inform users when privilege escalation is needed
5. **Error Recovery:** Provide clear error messages when escalation fails

### Security Considerations:
- **No Password Caching:** sudo crate never stores passwords - relies on system sudo
- **Command Validation:** Always validate arguments before privilege escalation
- **Minimal Scope:** Only escalate for specific operations that require it
- **User Consent:** System sudo prompt ensures user awareness and consent

## InstantCLI Doctor Integration Examples

### Command-Line Argument Handling
```rust
// Parse --internal-privileged-mode flag
#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
    
    /// Internal flag set when restarted with sudo
    #[arg(long, hide = true)]
    internal_privileged_mode: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    
    if cli.internal_privileged_mode {
        // We were restarted by sudo for a privileged operation
        handle_privileged_mode(&cli)
    } else {
        // Normal operation
        handle_normal_mode(&cli)
    }
}
```

### Doctor Fix with Privilege Escalation
```rust
async fn fix_single_check(check_id: &str) -> Result<()> {
    let check = REGISTRY.create_check(check_id)
        .ok_or_else(|| anyhow!("Unknown check: {}", check_id))?;
    
    // Check current privilege requirements
    if check.fix_privilege_level() == PrivilegeLevel::Root {
        match sudo::check() {
            RunningAs::Root => {
                // We have the required privileges, proceed
                apply_privileged_fix(check).await
            }
            RunningAs::User => {
                // Need to escalate - restart with sudo
                escalate_for_fix(check_id).await
            }
            RunningAs::Unknown => {
                return Err(anyhow!("Cannot determine privilege level"));
            }
        }
    } else {
        // No special privileges needed
        apply_fix(check).await
    }
}

async fn escalate_for_fix(check_id: &str) -> Result<()> {
    // Get current program path and arguments
    let current_exe = std::env::current_exe()?;
    let mut args: Vec<String> = std::env::args().collect();
    
    // Add internal flag to indicate privileged mode
    args.push("--internal-privileged-mode".to_string());
    
    println!("Requesting administrator privileges...");
    
    // Use sudo crate to restart with elevated privileges
    match sudo::with_env(&["RUST_BACKTRACE", "RUST_LOG"]) {
        Ok(_) => {
            // This should never be reached as process restarts
            unreachable!("sudo::with_env should restart the process");
        }
        Err(e) => {
            return Err(anyhow!("Failed to escalate privileges: {}", e));
        }
    }
}
```

### Privilege Validation
```rust
fn validate_privilege_requirements(
    check: &dyn DoctorCheck,
    is_fix: bool
) -> Result<(), PrivilegeError> {
    let required = if is_fix {
        check.fix_privilege_level()
    } else {
        check.check_privilege_level()
    };
    
    match (required, sudo::check()) {
        (PrivilegeLevel::Root, RunningAs::User) => {
            Err(PrivilegeError::NeedRoot)
        }
        (PrivilegeLevel::User, RunningAs::Root) => {
            Err(PrivilegeError::MustNotBeRoot)
        }
        (_, RunningAs::Unknown) => {
            Err(PrivilegeError::UnknownPrivilegeLevel)
        }
        _ => Ok(()) // Privileges match requirements
    }
}
```

## Dependencies Required

Add to `Cargo.toml`:
```toml
[dependencies]
sudo = "0.6"             # Process-based privilege escalation
anyhow = "1.0"           # Error handling
thiserror = "1.0"        # Custom error types
clap = { version = "4.0", features = ["derive"] }  # CLI parsing
```

## Platform Support

### Supported Platforms:
- **Linux** - Full support with native sudo integration
- **macOS** - Full support with system sudo
- **\*BSD** - Expected to work (not extensively tested)

### Requirements:
- `sudo` command must be installed and configured
- User must have sudo privileges for operations requiring root
- Terminal or TTY available for password prompts

### Limitations:
- **Windows** - Limited support (would need alternative approach)
- **Containerized environments** - May need special configuration
- **Headless systems** - Requires passwordless sudo or pre-authentication

## Testing Strategy

### Unit Tests
```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_privilege_detection() {
        // Test privilege level detection
        let current = sudo::check();
        assert!(matches!(current, RunningAs::User | RunningAs::Root | RunningAs::Unknown));
    }
    
    #[test] 
    fn test_check_validation() {
        let check = InstantRepoCheck::default();
        
        // Should pass validation when requirements match reality
        match sudo::check() {
            RunningAs::User => {
                // Fix requires root, should fail validation for fix
                assert!(validate_privilege_requirements(&check, true).is_err());
                // Check can run as any user, should pass
                assert!(validate_privilege_requirements(&check, false).is_ok());
            }
            RunningAs::Root => {
                // Both should pass when running as root
                assert!(validate_privilege_requirements(&check, true).is_ok());
                assert!(validate_privilege_requirements(&check, false).is_ok());
            }
            RunningAs::Unknown => {
                // Should fail when privilege level unknown
                assert!(validate_privilege_requirements(&check, true).is_err());
                assert!(validate_privilege_requirements(&check, false).is_err());
            }
        }
    }
}
```

### Integration Tests
- Test privilege escalation with actual sudo
- Test user cancellation scenarios  
- Test behavior when sudo is not available
- Test environment variable preservation
- Test internal privileged mode flag handling

## Troubleshooting

### Common Issues:

1. **"sudo command not found"**
   - Ensure sudo is installed: `apt install sudo` or equivalent
   - Check PATH includes sudo location

2. **"User is not in sudoers file"**
   - Add user to sudoers: `usermod -aG sudo username`
   - Configure sudo access for specific commands

3. **Password prompts in scripts**
   - Configure passwordless sudo for specific commands
   - Use `sudo -v` to pre-authenticate before running CLI

4. **Permission denied errors**
   - Check file ownership and permissions
   - Ensure target files are writable by root
   - Verify sudo configuration allows required operations

### Debug Tips:
- Enable `RUST_BACKTRACE=1` for better error traces
- Use `RUST_LOG=debug` for detailed logging
- Test privilege escalation manually first: `sudo instant doctor fix <check>`
