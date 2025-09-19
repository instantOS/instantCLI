# Menu Server Queue Management Plan

## Overview
Resolve menu server blocking issues when scratchpad is hidden by implementing simple visibility checks and request timeout mechanisms.

## Current State Analysis

### Problem Description
The menu server becomes unresponsive when:
1. Menu scratchpad is hidden while a menu is active (fzf running in hidden terminal)
2. Server waits indefinitely for user input that can never be provided
3. No mechanism to detect hidden scratchpad state during menu operations
4. Client requests timeout or block indefinitely

### Existing Infrastructure - Already Available!
- **Visibility detection**: `src/scratchpad/visibility.rs:12-37` `is_scratchpad_visible()` function
- **Compositor integration**: Hyprland and Sway support in `src/common/compositor/`
- **Menu server architecture**: Request handling in `src/menu/server.rs:201-230`
- **Scratchpad management**: Show/hide functions already implemented

### Current Menu Server Flow

```rust
// src/menu/server.rs:201-230
fn process_request(&self, request: MenuRequest) -> Result<MenuResponse> {
    // Show scratchpad if configured (for interactive requests only)
    let should_manage_scratchpad = matches!(
        request,
        MenuRequest::Confirm { .. } | MenuRequest::Choice { .. } | MenuRequest::Input { .. }
    );

    if should_manage_scratchpad {
        if let Err(e) = self.show_scratchpad() {
            eprintln!("Warning: Failed to show scratchpad: {e}");
        }
    }

    // Process the request
    let response = match request {
        MenuRequest::Confirm { message } => match FzfWrapper::confirm(&message) {
            // ... fzf processing - BLOCKS HERE
        },
        MenuRequest::Choice { prompt, items, multi, .. } => {
            // ... fzf processing - BLOCKS HERE
        },
        // ... other request types
    };
}
```

### Key Issues Identified
1. **No visibility monitoring**: Server doesn't check if scratchpad remains visible during fzf operations
2. **Blocking fzf calls**: Long-running fzf operations block entire server
3. **No timeout mechanism**: Requests can hang indefinitely if scratchpad hidden
4. **Single-threaded design**: Only one request processed at a time, blocking all others

## Root Cause Analysis

### Visibility Detection Already Exists
The `is_scratchpad_visible()` function in `src/scratchpad/visibility.rs` already provides:
- **Hyprland support**: Checks special workspace activity AND window existence
- **Sway support**: Window visibility detection
- **Fallback handling**: Graceful degradation for other compositors

### Core Problem
When user hides scratchpad while fzf is running:
1. fzf process continues in hidden terminal
2. Menu server waits indefinitely for fzf to complete
3. No way to detect that user can't interact with fzf
4. All other requests are blocked

## Proposed Solution

### Simple Visibility Monitoring Approach
Instead of complex queue systems, implement simple visibility checks during menu operations.

#### Solution Concept
1. **Pre-operation visibility check**: Verify scratchpad is visible before starting fzf
2. **Periodic monitoring**: Check visibility during long-running operations
3. **Timeout mechanism**: Cancel operations if scratchpad hidden for too long
4. **Process cleanup**: Terminate fzf processes when scratchpad hidden

#### Leverage Existing Infrastructure
- Use `is_scratchpad_visible()` function instead of creating new monitoring system
- Work with existing menu server architecture instead of complete rewrite
- Maintain current request/response patterns

### Enhanced Menu Server Logic

#### Basic Timeout Enhancement
```rust
// Simple enhancement to existing process_request function
fn process_request_with_timeout(&self, request: MenuRequest) -> Result<MenuResponse> {
    // Process with timeout monitoring
    let timeout_duration = Duration::from_secs(30);
    let start_time = Instant::now();

    // Spawn monitoring task
    let monitor_handle = tokio::spawn({
        let scratchpad_config = self.scratchpad_config.clone();
        let compositor = self.compositor.clone();
        async move {
            loop {
                if let Some(ref config) = scratchpad_config {
                    if let Ok(false) = is_scratchpad_visible(&compositor, config) {
                        break; // Scratchpad became hidden
                    }
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    });

    // Process request with timeout
    tokio::select! {
        result = self.process_request(request) => result,
        _ = monitor_handle => {
            Ok(MenuResponse::Error("Scratchpad hidden during operation".to_string()))
        },
        _ = tokio::time::sleep(timeout_duration) => {
            Ok(MenuResponse::Error("Operation timed out".to_string()))
        }
    }
}
```

### Client-side Enhancements

Do not change the client, retry code is not needed. 

## Testing Strategy

No new tests needed, existing tests are sufficient.
