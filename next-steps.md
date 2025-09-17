# InstantCLI Menu Server Implementation Plan

## Overview

This plan outlines the implementation of a persistent menu server for InstantCLI that addresses the Wayland dmenu replacement problem described in `dmenu_replacement.md`. The solution will create a server-client architecture where the menu runs persistently and responds to client requests via Unix domain sockets.

## CLI Command Structure

### New Commands
```
instant menu server                    # Launch scratchpad terminal with server inside
instant menu server --inside         # Run server inside terminal (the actual menu server)
instant menu confirm --gui --message "Are you sure?"
instant menu choice --gui --multi --prompt "Select:" items...
instant menu input --gui --prompt "Enter value:"
```

## Implementation Phases

### Phase 1: CLI Integration (Immediate)

1. **Enhanced Menu Commands in `src/menu/mod.rs`**
   - Add `--gui` flag to existing MenuCommands enum
   - Modify handle_menu_command to route --gui requests to client
   - Implement fallback to local menu when server unavailable

2. **New Server Subcommands**
   - Add ServerCommands enum with --inside flag
   - Implement main server command (terminal launching)
   - Implement --inside mode (actual menu server logic)
   - Integrate with main CLI structure

### Phase 2: Core Infrastructure

1. **IPC Protocol Design (`src/menu/protocol.rs`)**
   - Define request/response structs for Confirm, Choice, Input
   - Implement serialization/deserialization using serde
   - Create error codes and status types
   - Keep protocol simple initially (no chunking yet)

2. **Unix Domain Socket Server (`src/menu/server.rs`)**
   - Implement async socket listener using tokio
   - Create socket in XDG_RUNTIME_DIR/instantmenu.sock
   - Handle multiple concurrent connections
   - Implement proper cleanup and signal handling

3. **Client Implementation (`src/menu/client.rs`)**
   - Socket connection logic with timeout
   - Server spawning if not running
   - Request serialization and response handling
   - Graceful degradation to local menu on server failure

### Phase 3: Menu Server Core

1. **Server Command Handler**
   - Port existing menu logic to server context
   - Handle Confirm, Choice, and Input requests
   - Integrate with existing FzfWrapper
   - Maintain same exit code behavior

2. **Terminal Management**
   - Detect if running in server mode (`--inside` flag)
   - Clear screen between menu requests
   - Handle terminal closure detection

3. **Request Processing Loop**
   - Async request handling
   - Proper error responses
   - Timeout handling for stuck requests
   - Connection state management

### Phase 4: Scratchpad Integration (Last)

1. **Compositor Detection (`src/menu/scratchpad.rs`)**
   - Auto-detect running compositor (Sway, Hyprland, etc.)
   - Implement compositor-specific commands
   - Fallback to manual terminal management

2. **Sway Integration**
   - Use high-numbered workspace (e.g., 98)
   - Implement window rules for proper placement
   - Handle workspace showing/hiding

3. **Hyprland Integration**
   - Use special workspaces
   - Implement hyprctl commands for window management
   - Handle toggling and centering

### Phase 5: Advanced Features (Post-MVP)

1. **Large Input Handling**
   - Implement chunking for thousands of items
   - Use temporary files for very large datasets
   - Stream items to reduce memory usage

2. **Enhanced Server Management**
   - Graceful shutdown with cleanup
   - Automatic server respawn on crash
   - Connection retry logic

## Data Structures

### IPC Protocol

```rust
// Request types
enum MenuRequest {
    Confirm { message: String },
    Choice { prompt: String, items: Vec<String>, multi: bool },
    Input { prompt: String },
}

// Response types
enum MenuResponse {
    ConfirmResult(ConfirmResult),
    ChoiceResult(Vec<String>),
    InputResult(String),
    Error(String),
    Cancelled,
}

// Server status
enum ServerStatus {
    Ready,
    Busy,
    ShuttingDown,
}
```

### Socket Communication

```rust
// Message envelope
struct MenuMessage {
    request_id: String,
    payload: MenuRequest,
    timestamp: SystemTime,
}

// Response envelope
struct MenuResponseMessage {
    request_id: String,
    payload: MenuResponse,
    timestamp: SystemTime,
}
```

## Implementation Steps

### Step 1: CLI Commands and Structure (Week 1)
1. **Modify `src/menu/mod.rs`**
   - Add `--gui` flag to existing MenuCommands enum
   - Create ServerCommands enum with --inside flag
   - Implement handle_menu_command routing for --gui requests
   - Implement main server command (terminal launching: `kitty -e dash -c 'instant server --inside'`)
   - Implement --inside mode (actual menu server logic)

2. **Update `src/main.rs`**
   - Integrate new server commands into CLI structure
   - Test compilation and basic command parsing

### Step 2: Protocol and Socket Infrastructure (Week 1-2)
1. **Create `src/menu/protocol.rs`**
   - Define MenuRequest/MenuResponse enums
   - Implement serde serialization
   - Create error handling types

2. **Implement `src/menu/client.rs`**
   - Socket connection with timeout
   - Server spawning logic
   - Request/response handling
   - Fallback to local menu

3. **Add dependencies to Cargo.toml**
   - tokio with full features
   - serde and serde_json
   - xdg for runtime directory

### Step 3: Core Server Logic (Week 2-3)
1. **Create `src/menu/server.rs`**
   - Unix domain socket listener
   - Request processing loop
   - Command handler for menu types
   - Integration with existing FzfWrapper

2. **Terminal Management**
   - `--inside` flag detection
   - Screen clearing between requests
   - Basic terminal state management

### Step 4: Testing and Refinement (Week 3-4)
1. **Compile and Verify**
   - Ensure all code compiles without errors
   - Test basic command parsing
   - Verify dependency integration

2. **Manual Testing by User**
   - Test server startup with `instant menu server --inside` in manual terminal
   - Test terminal launching with `instant menu server` (should launch terminal with --inside mode)
   - Test --gui flag functionality
   - Verify server-client communication
   - Test fallback behavior

### Step 5: Advanced Features (Future)
1. **Large Input Handling**
   - Chunking implementation
   - Temporary file support
   - Memory optimization

2. **Enhanced Server Management**
   - Graceful shutdown
   - Error recovery

## Dependencies to Add

```toml
[dependencies]
# For async runtime and networking
tokio = { version = "1.0", features = ["full"] }

# For IPC protocol serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# For XDG directory support
xdg = "2.4"

# Future dependencies (add when needed)
# signal-hook = "0.3"  # For process management
# tempfile = "3.0"      # For large input handling
```

## Development Approach

**Code Implementation:**
- Focus on compilation verification only
- No automated testing of interactive components
- Manual testing will be performed by user
- Prioritize working code over comprehensive test coverage

**Testing Strategy:**
1. **Compilation Verification**: Ensure all code compiles without errors
2. **Command Parsing**: Test CLI command structure and argument parsing
3. **Manual User Testing**: User will test interactive functionality
4. **Integration Testing**: User will verify server-client communication

**Key Constraints:**
- No automated testing of fzf or menu interactions
- Scratchpad integration will be done last with manual terminal testing
- Large input handling (chunking) deferred until core functionality works
- Focus on getting MVP working with manual terminal management

## Success Criteria

1. **Core Functionality**: Menu commands work with --gui flag
2. **Server Communication**: Client can successfully send requests and receive responses
3. **Fallback Behavior**: Graceful degradation to local menu when server unavailable
4. **CLI Integration**: Both server commands work correctly (terminal launching and --inside mode)
5. **Compilation**: All code compiles without errors
6. **Manual Testing**: User can test both modes:
   - `instant menu server --inside` in manual terminal
   - `instant menu server` launching terminal with --inside mode

## Next Steps

1. Begin with Step 1: CLI command integration (both modes are part of MVP)
2. Progress through implementation phases in order
3. User will provide feedback after each phase
4. Both server modes are essential for the final product
5. Advanced features like chunking will be implemented last
