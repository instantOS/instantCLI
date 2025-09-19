# Debugging Infrastructure Enhancement Plan

## Overview
Enhance InstantCLI's debugging capabilities with structured logging, improved menu server debugging, and comprehensive diagnostic tools.

## Current State Analysis

### Existing Debugging Infrastructure
- **Basic debug flag**: `--debug` / `-d` global CLI option
- **Simple output**: `eprintln!` with colored error messages
- **Menu server**: Runs in scratchpad with no debug visibility
- **Error handling**: `anyhow`-based with context formatting

### Limitations
- No structured logging levels
- No file output capabilities
- Menu server runs in isolation, hard to debug
- No centralized log management
- Limited debugging for async operations

## Proposed Enhancements

### 1. Structured Logging System

#### Core Components
```rust
// New logging infrastructure
pub mod logging {
    use tracing::{info, warn, error, debug};
    use tracing_subscriber::{fmt, util::SubscriberInitExt};

    pub enum LogLevel {
        Error,
        Warn,
        Info,
        Debug,
        Trace
    }

    pub struct LogConfig {
        level: LogLevel,
        file_output: Option<PathBuf>,
        json_format: bool,
        enable_colors: bool,
    }
}
```

#### Features
- **Log levels**: ERROR, WARN, INFO, DEBUG, TRACE
- **Multiple outputs**: stderr + optional file output
- **Format options**: Human-readable or JSON
- **Context preservation**: Request/operation correlation IDs
- **Performance**: Async logging with minimal overhead

#### Integration Points
- Replace all `eprintln!` calls with structured logging
- Add context to all error paths
- Instrument key operations (file ops, network requests)
- Add timing metrics for performance analysis

### 2. Menu Server Debugging

#### Problem Statement
Menu server runs in scratchpad terminal, making it impossible to see debug output or diagnose issues.

#### Solution Architecture
```rust
// Debug mode for menu server
pub struct MenuServerConfig {
    pub debug_mode: bool,
    pub log_file: Option<PathBuf>,
    pub parent_process_id: u32,
}

impl MenuServer {
    pub fn spawn_with_debug(config: MenuServerConfig) -> Result<Self> {
        if config.debug_mode {
            // Launch in visible terminal with log capture
            let cmd = Self::build_debug_command(&config);
            // ... spawn with visible output
        } else {
            // Current scratchpad behavior
        }
    }

    fn build_debug_command(config: &MenuServerConfig) -> Command {
        let mut cmd = Command::new("kitty");
        cmd.arg("--title")
           .arg("instant-menu-debug")
           .arg("--class")
           .arg("instant-menu-debug")
           .arg("-e")
           .arg("bash");

        if let Some(log_file) = &config.log_file {
            cmd.arg(format!(
                "-c 'instant menu --inside --debug 2>&1 | tee {}'",
                log_file.display()
            ));
        } else {
            cmd.arg("-c 'instant menu --inside --debug'");
        }
        cmd
    }
}
```

#### Key Features
- **Debug flag inheritance**: `--debug` flag propagates to menu server
- **Log capture**: Optional file output for persistent debugging
- **Visual debugging**: Server runs in visible terminal when debugging
- **Process tracking**: Parent-child relationship for cleanup

### 3. Enhanced Error Reporting

#### Structured Error Types
```rust
#[derive(Debug, thiserror::Error)]
pub enum InstantError {
    #[error("IO error: {context}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error
    },

    #[error("Menu server communication failed: {reason}")]
    MenuServer {
        reason: String,
        suggestion: String,
        server_pid: Option<u32>,
    },

    #[error("Database error: {operation}")]
    Database {
        operation: String,
        #[source]
        source: rusqlite::Error
    },

    #[error("Configuration error: {path}")]
    Config {
        path: PathBuf,
        detail: String
    }
}
```

#### Error Context Enhancement
- **Suggestions**: Common solutions included in error messages
- **Recovery actions**: Next steps for user
- **Correlation IDs**: Trace errors across components
- **Metadata**: Environment, version, configuration state

### 4. Diagnostic Commands

#### Health Check System
```rust
// New doctor subcommands
instant doctor diagnostics    # Run comprehensive diagnostics
instant doctor logs --follow  # View/tail logs
instant doctor status       # Check system health
instant doctor test menu     # Test menu server functionality
```

#### Diagnostic Checks
- **Menu server connectivity**: Test IPC communication
- **Database integrity**: Verify SQLite database
- **Configuration validation**: Check TOML config
- **Permission verification**: File/system access
- **Compositor detection**: Verify Hyprland/Sway support

### 5. Performance Monitoring

#### Metrics Collection
```rust
pub struct Metrics {
    pub menu_response_times: Histogram,
    pub file_operation_duration: Timer,
    pub database_query_times: Histogram,
    pub memory_usage: Gauge,
    pub active_connections: Counter,
}
```

#### Integration
- **Prometheus export**: Optional metrics endpoint
- **CLI reporting**: `instant --metrics` command
- **Performance alerts**: Warning thresholds

## Implementation Plan

### Phase 1: Foundation (Week 1-2)
1. **Add tracing dependency**
   - Update Cargo.toml with tracing crates
   - Initialize logging system in main()
   - Replace key debug println! calls

2. **Basic menu server debugging**
   - Add `--inside-debug` flag
   - Implement log forwarding to parent
   - Add process cleanup handling

### Phase 2: Enhanced Logging (Week 3-4)
1. **Structured logging implementation**
   - Create logging configuration
   - Add file output support
   - Implement JSON formatting option

2. **Error type improvements**
   - Define structured error types
   - Add error context macros
   - Implement error recovery suggestions

### Phase 3: Diagnostics (Week 5-6)
1. **Diagnostic commands**
   - Implement health check system
   - Create diagnostic test suite
   - Add log viewing capabilities

2. **Performance monitoring**
   - Add basic metrics collection
   - Implement timing instrumentation
   - Create performance reporting

## Technical Considerations

### Dependencies
```toml
[dependencies]
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
tracing-appender = "0.2"
thiserror = "1.0"
tokio = { version = "1.0", features = ["full"] }
```

### Backwards Compatibility
- Existing `--debug` flag behavior preserved
- Default logging remains simple (stderr only)
- Optional features enabled via configuration

### Performance Impact
- Async logging minimizes blocking
- Conditional compilation for debug features
- Minimal overhead when logging disabled

## Testing Strategy

### Unit Tests
- Logging configuration parsing
- Error type construction
- Diagnostic check logic

### Integration Tests
- Menu server debugging workflow
- Log file rotation and cleanup
- End-to-end error reporting

### Manual Testing
- Debug menu server interaction
- Log file verification
- Performance validation

## Success Metrics

- **Developer experience**: Debugging issues 50% faster
- **Bug reporting**: Users can provide comprehensive logs
- **System reliability**: Proactive issue detection
- **Performance**: <1ms overhead for logging operations

## Future Enhancements

- **Remote debugging**: WebSocket-based log streaming
- **Log aggregation**: Centralized log management
- **Advanced profiling**: Integration with profiling tools
- **AI-assisted diagnostics**: Pattern recognition in logs