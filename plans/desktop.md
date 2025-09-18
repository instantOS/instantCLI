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

### 1.2 Create Lightweight Data Structures
```rust
// src/launch/types.rs - Performance-optimized design
// Lightweight enum containing only display name and identifier
pub enum LaunchItem {
    DesktopApp(String),    // desktop_id (e.g., "firefox.desktop")
    PathExecutable(String), // executable name
}

// Desktop app details loaded lazily when needed for execution
#[derive(Default)]
pub struct DesktopAppDetails {
    pub exec: String,                 // Exec command with field codes
    pub icon: Option<String>,         // Icon name
    pub categories: Vec<String>,      // Application categories
    pub no_display: bool,            // Should be hidden
    pub terminal: bool,               // Run in terminal
    pub file_path: PathBuf,           // Path to .desktop file
}

// Path executable details loaded lazily when needed
#[derive(Default)]
pub struct PathExecutableDetails {
    pub path: PathBuf,               // Full path to executable
}

impl LaunchItem {
    pub fn display_name(&self) -> &str {
        match self {
            LaunchItem::DesktopApp(id) => {
                // Extract name from desktop_id (remove .desktop suffix)
                id.strip_suffix(".desktop").unwrap_or(id)
            }
            PathExecutable(name) => name,
        }
    }

    pub fn sort_key(&self) -> String {
        self.display_name().to_lowercase()
    }
}
```

### 1.3 Simple High-Performance Cache
```rust
// src/launch/cache.rs - Simple and fast cache system
impl LaunchCache {
    /// Get launch items - extremely fast path for menu display
    pub async fn get_launch_items(&mut self) -> Result<Vec<LaunchItem>> {
        // Always return immediately, never wait
        let cached_items = self.read_launch_cache().unwrap_or_default();

        // Background refresh if stale (non-blocking)
        if !self.is_launch_cache_fresh()? {
            self.trigger_background_refresh();
        }

        // Apply frecency sorting and return
        let mut items = cached_items;
        self.sort_by_frecency_launch_items(&mut items)?;
        Ok(items)
    }

    /// Simple background refresh
    fn trigger_background_refresh(&self) {
        let cache_path = self.cache_path.clone();
        task::spawn(async move {
            // Simple scan for display names only
            let items = Self::build_item_list_simple().await;
            if let Err(e) = Self::save_cache_simple(cache_path, items) {
                eprintln!("Warning: Failed to refresh cache: {}", e);
            }
        });
    }

    /// Build item list with minimal overhead
    async fn build_item_list_simple() -> Vec<LaunchItem> {
        let mut items = Vec::new();

        // Get desktop app names (fast)
        items.extend(Self::get_desktop_names_fast());

        // Get PATH executables (fast)
        items.extend(Self::get_path_names_fast());

        // Simple conflict resolution
        Self::resolve_conflicts_simple(items)
    }

    /// Fast desktop name scanning - no parsing, just file names
    fn get_desktop_names_fast() -> Vec<LaunchItem> {
        let mut names = Vec::new();
        let data_dirs = Self::get_xdg_data_dirs();

        for data_dir in data_dirs {
            let apps_dir = data_dir.join("applications");
            if apps_dir.exists() {
                Self::scan_desktop_names_simple(&apps_dir, &mut names);
            }
        }

        names
    }

    /// Get XDG data directories
    fn get_xdg_data_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();

        if let Some(home_data) = dirs::data_dir() {
            dirs.push(home_data);
        }

        if let Ok(system_dirs) = env::var("XDG_DATA_DIRS") {
            for dir in system_dirs.split(':') {
                if !dir.is_empty() {
                    dirs.push(PathBuf::from(dir));
                }
            }
        } else {
            dirs.push(PathBuf::from("/usr/local/share"));
            dirs.push(PathBuf::from("/usr/share"));
        }

        dirs
    }

    /// Simple name scanning - skip parsing entirely
    fn scan_desktop_names_simple(apps_dir: &Path, names: &mut Vec<LaunchItem>) {
        if let Ok(entries) = fs::read_dir(apps_dir) {
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("desktop") {
                    if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                        // Skip obvious test/debug files by filename
                        if !file_name.contains("test") && !file_name.contains("debug") {
                            names.push(LaunchItem::DesktopApp(file_name.to_string()));
                        }
                    }
                }
            }
        }
    }
}
```

## Phase 2: Lazy Loading and Execution (2-3 days)

### 2.1 Lazy Loading System
```rust
// src/launch/lazy.rs - Performance-optimized lazy loading
impl LaunchCache {
    /// Load desktop app details only when needed for execution
    pub async fn get_desktop_details(&self, desktop_id: &str) -> Result<DesktopAppDetails> {
        // Check if already loaded and cached
        if let Some(cached) = self.get_cached_desktop_details(desktop_id)? {
            return Ok(cached);
        }

        // Find and parse the desktop file on demand
        let details = self.load_and_parse_desktop_file(desktop_id).await?;
        self.cache_desktop_details(desktop_id, &details)?;
        Ok(details)
    }

    /// Load path executable details only when needed
    pub async fn get_path_details(&self, name: &str) -> Result<PathExecutableDetails> {
        if let Some(cached) = self.get_cached_path_details(name)? {
            return Ok(cached);
        }

        // Resolve PATH on demand
        let path = self.resolve_executable_path(name)?;
        let details = PathExecutableDetails { path };
        self.cache_path_details(name, &details)?;
        Ok(details)
    }

    /// Parse desktop file only when actually needed for execution
    async fn load_and_parse_desktop_file(&self, desktop_id: &str) -> Result<DesktopAppDetails> {
        let file_path = self.find_desktop_file_path(desktop_id).await?;
        let content = fs::read_to_string(&file_path)?;
        let desktop_file = parse(&content)?;

        Ok(DesktopAppDetails {
            exec: self.extract_exec_command(&desktop_file),
            icon: desktop_file.entry.icon.default,
            categories: desktop_file.entry.categories.unwrap_or_default(),
            no_display: desktop_file.entry.no_display.unwrap_or(false),
            terminal: desktop_file.entry.terminal.unwrap_or(false),
            file_path,
        })
    }
}
```

### 2.2 Desktop File Discovery
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
            LaunchItem::DesktopApp(desktop_id) => desktop_id.clone(),
            LaunchItem::PathExecutable(name) => name.clone(),
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

1. **Immediate Response**: Always return cached data immediately, never block for scanning
2. **Background Refresh**: Update cache asynchronously without affecting user experience
3. **Lightweight Data Structures**: Store only strings in cache, parse details on-demand
4. **Minimal Scanning**: Skip desktop file parsing entirely during menu building
5. **Simple Architecture**: Single cache system focused on menu performance
6. **Fast Execution**: Load desktop file details only when actually launching applications

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

## Execution Flow Architecture

### Optimized Launch Sequence
```rust
// src/launch/execute.rs - Performance-optimized execution
pub async fn execute_selected_item(item_name: &str) -> Result<()> {
    // 1. Identify item type from lightweight cache
    let item_type = identify_item_type(item_name)?;

    // 2. Load execution details lazily (only when needed)
    match item_type {
        ItemType::DesktopApp(desktop_id) => {
            let details = cache.get_desktop_details(&desktop_id).await?;
            execute_desktop_app(&details)?;
        }
        ItemType::PathExecutable(name) => {
            let details = cache.get_path_details(&name).await?;
            execute_path_executable(&details)?;
        }
    }

    // 3. Record frecency (async, non-blocking)
    task::spawn(async move {
        if let Err(e) = cache.record_item_launch(item_name).await {
            eprintln!("Warning: Failed to record launch: {}", e);
        }
    });

    Ok(())
}
```

### Cache Strategy
- **Single Cache**: Stores lightweight LaunchItem enums (just strings) for fast menu display
- **Background Refresh**: Updates cache asynchronously without blocking user experience
- **On-Demand Loading**: Parse desktop file details only when actually launching applications
- **Minimal Overhead**: No desktop file parsing during menu building

## Risk Assessment

### High Risk
- Breaking existing PATH-only functionality
- Performance regression due to desktop file parsing
- Complex name conflict resolution logic

### Mitigation
- Performance-first architecture with immediate response guarantees
- Extensive benchmarking against current implementation
- Graceful fallback to PATH-only mode on failures
- Clear separation of concerns between display and execution logic
