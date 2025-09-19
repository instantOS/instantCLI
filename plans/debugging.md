# Debugging Infrastructure Enhancement Plan

## Overview
Enhance InstantCLI's debugging capabilities with structured logging, improved menu server debugging, and comprehensive diagnostic tools.

## Current State Analysis

### Existing Debugging Infrastructure
- **Basic debug flag**: `--debug` / `-d` global CLI option implemented in `src/main.rs:154`
- **Simple output**: Extensive use of `eprintln!` with colored error messages (20+ locations)
- **Menu server**: Runs in scratchpad with debug visibility problems
- **Error handling**: `anyhow`-based with context formatting in `handle_error()` function
- **Doctor system**: Existing diagnostic framework in `src/doctor/` with check registry

### Current Debug Usage Patterns
- Error handling through `handle_error()` function with colored output
- Debug flag checked in various components for conditional output
- Menu server runs in isolation with limited debug visibility
- Doctor system provides structured health checks but no debug logging

### Limitations
- No structured logging levels (all debug output goes to stderr)
- No file output capabilities or log rotation
- Menu server debugging difficult due to scratchpad isolation
- No centralized log management or correlation IDs
- Limited debugging for async operations and performance monitoring

## Proposed Enhancements

### 1. Lightweight Logging Integration

#### Approach
- Introduce `tracing` crate as a lightweight replacement for `eprintln!` calls
- Preserve existing error handling patterns while adding structured capabilities
- Maintain backwards compatibility with current `--debug` flag behavior

#### Key Components
- **LogLevel enum**: Simple levels (Error, Warn, Info, Debug, Trace)
- **LogConfig struct**: Configuration for output destinations and formatting
- **Macro replacements**: `debug!()`, `info!()`, `error!()` macros to replace selective `eprintln!` calls

#### Integration Strategy
- Gradual migration of existing `eprintln!` calls to structured logging
- Keep error handling through `handle_error()` for user-facing messages
- Add structured logging for internal operations and debugging

### 2. Menu Server Debugging Enhancements

#### Problem Statement
Menu server runs in scratchpad terminal, making it impossible to see debug output or diagnose issues.

#### Solution Approach
- Add `--inside-debug` flag for menu server to run in visible terminal
- Implement log file output for persistent debugging
- Add parent-child process relationship for better cleanup

#### Debug Mode Concepts
- **Visible debugging**: Menu server runs in visible terminal when debug flag detected
- **Log forwarding**: Optional file output for persistent debugging
- **Process tracking**: Better cleanup and lifecycle management
- **Flag inheritance**: Debug state propagates from parent to menu server process

### 3. Enhanced Error Reporting

#### Approach
- Leverage existing `anyhow` integration with additional context
- Add structured error variants for common failure scenarios
- Include recovery suggestions and diagnostic information

#### Error Enhancement Concepts
- **Error categorization**: Group common error types with specific handling
- **Context preservation**: Maintain request/operation correlation
- **Recovery suggestions**: Common solutions included in error messages
- **Diagnostic metadata**: Environment state, configuration info

### 4. Diagnostic System Extension

#### Current State Analysis
- Doctor system exists with registry-based checks
- Basic health checks implemented for various system components
- Fix system available for some issues

#### Enhancement Concepts
- **Debug-mode diagnostics**: Additional checks when debug flag enabled
- **Menu server testing**: Specific diagnostics for IPC communication
- **Performance metrics**: Basic timing and operation counting
- **Log management**: Commands to view and manage debug logs

#### Proposed Diagnostic Extensions
```bash
# Extend existing doctor commands
instant doctor debug     # Debug-specific diagnostics
instant doctor logs       # Log management and viewing
instant doctor test menu  # Menu server communication test
```

### 5. Performance Monitoring

#### Lightweight Metrics Approach
- **Operation timing**: Track key operation durations
- **Error counting**: Monitor error rates by category
- **Resource usage**: Basic memory and CPU monitoring
- **Success rates**: Track operation success/failure ratios

#### Integration Points
- Instrument key operations in dotfile management
- Monitor menu server response times
- Track database operation performance
- Measure file operation durations

## Implementation Strategy

### Phase 1: Foundation (Week 1-2)
1. **Add tracing dependency**
   - Update Cargo.toml with tracing crates
   - Initialize logging system in main()
   - Create log configuration structure

2. **Basic logging migration**
   - Replace internal `eprintln!` calls with structured logging
   - Preserve user-facing error messages
   - Add debug level filtering

### Phase 2: Menu Server Debugging (Week 3-4)
1. **Debug mode enhancement**
   - Add visible terminal debugging option
   - Implement log file output
   - Add process cleanup handling

2. **Error reporting improvements**
   - Add structured error variants
   - Include recovery suggestions
   - Enhance error context preservation

### Phase 3: Diagnostics Enhancement (Week 5-6)
1. **Diagnostic system extension**
   - Add debug-specific health checks
   - Implement menu server testing
   - Add log management commands

2. **Performance monitoring**
   - Add basic timing instrumentation
   - Implement operation counting
   - Create performance reporting

## Technical Considerations

### Dependencies
```toml
[dependencies]
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
# Optional: tracing-appender for file output
```

### Backwards Compatibility
- Preserve existing `--debug` flag behavior
- Keep current error output format for user-facing messages
- Maintain existing doctor system functionality
- Gradual migration path for logging changes

### Performance Impact
- Minimal overhead when logging disabled
- Conditional compilation for debug features
- Async logging for non-blocking operations
- Lazy evaluation of debug information

## Integration with Existing Code

### Error Handling Integration
- Work with existing `handle_error()` function
- Enhance `anyhow` context preservation
- Add structured error types alongside existing ones
- Maintain colored output for user-facing messages

### Doctor System Integration
- Extend existing check registry
- Add debug-specific diagnostic checks
- Leverage existing fix system
- Use established reporting patterns

### Menu System Integration
- Add debug mode to menu server lifecycle
- Preserve existing IPC communication
- Enhance error reporting in menu operations
- Add visibility monitoring for debugging

## Testing Strategy

### Unit Tests
- Logging configuration and filtering
- Error type construction and formatting
- Diagnostic check logic and validation

### Integration Tests
- Menu server debugging workflows
- Log file output and rotation
- Error reporting and context preservation

### Manual Testing
- Debug mode interaction and visibility
- Log file verification and management
- Performance monitoring validation

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

## TODO Notes from Review

### Issues with Current Plan
- Too much concrete Rust code implementation details
- Doesn't leverage existing doctor system enough
- Over-engineered for current needs
- Should focus more on concepts than specific implementations

### Alignment with Existing Codebase
- Build on existing `handle_error()` function instead of replacing it
- Integrate with doctor system registry rather than creating new diagnostic framework
- Work with current menu server architecture instead of complete rewrite
- Preserve existing debug flag behavior and error output patterns

### Simplification Approach
- Focus on gradual enhancement rather than complete overhaul
- Use existing patterns and structures where possible
- Prioritize high-impact improvements over comprehensive changes
- Maintain backwards compatibility throughout