# Menu Server Queue Management Plan

## Overview
Resolve menu server blocking issues by implementing a visibility monitoring system, proper queue management, and enhanced scratchpad integration.

## Current State Analysis

### Problem Description
The menu server becomes unresponsive when:
1. Menu scratchpad is hidden while a menu is active
2. Server waits indefinitely for user input that can never be provided
3. No mechanism to detect hidden scratchpad state
4. Client requests timeout or block indefinitely

### Current Architecture
```rust
// Current menu server flow
impl MenuServer {
    pub async fn handle_request(&mut self, request: MenuRequest) -> MenuResponse {
        match request {
            MenuRequest::ShowChoice(options) => {
                // Show fzf menu in scratchpad
                let result = self.run_fzf_menu(options).await?;
                // Server blocks here until fzf completes
                result
            }
            // Other request types...
        }
    }
}
```

### Key Issues Identified
1. **No visibility detection**: Server cannot determine if scratchpad is hidden
2. **Blocking operations**: Long-running fzf operations block all other requests
3. **No timeout mechanism**: Requests can hang indefinitely
4. **Missing cleanup**: Hidden menus remain active in server state
5. **Single-threaded design**: Only one request processed at a time

## Root Cause Analysis

### Hyprland Special Workspace Behavior
Based on research of Hyprland documentation:
- **Special workspaces**: Can be toggled on/off with `togglespecialworkspace`
- **Window visibility**: Windows in special workspaces exist but are not visible when hidden
- **IPC commands**: `hyprctl` commands are blocking as mentioned in the original issue
- **Window detection**: Can check if window class exists and workspace state

### Current Menu Server Limitations
- **No polling mechanism**: Cannot check scratchpad visibility
- **Synchronous design**: Blocks on each menu operation
- **State management**: No tracking of scratchpad window state
- **Error recovery**: No mechanism to recover from hidden state

## Proposed Solution Architecture

# TODO

This plan does too much. 
visibility.rs should already contain most of the things needed to make this
happen. On sway there isn't a hotkey to hide the scratchpad, but it's possible
for the user to hide the window as well and have the same problem. Using the
universal is_scratchpad_visible function should be enough.

### 1. Visibility Monitoring System
```rust
pub struct VisibilityMonitor {
    pub scratchpad_class: String,
    pub check_interval: Duration,
    pub hyprland_available: bool,
}

impl VisibilityMonitor {
    pub fn new(scratchpad_class: String) -> Self {
        Self {
            scratchpad_class,
            check_interval: Duration::from_millis(500),
            hyprland_available: Self::check_hyprland(),
        }
    }

    fn check_hyprland() -> bool {
        // Check if Hyprland IPC is available
        std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok()
    }

    pub async fn is_scratchpad_visible(&self) -> Result<bool> {
        if !self.hyprland_available {
            // Fallback: Assume visible if not on Hyprland
            return Ok(true);
        }

        // Use existing hyprland integration
        match hyprland::dispatch::GetWindows {
            filter: Some(format!("class:{}", self.scratchpad_class)),
        }.await {
            Ok(windows) => {
                Ok(windows.iter().any(|w| {
                    w.workspace.map_or(false, |ws| !ws.special.is_empty())
                }))
            }
            Err(e) => {
                warn!("Failed to check scratchpad visibility: {}", e);
                Ok(false) // Assume hidden if check fails
            }
        }
    }

    pub async fn wait_until_hidden(&self, timeout: Duration) -> bool {
        let start = Instant::now();

        while start.elapsed() < timeout {
            if let Ok(false) = self.is_scratchpad_visible().await {
                return true;
            }
            tokio::time::sleep(self.check_interval).await;
        }

        false
    }
}
```

### 2. Enhanced Menu Server
```rust
pub struct EnhancedMenuServer {
    pub config: MenuServerConfig,
    pub visibility_monitor: VisibilityMonitor,
    pub active_menu: Option<ActiveMenu>,
    pub request_queue: VecDeque<MenuRequest>,
    pub is_running: bool,
}

pub struct ActiveMenu {
    pub request_id: String,
    pub started_at: Instant,
    pub timeout: Duration,
    pub scratchpad_class: String,
}

impl EnhancedMenuServer {
    pub async fn handle_request(&mut self, request: MenuRequest) -> MenuResponse {
        // Check if scratchpad is visible before proceeding
        if let Ok(false) = self.visibility_monitor.is_scratchpad_visible().await {
            if let Some(active_menu) = &self.active_menu {
                // Cancel the hidden menu
                self.cancel_hidden_menu(active_menu.request_id.clone()).await;
            }
        }

        // Check if there's already an active menu
        if self.active_menu.is_some() {
            // Queue the request
            self.request_queue.push_back(request);
            return MenuResponse::Queued;
        }

        // Create new active menu
        let menu_id = uuid::Uuid::new_v4().to_string();
        let active_menu = ActiveMenu {
            request_id: menu_id.clone(),
            started_at: Instant::now(),
            timeout: Duration::from_secs(30), // 30 second timeout
            scratchpad_class: self.get_scratchpad_class(&request),
        };

        self.active_menu = Some(active_menu);

        // Spawn background task for menu with visibility monitoring
        let result = self.run_menu_with_monitoring(request, menu_id.clone()).await;

        // Clean up active menu state
        self.active_menu = None;

        // Process queued requests
        self.process_queue().await;

        result
    }

    async fn run_menu_with_monitoring(&mut self, request: MenuRequest, menu_id: String) -> MenuResponse {
        let visibility_task = tokio::spawn({
            let monitor = self.visibility_monitor.clone();
            let id = menu_id.clone();
            async move {
                // Monitor visibility in background
                monitor.wait_until_hidden(Duration::from_secs(60)).await;
                id
            }
        });

        let menu_task = tokio::spawn({
            let request = request.clone();
            async move {
                // Run the actual menu operation
                self.run_menu_operation(request).await
            }
        });

        tokio::select! {
            visibility_result = visibility_task => {
                match visibility_result {
                    Ok(hidden_menu_id) => {
                        info!("Scratchpad became hidden, cancelling menu: {}", hidden_menu_id);
                        MenuResponse::Cancelled {
                            reason: "Scratchpad hidden".to_string(),
                        }
                    }
                    Err(e) => {
                        error!("Visibility monitoring failed: {}", e);
                        // Continue with menu operation
                        menu_task.await.unwrap_or_else(|e| {
                            error!("Menu operation failed: {}", e);
                            MenuResponse::Error {
                                message: format!("Menu operation failed: {}", e),
                            }
                        })
                    }
                }
            }
            menu_result = menu_task => {
                menu_result.unwrap_or_else(|e| {
                    error!("Menu operation failed: {}", e);
                    MenuResponse::Error {
                        message: format!("Menu operation failed: {}", e),
                    }
                })
            }
        }
    }

    async fn cancel_hidden_menu(&mut self, menu_id: String) {
        info!("Cancelling hidden menu: {}", menu_id);

        // Try to kill the fzf process
        if let Some(active_menu) = &self.active_menu {
            if active_menu.request_id == menu_id {
                // Send SIGTERM to fzf process
                if let Ok(pid) = self.get_fzf_process_pid(&menu_id).await {
                    if let Err(e) = tokio::process::Command::new("kill")
                        .arg(pid.to_string())
                        .output()
                        .await
                    {
                        warn!("Failed to kill fzf process: {}", e);
                    }
                }

                self.active_menu = None;
            }
        }
    }

    async fn process_queue(&mut self) {
        while let Some(request) = self.request_queue.pop_front() {
            // Process next queued request
            let response = self.handle_request(request).await;
            // Handle response (send back to client, etc.)
        }
    }
}
```

### 3. Queue Management System
```rust
#[derive(Debug, Clone)]
pub struct MenuRequest {
    pub id: String,
    pub request_type: MenuRequestType,
    pub priority: RequestPriority,
    pub timeout: Duration,
    pub client_info: ClientInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RequestPriority {
    High,    // System-critical (e.g., confirmations)
    Normal,  // Regular user requests
    Low,     // Background operations
}

pub struct RequestQueue {
    queue: PriorityQueue<MenuRequest, RequestPriority>,
    max_size: usize,
    timeout: Duration,
}

impl RequestQueue {
    pub fn new(max_size: usize) -> Self {
        Self {
            queue: PriorityQueue::new(),
            max_size,
            timeout: Duration::from_secs(60),
        }
    }

    pub fn push(&mut self, request: MenuRequest) -> Result<(), QueueError> {
        if self.queue.len() >= self.max_size {
            return Err(QueueError::Full);
        }

        self.queue.push(request, request.priority.clone());
        Ok(())
    }

    pub fn pop(&mut self) -> Option<MenuRequest> {
        self.queue.pop()
    }

    pub fn cancel_timed_out(&mut self) -> Vec<MenuRequest> {
        let now = Instant::now();
        let mut timed_out = Vec::new();

        // Remove timed out requests
        self.queue.retain(|request| {
            if request.started_at.elapsed() > request.timeout {
                timed_out.push(request.clone());
                false
            } else {
                true
            }
        });

        timed_out
    }
}
```

### 4. Client Communication Enhancement
```rust
pub struct MenuClient {
    pub server_connection: UnixStream,
    pub request_timeout: Duration,
    pub retry_policy: RetryPolicy,
}

impl MenuClient {
    pub async fn send_request(&mut self, request: MenuRequest) -> Result<MenuResponse> {
        let start = Instant::now();

        loop {
            // Check if we've exceeded total timeout
            if start.elapsed() > self.request_timeout {
                return Err(MenuError::Timeout);
            }

            // Send request to server
            match self.send_request_to_server(&request).await {
                Ok(response) => {
                    match response {
                        MenuResponse::Queued => {
                            // Request was queued, wait and retry
                            tokio::time::sleep(Duration::from_millis(100)).await;
                            continue;
                        }
                        other => return Ok(other),
                    }
                }
                Err(e) => {
                    match self.retry_policy.should_retry(&e) {
                        true => {
                            tokio::time::sleep(self.retry_policy.delay).await;
                            continue;
                        }
                        false => return Err(e),
                    }
                }
            }
        }
    }
}
```

## Implementation Plan

### Phase 1: Visibility Monitoring (Week 1-2)
1. **Implement VisibilityMonitor**
   - Add Hyprland IPC integration
   - Create window visibility detection
   - Add fallback for non-Hyprland environments

2. **Enhanced error handling**
   - Add timeout mechanisms
   - Implement graceful degradation
   - Add logging and monitoring

### Phase 2: Queue Management (Week 3-4)
1. **Request queue implementation**
   - Create priority-based queue system
   - Add timeout handling
   - Implement request cancellation

2. **Enhanced menu server**
   - Integrate visibility monitoring
   - Add async request processing
   - Implement background monitoring tasks

### Phase 3: Client Enhancement (Week 5-6)
1. **Client retry mechanism**
   - Implement smart retry policies
   - Add request timeout handling
   - Create status reporting

2. **Testing and refinement**
   - End-to-end testing of queue system
   - Performance optimization
   - User experience improvements

## Technical Considerations

### Dependencies
```toml
[dependencies]
tokio = { version = "1.0", features = ["full"] }
uuid = { version = "1.0", features = ["v4"] }
priority-queue = "1.0"
hyprland = "0.4.0-beta.2"
async-trait = "0.1"
```

### Performance Optimization
- **Minimal polling**: 500ms check interval to avoid excessive IPC calls
- **Async design**: Non-blocking operations throughout
- **Resource cleanup**: Proper cleanup of background tasks
- **Memory management**: Limit queue size and timeout

### Error Handling
- **Graceful degradation**: Function without Hyprland integration
- **Timeout recovery**: Automatic cancellation of stuck operations
- **State recovery**: Clear server state on errors
- **User feedback**: Clear error messages and status updates


## Future Enhancements

- **Multi-monitor support**: Handle scratchpads on different monitors
- **Advanced queuing**: Priority inheritance and request merging
- **Remote monitoring**: WebSocket-based status reporting
- **AI optimization**: Adaptive timeout and retry policies
