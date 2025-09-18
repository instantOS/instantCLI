# Application Launcher Plan

## Overview

Build a native Rust application launcher for InstantCLI using the existing menu and scratchpad infrastructure. The launcher will discover available applications from PATH and present them in an interactive GUI menu for selection and execution.

## Command Structure

Add a new top-level command: `instant launch`

This will be a simple command with no subcommands that:
- Discovers applications from PATH
- Shows them in a GUI menu (like `menu --gui` commands)
- Executes the selected application

## Application Discovery

### Discovery Sources

1. **PATH Executables**: Scan all directories in `$PATH` for executable files

### Application Structure

Keep it simple like dmenu - just executable names:
- Store only the executable name (not full path)
- Use `which` or PATH lookup at execution time to find full path
- No metadata, descriptions, or complex structures needed

### Discovery Implementation

Fast startup with background refresh:
- Cache file: `~/.cache/instant/launch_cache`
- Check if any PATH directory is newer than cache file
- If cache is fresh: read from cache file
- If cache is stale:
  - Immediately read and use stale cache for fast startup
  - Spawn background task to scan PATH directories and update cache
  - Next invocation will use the refreshed cache
- No filtering - include all executables like dmenu does
- Sort alphabetically and deduplicate by name

## Menu Integration

### Menu Item Creation

Simple SerializableMenuItem creation:
- display_text: just the executable name
- preview: none (keep it simple like dmenu)
- metadata: none needed

### Menu Flow

1. **Discovery**: Check cache freshness, use stale cache immediately if needed
2. **Background Refresh**: If cache was stale, spawn background task to update it
3. **Menu Display**: Present executable names in GUI choice menu (no waiting)
4. **Selection**: User selects executable name from menu
5. **Execution**: Use selected name to launch application

## Execution Methods

### Application Execution

Simple execution like dmenu:
- Use Command::new() with just the executable name (let PATH resolve it)
- Spawn process in background with detachment
- Redirect stdout/stderr to /dev/null
- No need to wait for process completion
- Handle execution errors gracefully

## File Structure

```
src/
├── launch/
│   ├── mod.rs        # Main module and command handling
│   └── cache.rs      # dmenu-style caching and PATH scanning
└── main.rs           # Add Launch command to main CLI
```

## Implementation Steps

1. **Create Module Structure**: Set up the launch module (mod.rs + cache.rs)
2. **Cache Implementation**: Implement instant cache reading with background refresh
3. **PATH Scanning**: Async PATH scanning that doesn't block startup
4. **Menu Integration**: Connect with existing GUI menu system
5. **Execution Logic**: Simple background execution by name
6. **CLI Integration**: Add launch command to main CLI parser
7. **Testing**: Test with various PATH executables

## Error Handling

- **Discovery Errors**: Log warnings for inaccessible PATH directories, continue with partial results
- **Execution Errors**: Provide clear error messages for failed launches
- **Menu Cancellation**: Handle user cancellation gracefully (exit code 1)
- **Permission Issues**: Detect and report permission problems

## Performance Considerations

- **Instant Startup**: Always use existing cache immediately, never block on scanning
- **Background Refresh**: Update stale cache in background for next invocation
- **Smart Caching**: dmenu-style cache based on PATH directory modification times
- **No Filtering**: Include all executables like dmenu (let user decide what to run)

## Future Optional Features

### Desktop Application Support
- Parse `.desktop` files from standard locations:
  - `/usr/share/applications/`
  - `/usr/local/share/applications/`
  - `~/.local/share/applications/`
- Extract application names, descriptions, icons, and categories
- Handle terminal vs GUI application detection
- Support for application categories and filtering

### Enhanced Features
- **Favorites**: Track frequently used applications
- **Search**: Add fuzzy search within the menu
- **Categories**: Group applications by category
- **Custom Commands**: Allow user-defined custom commands
- **History**: Track launch history for better sorting
- **Configuration**: Allow customization of discovery sources and behavior
- **Scratchpad Integration**: Option to launch applications in scratchpad terminals
