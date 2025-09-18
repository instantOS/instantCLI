# Desktop File Support Implementation Plan

## Overview
Plan to extend the InstantCLI launcher to support both XDG desktop files and PATH executables, with proper name conflict resolution and frecency tracking.

## Phase 1: Core Infrastructure (2-3 days)

### 1.1 Add Dependencies
```toml
[dependencies]
freedesktop-file-parser = "0.3.0"
# Additional dependencies may be needed for icon support
```

### 1.2 Create Core Data Structures
```rust
// src/launch/types.rs
pub enum LaunchItem {
    DesktopApp(DesktopApp),
    PathExecutable(PathExecutable),
}

pub struct DesktopApp {
    pub desktop_id: String,           // e.g., "firefox.desktop"
    pub name: String,                 // Localized name
    pub display_name: String,         // Name for UI display
    pub exec: String,                 // Exec command with field codes
    pub icon: Option<String>,         // Icon name
    pub categories: Vec<String>,      // Application categories
    pub no_display: bool,            // Should be hidden
    pub terminal: bool,               // Run in terminal
    pub file_path: PathBuf,           // Path to .desktop file
}

pub struct PathExecutable {
    pub name: String,                 // Executable name
    pub display_name: String,         // Name for UI display
    pub path: PathBuf,               // Full path to executable
}

impl LaunchItem {
    pub fn display_name(&self) -> &str {
        match self {
            LaunchItem::DesktopApp(app) => &app.display_name,
            LaunchItem::PathExecutable(exe) => &exe.display_name,
        }
    }

    pub fn sort_key(&self) -> String {
        match self {
            LaunchItem::DesktopApp(app) => app.name.to_lowercase(),
            LaunchItem::PathExecutable(exe) => exe.name.to_lowercase(),
        }
    }
}
```

### 1.3 Extend Cache System
```rust
// src/launch/cache.rs - Additions
impl LaunchCache {
    /// Get all launch items (desktop apps + PATH executables)
    pub async fn get_launch_items(&mut self) -> Result<Vec<LaunchItem>> {
        // Check cache freshness for both desktop files and PATH
        if self.is_launch_cache_fresh()? {
            let mut items = self.read_launch_cache()?;
            self.sort_by_frecency_launch_items(&mut items)?;
            Ok(items)
        } else {
            // Return stale cache while refreshing in background
            let stale_items = self.read_launch_cache().unwrap_or_default();

            // Background refresh
            let cache_path = self.cache_path.clone();
            task::spawn(async move {
                if let Err(e) = Self::refresh_launch_cache_background(cache_path).await {
                    eprintln!("Warning: Failed to refresh launch cache: {}", e);
                }
            });

            // Return items or do quick scan if empty
            let mut items = if stale_items.is_empty() {
                self.quick_scan_launch_items()?
            } else {
                stale_items
            };

            self.sort_by_frecency_launch_items(&mut items)?;
            Ok(items)
        }
    }

    /// Discover desktop files from XDG directories
    fn discover_desktop_files(&self) -> Result<Vec<DesktopApp>> {
        let mut apps = Vec::new();

        // Get XDG data directories
        let data_dirs = self.get_xdg_data_dirs();

        for data_dir in data_dirs {
            let apps_dir = data_dir.join("applications");
            if apps_dir.exists() {
                self.scan_desktop_directory(&apps_dir, &mut apps)?;
            }
        }

        Ok(apps)
    }

    /// Get XDG data directories in correct priority order
    fn get_xdg_data_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();

        // User-specific directory (highest priority)
        if let Some(home_data) = dirs::data_dir() {
            dirs.push(home_data);
        }

        // System directories
        if let Ok(system_dirs) = env::var("XDG_DATA_DIRS") {
            for dir in system_dirs.split(':') {
                if !dir.is_empty() {
                    dirs.push(PathBuf::from(dir));
                }
            }
        } else {
            // Default system directories
            dirs.push(PathBuf::from("/usr/local/share"));
            dirs.push(PathBuf::from("/usr/share"));
        }

        dirs
    }
}
```

## Phase 2: Desktop File Discovery and Parsing (2-3 days)

### 2.1 Desktop File Scanner
```rust
// src/launch/desktop.rs
use freedesktop_file_parser::{parse, EntryType};

pub fn scan_desktop_directory(
    apps_dir: &Path,
    apps: &mut Vec<DesktopApp>
) -> Result<()> {
    for entry in fs::read_dir(apps_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("desktop") {
            if let Ok(app) = parse_desktop_file(&path)? {
                apps.push(app);
            }
        }
    }
    Ok(())
}

pub fn parse_desktop_file(path: &Path) -> Result<DesktopApp> {
    let content = fs::read_to_string(path)?;
    let desktop_file = parse(&content)?;

    // Extract desktop ID from file path
    let desktop_id = path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let name = match &desktop_file.entry.entry_type {
        EntryType::Application(app) => app.name.default.clone(),
        _ => desktop_id.clone(), // Fallback for non-application types
    };

    let exec = match &desktop_file.entry.entry_type {
        EntryType::Application(app) => app.exec.clone().unwrap_or_default(),
        _ => String::new(),
    };

    Ok(DesktopApp {
        desktop_id,
        name: name.clone(),
        display_name: name,
        exec,
        icon: desktop_file.entry.icon.default,
        categories: desktop_file.entry.categories.unwrap_or_default(),
        no_display: desktop_file.entry.no_display.unwrap_or(false),
        terminal: desktop_file.entry.terminal.unwrap_or(false),
        file_path: path.to_path_buf(),
    })
}
```

### 2.2 Name Conflict Resolution
```rust
// src/launch/conflict.rs
pub fn resolve_name_conflicts(
    desktop_apps: Vec<DesktopApp>,
    path_execs: Vec<PathExecutable>
) -> Vec<LaunchItem> {
    let mut items = Vec::new();
    let desktop_names: std::collections::HashSet<_> = desktop_apps
        .iter()
        .map(|app| app.name.to_lowercase())
        .collect();

    // Add desktop apps
    for app in desktop_apps {
        items.push(LaunchItem::DesktopApp(app));
    }

    // Add PATH executables with prefix if needed
    for exec in path_execs {
        let display_name = if desktop_names.contains(&exec.name.to_lowercase()) {
            format!("path:{}", exec.name)
        } else {
            exec.name.clone()
        };

        items.push(LaunchItem::PathExecutable(PathExecutable {
            name: exec.name,
            display_name,
            path: exec.path,
        }));
    }

    items
}
```

## Phase 3: Execution and Field Code Handling (2 days)

### 3.1 Enhanced Execution System
```rust
// src/launch/execute.rs
use std::process::Command;

pub fn execute_launch_item(item: &LaunchItem) -> Result<()> {
    match item {
        LaunchItem::DesktopApp(app) => execute_desktop_app(app),
        LaunchItem::PathExecutable(exe) => execute_path_executable(exe),
    }
}

fn execute_desktop_app(app: &DesktopApp) -> Result<()> {
    // Parse and expand field codes in Exec string
    let exec_cmd = expand_exec_field_codes(&app.exec, app)?;

    // Split into command and arguments
    let parts: Vec<&str> = exec_cmd.split_whitespace().collect();
    if parts.is_empty() {
        return Err(anyhow::anyhow!("Empty Exec command"));
    }

    let mut cmd = Command::new(parts[0]);

    // Add remaining arguments
    for arg in &parts[1..] {
        cmd.arg(arg);
    }

    // Set working directory if specified
    if let Some(ref path_str) = app.path {
        cmd.current_dir(path_str);
    }

    // Handle terminal execution
    if app.terminal {
        // Wrap with terminal command
        wrap_with_terminal(&mut cmd)?;
    }

    // Execute in background
    cmd.stdout(std::process::Stdio::null())
       .stderr(std::process::Stdio::null())
       .stdin(std::process::Stdio::null())
       .spawn()
       .map_err(|e| anyhow::anyhow!("Failed to launch desktop app: {}", e))?;

    println!("Launched: {}", app.display_name);
    Ok(())
}

fn execute_path_executable(exe: &PathExecutable) -> Result<()> {
    let mut cmd = Command::new(&exe.path);

    cmd.stdout(std::process::Stdio::null())
       .stderr(std::process::Stdio::null())
       .stdin(std::process::Stdio::null())
       .spawn()
       .map_err(|e| anyhow::anyhow!("Failed to launch path executable: {}", e))?;

    println!("Launched: {}", exe.display_name);
    Ok(())
}

fn expand_exec_field_codes(exec: &str, app: &DesktopApp) -> Result<String> {
    // For launcher use, we'll implement basic field code expansion
    // Focus on %% (literal %) and %c (app name) for now
    let mut expanded = exec.to_string();

    // Handle %% -> %
    expanded = expanded.replace("%%", "%");

    // Handle %c -> application name
    expanded = expanded.replace("%c", &app.name);

    // TODO: Add support for other field codes as needed
    // %f, %F, %u, %U, %i, %k

    Ok(expanded)
}
```

## Phase 4: Frecency Integration (1-2 days)

### 4.1 Extended Frecency System
```rust
// src/launch/cache.rs - Frecency Extensions
impl LaunchCache {
    fn sort_by_frecency_launch_items(&mut self, items: &mut Vec<LaunchItem>) -> Result<()> {
        let frecency_store = self.get_frecency_store()?;
        let sorted_items = frecency_store.sorted(SortMethod::Frecent);

        let frequent_keys: std::collections::HashSet<_> = sorted_items
            .iter()
            .map(|item| &item.item)
            .collect();

        items.sort_by(|a, b| {
            let a_key = self.get_frecency_key(a);
            let b_key = self.get_frecency_key(b);

            let a_is_frequent = frequent_keys.contains(&a_key);
            let b_is_frequent = frequent_keys.contains(&b_key);

            match (a_is_frequent, b_is_frequent) {
                (true, true) => {
                    let a_index = sorted_items.iter().position(|item| &item.item == &a_key).unwrap_or(0);
                    let b_index = sorted_items.iter().position(|item| &item.item == &b_key).unwrap_or(0);
                    a_index.cmp(&b_index)
                }
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                (false, false) => a.sort_key().cmp(&b.sort_key()),
            }
        });

        Ok(())
    }

    fn get_frecency_key(&self, item: &LaunchItem) -> String {
        match item {
            LaunchItem::DesktopApp(app) => format!("desktop:{}", app.desktop_id),
            LaunchItem::PathExecutable(exe) => format!("path:{}", exe.name),
        }
    }

    pub fn record_launch_item(&mut self, item: &LaunchItem) -> Result<()> {
        let key = self.get_frecency_key(item);
        let frecency_store = self.get_frecency_store()?;
        frecency_store.add(&key);
        self.save_frecency_store()?;
        Ok(())
    }
}
```

## Phase 5: Integration and Testing (2 days)

### 5.1 Update Main Launcher
```rust
// src/launch/mod.rs - Updates
pub async fn handle_launch_command() -> Result<i32> {
    let mut cache = LaunchCache::new()?;

    // Get all launch items (desktop apps + PATH executables)
    let launch_items = cache.get_launch_items().await?;

    // Convert to menu items
    let menu_items: Vec<SerializableMenuItem> = launch_items
        .into_iter()
        .map(|item| {
            let display_text = item.display_name().to_string();
            // Add metadata to distinguish item types for execution
            let metadata = match item {
                LaunchItem::DesktopApp(_) => Some("desktop".to_string()),
                LaunchItem::PathExecutable(_) => Some("path".to_string()),
            };

            SerializableMenuItem {
                display_text,
                preview: FzfPreview::None,
                metadata,
            }
        })
        .collect();

    // Use GUI menu to select application
    let client = client::MenuClient::new();
    client.ensure_server_running()?;

    match client.choice("Launch application:".to_string(), menu_items, false) {
        Ok(selected) => {
            if selected.is_empty() {
                Ok(1) // Cancelled
            } else {
                // Reconstruct launch item from selection
                // This needs more sophisticated metadata handling
                let item_name = &selected[0].display_text;

                // Find the corresponding launch item
                let launch_items = cache.get_launch_items().await?;
                let item = launch_items.iter()
                    .find(|item| item.display_name() == item_name)
                    .ok_or_else(|| anyhow::anyhow!("Selected item not found"))?;

                execute_launch_item(item)?;

                if let Err(e) = cache.record_launch_item(item) {
                    eprintln!("Warning: Failed to record launch: {}", e);
                }

                Ok(0)
            }
        }
        Err(e) => {
            eprintln!("Error showing menu: {e}");
            Ok(2)
        }
    }
}
```

## Testing Strategy

### Unit Tests
- Desktop file parsing
- Name conflict resolution
- Field code expansion
- Frecency key generation

### Integration Tests
- Discovery of desktop files from XDG directories
- Integration with existing cache system
- Execution of both desktop apps and PATH executables

### Manual Testing
- Real-world desktop files
- Name conflict scenarios
- Frecency tracking behavior
- Performance with large application sets

## Performance Considerations

1. **Background Scanning**: Desktop file discovery should happen in background
2. **Cache Invalidation**: Track modification times of XDG directories
3. **Memory Usage**: Desktop file parsing should be memory-efficient
4. **Startup Time**: Fast path for cached data availability

## Error Handling

1. **Graceful Degradation**: If desktop files fail, fall back to PATH-only mode
2. **Corrupted Files**: Skip corrupted .desktop files with warnings
3. **Permission Errors**: Handle permission issues gracefully
4. **Missing Dependencies**: Continue working if desktop file parsing fails

## Future Enhancements

1. **Icon Support**: Add icon display in the launcher
2. **Desktop Actions**: Support additional application actions
3. **Filtering**: Filter by desktop environment, categories
4. **Search Enhancement**: Search in multiple fields (name, comment, categories)
5. **Configuration**: User preferences for desktop file handling

## Risk Assessment

### High Risk
- Breaking existing PATH-only functionality
- Performance regression
- Complex name conflict resolution

### Mitigation
- Extensive testing of existing functionality
- Performance benchmarks
- Clear documentation of behavior changes