# Terminal Application Integration Plan

## Overview
Implement proper terminal application detection and launching for .desktop files that specify `Terminal=true`, following XDG Desktop Entry specification and matching fuzzel's implementation approach.

## Current State Analysis

### Problem Statement
Some .desktop files (like ncspot, htop, btop) indicate they need to run in a terminal, but InstantCLI doesn't properly detect and wrap these applications, causing them to fail to launch.

### Current Implementation
Based on codebase analysis, InstantCLI has:
- **Desktop file processing**: `src/launch/desktop.rs` with `freedesktop-file-parser`
- **Terminal abstraction**: `src/scratchpad/terminal.rs` with Kitty/Alacritty/Wezterm support
- **Execution system**: Basic field code expansion but no terminal detection

### Current Limitations
- **No terminal detection**: Doesn't check `Terminal=` key in .desktop files
- **No auto-wrapping**: Doesn't automatically wrap terminal applications
- **Limited field code support**: Missing some XDG field codes
- **No fallback mechanism**: No graceful handling of missing terminals

## Research Findings

### Fuzzel Implementation Analysis
Based on fuzzel manpage research:
- **`-T, --terminal=TERMINAL ARGS`**: Command to launch XDG applications with `Terminal=true`
- **Example usage**: `xterm -e` for terminal execution
- **Default behavior**: No terminal wrapping unless specified
- **Field code support**: Handles `%f`, `%u`, `%i`, `%c`, `%%` field codes

### XDG Desktop Entry Specification
Key requirements for terminal applications:
- **Terminal key**: `Terminal=true` indicates application needs terminal
- **Execution wrapping**: Must wrap `Exec=` string with terminal command
- **Field code expansion**: Handle special codes in `Exec=` strings
- **Fallback behavior**: Handle cases where terminal is not available

## Proposed Solution Architecture

# TODO

There is already a crate present for parsing .desktop files, can that be
used instead of implementing all of this?

### 1. Enhanced Desktop Entry Processing
```rust
#[derive(Debug, Clone)]
pub struct DesktopEntry {
    pub name: String,
    pub exec: String,
    pub terminal: bool,
    pub icon: Option<String>,
    pub categories: Vec<String>,
    pub comment: Option<String>,
    pub path: Option<PathBuf>,
    pub field_codes: Vec<FieldCode>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldCode {
    File,           // %f - Single file
    Files,          // %F - Multiple files
    Url,            // %u - Single URL
    Urls,           // %U - Multiple URLs
    Icon,           // %i - Icon name
    IconName,       // %k - Icon name (deprecated)
    Comment,        // %c - Comment/tooltip
    DesktopFile,    // %d - Desktop file path
    Percent,        // %% - Literal percent
}

impl DesktopEntry {
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let entry = freedesktop_file_parser::parse_entry(&content)?;

        let terminal = entry.get("Desktop Entry", "Terminal")
            .map(|s| s.to_lowercase() == "true")
            .unwrap_or(false);

        let exec = entry.get("Desktop Entry", "Exec")
            .ok_or_else(|| anyhow::anyhow!("Missing Exec key"))?;

        let field_codes = Self::extract_field_codes(exec);

        Ok(Self {
            name: entry.get("Desktop Entry", "Name").unwrap_or_default().to_string(),
            exec: exec.to_string(),
            terminal,
            icon: entry.get("Desktop Entry", "Icon").map(String::from),
            categories: entry.get("Desktop Entry", "Categories")
                .map(|cats| cats.split(';').filter(|s| !s.is_empty()).map(String::from).collect())
                .unwrap_or_default(),
            comment: entry.get("Desktop Entry", "Comment").map(String::from),
            path: entry.get("Desktop Entry", "Path").map(PathBuf::from),
            field_codes,
        })
    }

    fn extract_field_codes(exec_str: &str) -> Vec<FieldCode> {
        let mut codes = Vec::new();
        let mut chars = exec_str.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '%' {
                if let Some(next) = chars.next() {
                    match next {
                        'f' => codes.push(FieldCode::File),
                        'F' => codes.push(FieldCode::Files),
                        'u' => codes.push(FieldCode::Url),
                        'U' => codes.push(FieldCode::Urls),
                        'i' => codes.push(FieldCode::Icon),
                        'k' => codes.push(FieldCode::IconName),
                        'c' => codes.push(FieldCode::Comment),
                        'd' => codes.push(FieldCode::DesktopFile),
                        '%' => codes.push(FieldCode::Percent),
                        _ => {} // Unknown field code, ignore
                    }
                }
            }
        }

        codes
    }

    pub fn expand_field_codes(&self, files: &[PathBuf], urls: &[String]) -> Result<String> {
        let mut expanded = self.exec.clone();

        // Handle each field code type
        for code in &self.field_codes {
            let replacement = match code {
                FieldCode::File => {
                    files.first()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default()
                }
                FieldCode::Files => {
                    files.iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join(" ")
                }
                FieldCode::Url => {
                    urls.first()
                        .cloned()
                        .unwrap_or_default()
                }
                FieldCode::Urls => {
                    urls.join(" ")
                }
                FieldCode::Icon => {
                    self.icon.as_ref()
                        .map(|icon| format!("--icon {}", icon))
                        .unwrap_or_default()
                }
                FieldCode::IconName => {
                    self.icon.as_deref().unwrap_or_default().to_string()
                }
                FieldCode::Comment => {
                    self.comment.as_deref().unwrap_or_default().to_string()
                }
                FieldCode::DesktopFile => {
                    // This is complex, need to determine actual desktop file path
                    "unknown".to_string()
                }
                FieldCode::Percent => "%".to_string(),
            };

            expanded = expanded.replace(&format!("%{}", match code {
                FieldCode::File => "f",
                FieldCode::Files => "F",
                FieldCode::Url => "u",
                FieldCode::Urls => "U",
                FieldCode::Icon => "i",
                FieldCode::IconName => "k",
                FieldCode::Comment => "c",
                FieldCode::DesktopFile => "d",
                FieldCode::Percent => "%",
            }), &replacement);
        }

        Ok(expanded)
    }
}
```

### 2. Terminal Detection and Wrapping System
```rust
pub struct TerminalWrapper {
    pub terminal: Terminal,
    pub execute_flag: String,
    pub available: bool,
}

impl TerminalWrapper {
    pub fn new() -> Self {
        let terminal = Self::detect_terminal();
        let execute_flag = terminal.execute_flag().to_string();
        let available = Self::check_terminal_availability(&terminal);

        Self {
            terminal,
            execute_flag,
            available,
        }
    }

    fn detect_terminal() -> Terminal {
        // Check environment variables first
        if let Ok(term) = std::env::var("TERMINAL") {
            return Terminal::Other(term);
        }

        // Try common terminals in order of preference
        let terminals = vec![
            Terminal::Kitty,
            Terminal::Alacritty,
            Terminal::Wezterm,
            Terminal::Other("xterm".to_string()),
        ];

        for term in terminals {
            if Self::check_terminal_availability(&term) {
                return term;
            }
        }

        // Fallback to xterm
        Terminal::Other("xterm".to_string())
    }

    fn check_terminal_availability(terminal: &Terminal) -> bool {
        let cmd = terminal.command();

        match std::process::Command::new("which")
            .arg(&cmd)
            .output()
        {
            Ok(output) => output.status.success(),
            Err(_) => false,
        }
    }

    pub fn wrap_command(&self, command: &str) -> Result<Vec<String>> {
        if !self.available {
            return Err(anyhow::anyhow!("No terminal emulator available"));
        }

        let mut args = vec![self.terminal.command().to_string()];
        args.push(self.execute_flag.clone());
        args.push("sh".to_string());
        args.push("-c".to_string());
        args.push(format!("exec {}", command));

        Ok(args)
    }

    pub fn should_wrap(&self, desktop_entry: &DesktopEntry) -> bool {
        desktop_entry.terminal && self.available
    }
}
```

### 3. Enhanced Application Launcher
```rust
pub struct ApplicationLauncher {
    pub terminal_wrapper: TerminalWrapper,
    pub cache: LaunchCache,
}

impl ApplicationLauncher {
    pub fn new() -> Self {
        Self {
            terminal_wrapper: TerminalWrapper::new(),
            cache: LaunchCache::new(),
        }
    }

    pub async fn launch_desktop_entry(
        &self,
        entry: &DesktopEntry,
        files: &[PathBuf],
        urls: &[String],
    ) -> Result<()> {
        // Expand field codes in Exec string
        let exec_command = entry.expand_field_codes(files, urls)?;

        // Determine if terminal wrapping is needed
        let final_command = if self.terminal_wrapper.should_wrap(entry) {
            info!("Wrapping terminal application: {}", entry.name);
            self.terminal_wrapper.wrap_command(&exec_command)?
        } else {
            // Parse exec command into args
            self.parse_exec_command(&exec_command)?
        };

        // Set working directory if specified
        let working_dir = entry.path.as_ref()
            .filter(|p| p.exists())
            .cloned();

        // Execute the command
        self.execute_command(&final_command, working_dir).await?;

        Ok(())
    }

    fn parse_exec_command(&self, exec_str: &str) -> Result<Vec<String>> {
        // Simple parsing - split on spaces but respect quotes
        let mut args = Vec::new();
        let mut current = String::new();
        let mut in_quotes = false;
        let mut escape_next = false;

        for c in exec_str.chars() {
            if escape_next {
                current.push(c);
                escape_next = false;
            } else if c == '\\' {
                escape_next = true;
            } else if c == '"' {
                in_quotes = !in_quotes;
            } else if c.is_whitespace() && !in_quotes {
                if !current.is_empty() {
                    args.push(current.clone());
                    current.clear();
                }
            } else {
                current.push(c);
            }
        }

        if !current.is_empty() {
            args.push(current);
        }

        Ok(args)
    }

    async fn execute_command(
        &self,
        args: &[String],
        working_dir: Option<PathBuf>,
    ) -> Result<()> {
        let mut command = tokio::process::Command::new(&args[0]);

        // Add remaining arguments
        command.args(&args[1..]);

        // Set working directory
        if let Some(wd) = working_dir {
            command.current_dir(wd);
        }

        // Execute in background
        command.spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn command: {}", e))?;

        Ok(())
    }
}
```

### 4. Configuration Integration
```rust
#[derive(Debug, Clone, serde::Deserialize)]
pub struct TerminalConfig {
    pub preferred_terminal: Option<String>,
    pub execute_flag: Option<String>,
    pub fallback_terminals: Vec<String>,
    pub auto_wrap: bool,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            preferred_terminal: None,
            execute_flag: None,
            fallback_terminals: vec![
                "kitty".to_string(),
                "alacritty".to_string(),
                "wezterm".to_string(),
                "xterm".to_string(),
            ],
            auto_wrap: true,
        }
    }
}

impl TerminalWrapper {
    pub fn with_config(config: &TerminalConfig) -> Self {
        let terminal = if let Some(ref preferred) = config.preferred_terminal {
            Terminal::Other(preferred.clone())
        } else {
            Self::detect_terminal()
        };

        let execute_flag = config.execute_flag
            .as_ref()
            .cloned()
            .unwrap_or_else(|| terminal.execute_flag().to_string());

        let available = Self::check_terminal_availability(&terminal);

        // Try fallbacks if preferred is not available
        let final_terminal = if !available {
            for fallback in &config.fallback_terminals {
                let fallback_term = Terminal::Other(fallback.clone());
                if Self::check_terminal_availability(&fallback_term) {
                    break fallback_term;
                }
            }
            // Fallback to xterm as last resort
            Terminal::Other("xterm".to_string())
        } else {
            terminal
        };

        Self {
            terminal: final_terminal,
            execute_flag,
            available: true,
        }
    }
}
```

### 5. Integration with Existing Launch System
```rust
// Integration with src/launch/desktop.rs
impl LaunchItem for DesktopEntry {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn description(&self) -> Option<String> {
        self.comment.clone()
    }

    fn icon(&self) -> Option<String> {
        self.icon.clone()
    }

    async fn launch(&self, context: &LaunchContext) -> Result<()> {
        let launcher = ApplicationLauncher::new();
        launcher.launch_desktop_entry(self, &context.files, &context.urls).await
    }

    fn is_terminal_app(&self) -> bool {
        self.terminal
    }
}

#[derive(Debug, Clone)]
pub struct LaunchContext {
    pub files: Vec<PathBuf>,
    pub urls: Vec<String>,
    pub working_dir: Option<PathBuf>,
}
```

## Implementation Plan

### Phase 1: Terminal Detection (Week 1)
1. **Implement TerminalWrapper**
   - Terminal detection logic
   - Availability checking
   - Configuration integration

2. **Enhanced DesktopEntry processing**
   - Terminal key detection
   - Field code extraction
   - Expansion logic

### Phase 2: Application Launching (Week 2)
1. **ApplicationLauncher implementation**
   - Command parsing and execution
   - Terminal wrapping
   - Working directory handling

2. **Integration with existing code**
   - Replace current desktop file handling
   - Update launch caching
   - Add error handling

### Phase 3: Advanced Features (Week 3)
1. **Enhanced field code support**
   - Complete XDG specification compliance
   - Advanced escaping and quoting
   - File/URL handling

2. **Configuration and customization**
   - User terminal preferences
   - Fallback mechanisms
   - Performance optimization

## Technical Considerations

### Dependencies
```toml
[dependencies]
freedesktop-file-parser = "0.2"
serde = { version = "1.0", features = ["derive"] }
tokio = { version = "1.0", features = ["full"] }
anyhow = "1.0"
```

### Error Handling
- **Graceful degradation**: Work without terminals
- **Clear error messages**: Specific terminal application errors
- **Fallback mechanisms**: Multiple terminal options
- **User feedback**: Show when terminal wrapping is applied

### Performance
- **Caching**: Cache terminal detection results
- **Lazy loading**: Only detect terminals when needed
- **Minimal overhead**: Fast field code expansion
- **Background execution**: Non-blocking launches

## Testing Strategy

### Unit Tests
- Terminal detection logic
- Field code expansion
- Command parsing
- Configuration handling

### Integration Tests
- Real .desktop file processing
- Terminal application launching
- Error scenario handling
- Configuration validation

### Manual Testing
- Terminal applications (ncspot, htop, etc.)
- GUI applications
- Mixed field code scenarios
- Various terminal emulators

## Success Metrics

- **Compatibility**: Launch all terminal applications correctly
- **Reliability**: Graceful handling of missing terminals
- **Performance**: Minimal delay in application launching
- **User experience**: Seamless integration with existing launcher

## Future Enhancements

- **Advanced terminal features**: Tabs, splits, profiles
- **Session management**: Terminal session persistence
- **Custom terminals**: User-defined terminal configurations
- **Remote terminals**: SSH terminal support
- **Containerization**: Terminal application sandboxing
