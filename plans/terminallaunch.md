# Terminal Application Integration Plan

## Overview
Enhance the existing terminal application launching system to improve XDG compliance, field code support, and terminal detection.

## Current State Analysis

### Problem Statement
Some .desktop files (like ncspot, htop, btop) indicate they need to run in a terminal, and while InstantCLI already has basic terminal wrapping, the implementation could be more robust and XDG-compliant.

### Existing Implementation - Already Working!
InstantCLI already has a comprehensive terminal application launching system:

#### Desktop File Processing (`src/launch/desktop.rs`)
- **Parser**: Uses `freedesktop-file-parser` crate for XDG desktop entry parsing
- **Terminal detection**: Already extracts `Terminal=` key from .desktop files (line 42)
- **Field code expansion**: Basic implementation in `expand_exec_field_codes()` (lines 147-176)
- **Execution flow**: `execute_launch_item()` -> `execute_desktop_app()` -> `wrap_with_terminal()`

#### Terminal Abstraction (`src/scratchpad/terminal.rs`)
- **Terminal enum**: Supports Kitty, Alacritty, Wezterm, and custom terminals
- **Command mapping**: Proper command names and execute flags (`-e`)
- **Class flags**: Window class handling for scratchpad integration

#### Terminal Wrapping (`src/launch/desktop.rs:178-221`)
- **Terminal detection**: Environment variable + fallback detection
- **Command wrapping**: Properly wraps commands with terminal emulators
- **Fallback handling**: Multiple terminal support with xterm fallback

### Current Implementation Details
```rust
// Terminal detection already exists in src/launch/desktop.rs:42
let terminal = app.terminal.unwrap_or(false);

// Terminal wrapping already implemented in src/launch/desktop.rs:133-135
if details.terminal {
    wrap_with_terminal(&mut cmd)?;
}

// Field code expansion already exists in src/launch/desktop.rs:147-176
fn expand_exec_field_codes(exec: &str) -> Result<String> {
    // Handles %% -> %, removes unsupported field codes
}
```

### Current Limitations
1. **Limited field code support**: Only handles `%%` properly, removes other codes
2. **Basic terminal detection**: Doesn't use existing `Terminal` enum abstraction
3. **No field code context**: Missing file/URL argument support
4. **Suboptimal terminal integration**: Could leverage existing terminal infrastructure better

## Root Cause Analysis

### Field Code Handling Issues
Current implementation removes most field codes instead of properly expanding them:
- `%f`, `%F`, `%u`, `%U`: File/URL arguments (removed instead of expanded)
- `%c`: Application comment/tooltip (removed)
- `%i`: Icon name (removed)
- `%k`: Desktop file path (removed)

### Terminal Detection Issues
Current implementation in `wrap_with_terminal()` doesn't leverage the existing `Terminal` enum from `src/scratchpad/terminal.rs`.

## Proposed Enhancement Strategy

### Enhancement 1: Leverage Existing Terminal Abstraction
Instead of creating new terminal detection, use the existing `Terminal` enum.

#### Enhancement Concept
```rust
// Enhance existing wrap_with_terminal to use Terminal enum
fn wrap_with_terminal_enhanced(cmd: &mut std::process::Command) -> Result<()> {
    use crate::scratchpad::terminal::Terminal;

    // Use existing terminal abstraction
    let terminal = Self::detect_terminal()?;

    // Build terminal command using existing enum
    let mut term_cmd = std::process::Command::new(terminal.command());

    // Use existing execute_flag
    term_cmd.arg(terminal.execute_flag());

    // Add original command
    let program = cmd.get_program().to_string_lossy().to_string();
    let args: Vec<String> = cmd.get_args()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect();

    term_cmd.arg(program);
    for arg in args {
        term_cmd.arg(arg);
    }

    *cmd = term_cmd;
    Ok(())
}
```

### Enhancement 2: Improved Field Code Support
Enhance existing `expand_exec_field_codes()` to support more XDG field codes.

#### Enhancement Concept
```rust
// Enhance existing field code expansion
fn expand_exec_field_codes_enhanced(
    exec: &str,
    files: &[PathBuf],
    urls: &[String],
    app_details: &DesktopAppDetails
) -> Result<String> {
    let mut expanded = exec.to_string();

    // Handle %% -> %
    expanded = expanded.replace("%%", "%");

    // Handle %c -> application comment
    if let Some(ref comment) = app_details.comment {
        expanded = expanded.replace("%c", comment);
    } else {
        expanded = expanded.replace("%c", "");
    }

    // Handle %f -> single file
    if let Some(file) = files.first() {
        expanded = expanded.replace("%f", &file.display().to_string());
    } else {
        expanded = expanded.replace("%f", "");
    }

    // Handle %F -> multiple files
    if !files.is_empty() {
        let file_list: Vec<String> = files.iter()
            .map(|f| f.display().to_string())
            .collect();
        expanded = expanded.replace("%F", &file_list.join(" "));
    } else {
        expanded = expanded.replace("%F", "");
    }

    // Handle %i -> icon name
    if let Some(ref icon) = app_details.icon {
        expanded = expanded.replace("%i", &format!("--icon {}", icon));
    } else {
        expanded = expanded.replace("%i", "");
    }

    // Clean up spacing
    while expanded.contains("  ") {
        expanded = expanded.replace("  ", " ");
    }
    expanded.trim().to_string()
}
```

### Enhancement 3: Better Terminal Detection
Use existing terminal detection logic from scratchpad module.

#### Enhancement Concept
```rust
// Terminal detection using existing infrastructure
fn detect_terminal() -> Result<Terminal> {
    use crate::scratchpad::terminal::Terminal;

    // Check environment variable first
    if let Ok(term_env) = std::env::var("TERMINAL") {
        return Ok(Terminal::from(term_env));
    }

    // Use existing terminal detection logic
    let terminals = vec![
        Terminal::Kitty,
        Terminal::Alacritty,
        Terminal::Wezterm,
        Terminal::Other("gnome-terminal".to_string()),
        Terminal::Other("xterm".to_string()),
    ];

    for terminal in terminals {
        if which::which(terminal.command()).is_ok() {
            return Ok(terminal);
        }
    }

    // Fallback
    Ok(Terminal::Other("xterm".to_string()))
}
```

## Implementation Strategy

### Phase 1: Integration Improvements (Week 1)
1. **Integrate existing Terminal enum**
   - Update `wrap_with_terminal()` to use `Terminal` enum
   - Leverage existing execute flags and command mappings
   - Test with existing terminal emulators

2. **Enhance field code support**
   - Improve `expand_exec_field_codes()` with better context
   - Add support for file arguments and icon names
   - Maintain backwards compatibility

### Phase 2: Context Enhancement (Week 2)
1. **Launch context enhancement**
   - Add file/URL argument support to launch system
   - Pass context to field code expansion
   - Test with real .desktop files that use field codes

2. **Error handling improvements**
   - Better error messages for terminal detection failures
   - Graceful fallback mechanisms
   - User feedback for missing terminals

### Phase 3: Advanced Features (Week 3)
1. **Configuration integration**
   - Add user terminal preferences to config
   - Custom terminal command support
   - Performance optimizations

2. **Testing and validation**
   - Test with various terminal applications
   - Validate XDG specification compliance
   - Performance benchmarking

## Technical Considerations

### Dependencies
- **No new dependencies needed**: Use existing `freedesktop-file-parser` and terminal infrastructure
- **Leverage existing code**: Build on current implementation rather than rewriting
- **Backwards compatibility**: Maintain existing behavior while enhancing

### Performance Impact
- **Minimal overhead**: Slight improvement from using existing terminal enum
- **Better caching**: Potential for caching terminal detection results
- **Reduced duplication**: Reusing existing terminal abstraction

### Error Handling
- **Graceful degradation**: Fall back to current implementation if enhancements fail
- **Clear error messages**: Indicate terminal detection issues
- **User feedback**: Show when terminal wrapping is applied

## Integration with Existing Code

### Current Desktop Execution Flow
```rust
// Enhance existing execute_desktop_app function
async fn execute_desktop_app_enhanced(details: &DesktopAppDetails) -> Result<()> {
    // Parse exec string with enhanced field code support
    let exec_expanded = expand_exec_field_codes_enhanced(
        &details.exec,
        &[], // TODO: Pass file arguments from launch context
        &[], // TODO: Pass URL arguments from launch context
        details
    )?;

    // Build command
    let mut cmd = std::process::Command::new(parse_exec_command(&exec_expanded)?);

    // Use enhanced terminal wrapping
    if details.terminal {
        wrap_with_terminal_enhanced(&mut cmd)?;
    }

    // Execute (existing code)
    cmd.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()?;

    Ok(())
}
```

## Testing Strategy

### Unit Tests
- Enhanced field code expansion with various scenarios
- Terminal detection using existing enum
- Command parsing and wrapping

### Integration Tests
- Real .desktop file processing with terminal applications
- Terminal emulator compatibility testing
- Field code expansion with file arguments

### Manual Testing
- Terminal applications (ncspot, htop, btop, etc.)
- GUI applications to ensure no regression
- Various terminal emulators

## Success Metrics

- **Compatibility**: Launch all terminal applications correctly
- **XDG compliance**: Better support for XDG field codes
- **Code quality**: Reduced duplication through better integration
- **User experience**: More reliable terminal application launching

## Future Enhancements

- **Advanced field codes**: Complete XDG specification support
- **User preferences**: Configurable terminal selection
- **Session management**: Terminal session persistence
- **Performance**: Terminal detection caching

## TODO Notes from Review

### Issues with Current Plan
- Original plan was completely over-engineered
- Proposed building entire new systems when existing infrastructure already exists
- Didn't leverage existing `freedesktop-file-parser` usage
- Ignored existing terminal abstraction in `src/scratchpad/terminal.rs`

### Key Insight
Terminal application launching is already implemented! The system just needs enhancement, not replacement.

### Existing Infrastructure to Leverage
- **Desktop file parsing**: `freedesktop-file-parser` crate already in use
- **Terminal abstraction**: `Terminal` enum with execute flags already exists
- **Terminal wrapping**: `wrap_with_terminal()` function already implemented
- **Field code expansion**: Basic implementation already exists

### Simplification Approach
- Enhance existing systems instead of building new ones
- Use existing `Terminal` enum for better terminal detection
- Improve field code expansion with proper context
- Integrate better with existing launch flow