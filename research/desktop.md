# XDG Desktop File Research for InstantCLI

## Overview
Research document for implementing XDG desktop file support in the InstantCLI launcher, with the goal of supporting both .desktop files and PATH executables.

## Current Implementation Analysis

The current launcher implementation (`src/launch/`) supports only PATH executables:
- Scans PATH directories for executable files
- Uses frecency-based sorting with the `fre` crate
- Implements background cache refreshing
- Has simple execution via `Command::new(app_name)`

## Key Requirements from User

1. **Support both .desktop files and PATH executables**
2. **Use XDG desktop file specification**
3. **Prefix PATH executables with "path:" when naming conflicts occur**
4. **Maintain frecency support**
5. **Reference fuzzel as implementation example**

## XDG Desktop File Specification Research

### Key Standards
- **XDG Desktop Entry Specification**: https://specifications.freedesktop.org/desktop-entry-spec/latest/
- **XDG Base Directory Specification**: https://specifications.freedesktop.org/basedir-spec/latest/

### Desktop File Structure
```ini
[Desktop Entry]
Type=Application
Name=Firefox
Exec=firefox %u
Icon=firefox
Categories=Network;WebBrowser;
```

### Important Fields
- **Type**: Application, Link, Directory
- **Name**: Display name (localized)
- **Exec**: Command to execute with field codes
- **Icon**: Icon name
- **Categories**: Semicolon-separated categories
- **NoDisplay**: Boolean for visibility
- **Terminal**: Whether to run in terminal

### Exec Field Codes
- `%f`: Single file name
- `%F`: Multiple file names
- `%u`: Single URL
- `%U`: Multiple URLs
- `%i`: Icon name
- `%c`: Application name
- `%k`: Desktop file path
- `%%`: Literal percent

## XDG Directory Structure

### Standard Search Paths
Desktop files should be searched in these directories (in order of priority):

1. `$XDG_DATA_HOME/applications/` (typically `~/.local/share/applications/`)
2. `$XDG_DATA_DIRS/applications/` (typically `/usr/share/applications/`, `/usr/local/share/applications/`)

### Environment Variables
- `XDG_DATA_HOME`: User-specific data directory
- `XDG_DATA_DIRS`: Colon-separated list of data directories
- `XDG_CURRENT_DESKTOP`: Current desktop environment (for filtering)

## Rust Libraries for Desktop File Handling

### 1. freedesktop-file-parser (Recommended)
- **Repository**: https://github.com/BL-CZY/desktop_file_parser
- **Version**: 0.3.1
- **Features**:
  - Full support for Desktop Entry Specification v1.5
  - Locale-aware string handling
  - Icon resolution using freedesktop icon theme
  - Support for desktop actions
  - Strong type safety with Rust's type system

### Basic Usage Example
```rust
use freedesktop_file_parser::{parse, EntryType};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let content = r#"[Desktop Entry]
Type=Application
Name=Firefox
Exec=firefox %u
Icon=firefox
Categories=Network;WebBrowser;
    "#;

    let desktop_file = parse(content)?;

    // Access basic properties
    println!("Name: {}", desktop_file.entry.name.default);

    // Check entry type
    if let EntryType::Application(app) = &desktop_file.entry.entry_type {
        println!("Exec: {}", app.exec.as_ref().unwrap());
    }

    Ok(())
}
```

### 2. xgkit
- **Repository**: https://github.com/1sra3l/xdgkit
- **Features**: Comprehensive XDG toolkit with desktop entry support
- **Status**: More comprehensive but potentially heavier dependency

### 3. xdg-desktop-entries
- **Repository**: https://github.com/OmegaMetor/xdg-desktop-entries
- **Version**: 0.1.1
- **Status**: Simple parser, less feature-complete

## Fuzzel Implementation Insights

### Key Features from Fuzzel
1. **Desktop File Discovery**: Scans XDG directories for .desktop files
2. **Field Matching**: Can match against multiple fields (filename, name, generic, exec, etc.)
3. **Name Prefixing**: Uses `--list-executables-in-path` to include PATH executables
4. **Icon Support**: Renders icons from desktop files
5. **Actions Support**: Supports desktop actions

### Relevant Configuration Options
- `fields`: Comma-separated list of fields to match against
- `list-executables-in-path`: Include PATH executables
- `icon-theme`: Icon theme to use
- `filter-desktop`: Filter based on XDG_CURRENT_DESKTOP

## Implementation Plan Outline

### Phase 1: Core Desktop File Support
1. **Add dependencies**: `freedesktop-file-parser` for desktop file parsing
2. **New data structures**:
   - `LaunchItem` enum to handle both desktop files and PATH executables
   - `DesktopApp` struct for parsed desktop entries
3. **Discovery system**: Scan XDG directories for .desktop files
4. **Merging logic**: Combine desktop apps and PATH executables with proper naming

### Phase 2: Enhanced Features
1. **Icon support**: Basic icon handling
2. **Desktop actions**: Support for additional actions
3. **Field code expansion**: Proper handling of Exec field codes
4. **Desktop filtering**: Based on XDG_CURRENT_DESKTOP

### Phase 3: Integration and Polish
1. **Cache system**: Extend existing cache for desktop files
2. **Frecency integration**: Apply existing frecency system to launch items
3. **Performance optimizations**: Background scanning, cache invalidation
4. **User configuration**: Options for desktop file handling

## Naming Conflict Resolution

The key requirement is to prefix PATH executables with "path:" when there's a name conflict with a desktop file.

### Example Resolution:
- Desktop file: `firefox.desktop` → Display as "Firefox"
- PATH executable: `firefox` → Display as "path:firefox"
- No conflict: `htop` → Display as "htop" (PATH only)

## Code Architecture Considerations

### Data Structure Design
```rust
pub enum LaunchItem {
    DesktopApp(DesktopApp),
    PathExecutable(String),
}

pub struct DesktopApp {
    pub desktop_id: String,
    pub name: String,
    pub exec: String,
    pub icon: Option<String>,
    pub categories: Vec<String>,
    pub no_display: bool,
    pub terminal: bool,
    pub file_path: PathBuf,
}
```

### Cache Extension
The existing cache system should be extended to handle both types of launch items while maintaining the frecency tracking functionality.

## Next Steps

1. **Evaluate `freedesktop-file-parser`** for actual use
2. **Design the merged discovery system** for desktop files and PATH
3. **Implement core desktop file parsing** and discovery
4. **Create naming conflict resolution** logic
5. **Extend cache system** for the new data structure
6. **Integrate with existing launcher** UI and execution

## References and Resources

### Specifications
- [Desktop Entry Specification](https://specifications.freedesktop.org/desktop-entry-spec/latest/)
- [Base Directory Specification](https://specifications.freedesktop.org/basedir-spec/latest/)

### Rust Libraries
- [freedesktop-file-parser](https://lib.rs/crates/freedesktop-file-parser)
- [xdgkit](https://docs.rs/xdgkit/latest/xdgkit/)

### Reference Implementations
- [Fuzzel Launcher](https://codeberg.org/dnkl/fuzzel)
- [Fuzzel Manual Pages](https://man.archlinux.org/man/fuzzel.1.en)

### Additional Resources
- [PyXDG DesktopEntry Implementation](https://pyxdg.readthedocs.io/en/latest/_modules/xdg/DesktopEntry.html)
- [XDG Desktop Portal](https://flatpak.github.io/xdg-desktop-portal/)