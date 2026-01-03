use super::processing::RequestProcessor;
use super::protocol::*;
use super::scratchpad_manager::ScratchpadManager;
use super::tui::MenuServerTui;
use crate::common::compositor::CompositorType;
use crate::scratchpad::config::ScratchpadConfig;
use anyhow::{Context, Result};
use std::io::{self, BufRead, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::thread;
use std::time::Duration;
use tokio::signal;

/// Global registry for tracking active menu processes (fzf, yazi, etc.) that can be cancelled
/// when scratchpad becomes invisible
static ACTIVE_MENU_PROCESSES: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// Register a process ID as an active menu process
pub fn register_menu_process(pid: u32) -> Result<()> {
    let mut processes = ACTIVE_MENU_PROCESSES
        .lock()
        .map_err(|e| anyhow::anyhow!("Failed to acquire process lock: {}", e))?;
    processes.push(pid);
    Ok(())
}

/// Unregister a process ID (called when process completes normally)
pub fn unregister_menu_process(pid: u32) {
    if let Ok(mut processes) = ACTIVE_MENU_PROCESSES.lock() {
        processes.retain(|&p| p != pid);
    }
}

/// Kill all registered menu processes (called when scratchpad becomes invisible)
/// Uses SIGINT to simulate the same behavior as pressing ESC
pub fn kill_active_menu_processes() -> Result<usize> {
    let processes = if let Ok(mut procs) = ACTIVE_MENU_PROCESSES.lock() {
        let current = procs.clone();
        procs.clear(); // Clear the list since we're killing them all
        current
    } else {
        return Ok(0); // If we can't lock, just return
    };

    let count = processes.len();
    for pid in processes {
        // Use SIGINT (same as Ctrl+C/ESC) instead of SIGTERM to match normal cancellation behavior
        let _ = std::process::Command::new("kill")
            .arg("-INT")
            .arg(pid.to_string())
            .output();
    }

    Ok(count)
}

/// Menu server for handling GUI menu requests
pub struct MenuServer {
    socket_path: String,
    running: Arc<AtomicBool>,
    start_time: std::time::SystemTime,
    requests_processed: Arc<AtomicU64>,
    compositor: CompositorType,
    scratchpad_manager: Option<ScratchpadManager>,
    tui: Option<MenuServerTui>,
}

impl MenuServer {
    /// Create a new menu server
    pub fn new() -> Result<Self> {
        let tui = MenuServerTui::new()?;
        Ok(Self {
            socket_path: default_socket_path(),
            running: Arc::new(AtomicBool::new(false)),
            start_time: std::time::SystemTime::now(),
            requests_processed: Arc::new(AtomicU64::new(0)),
            compositor: CompositorType::detect(),
            scratchpad_manager: None,
            tui: Some(tui),
        })
    }

    /// Create a menu server with compositor type and optional scratchpad config
    pub fn with_compositor_and_scratchpad(
        compositor: CompositorType,
        scratchpad_config: Option<ScratchpadConfig>,
    ) -> Result<Self> {
        let scratchpad_manager =
            scratchpad_config.map(|config| ScratchpadManager::new(compositor.clone(), config));
        let tui = MenuServerTui::new()?;

        Ok(Self {
            socket_path: default_socket_path(),
            running: Arc::new(AtomicBool::new(false)),
            start_time: std::time::SystemTime::now(),
            requests_processed: Arc::new(AtomicU64::new(0)),
            compositor,
            scratchpad_manager,
            tui: Some(tui),
        })
    }

    /// Start the server
    pub async fn start(&mut self) -> Result<()> {
        if Path::new(&self.socket_path).exists() {
            std::fs::remove_file(&self.socket_path)
                .context("Failed to remove existing socket file")?;
        }

        let listener = UnixListener::bind(&self.socket_path)
            .context(format!("Failed to bind to socket at {}", self.socket_path))?;

        self.running.store(true, Ordering::SeqCst);

        // TUI is already initialized in the struct

        let running_clone = self.running.clone();
        let socket_path_clone = self.socket_path.clone();
        tokio::spawn(async move {
            let mut sigint = signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
                .expect("Failed to setup SIGINT handler");
            let mut sigterm = signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("Failed to setup SIGTERM handler");

            tokio::select! {
                _ = sigint.recv() => {}
                _ = sigterm.recv() => {}
            }
            running_clone.store(false, Ordering::SeqCst);
            if Path::new(&socket_path_clone).exists() {
                let _ = std::fs::remove_file(&socket_path_clone);
            }
        });

        while self.running.load(Ordering::SeqCst) {
            listener.set_nonblocking(true)?;

            match listener.accept() {
                Ok((stream, _addr)) => {
                    // Temporarily suspend TUI for connection handling
                    if let Some(ref mut tui) = self.tui {
                        tui.suspend()?;
                    }

                    let _ = self.handle_connection_sync(stream);

                    // Resume TUI after connection handling
                    if let Some(ref mut tui) = self.tui {
                        tui.resume()?;
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // Draw the status screen using TUI module
                    if let Some(ref mut tui) = self.tui {
                        let has_scratchpad = self.scratchpad_manager.is_some();
                        let requests_processed = self.requests_processed.load(Ordering::SeqCst);
                        tui.draw_status_screen(
                            has_scratchpad,
                            requests_processed,
                            self.start_time,
                        )?;

                        // Sleep to prevent high CPU usage - no event handling to allow input buffering
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                    continue;
                }
                Err(_e) => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }

        // Cleanup TUI
        if let Some(ref mut tui) = self.tui {
            tui.cleanup()?;
        }

        self.cleanup_socket().await;
        Ok(())
    }

    /// Clean up socket file
    async fn cleanup_socket(&self) {
        if Path::new(&self.socket_path).exists() {
            if let Err(e) = std::fs::remove_file(&self.socket_path) {
                eprintln!("Failed to remove socket file: {e}");
            } else {
                println!("Socket file cleaned up");
            }
        }
    }

    /// Handle a client connection synchronously
    fn handle_connection_sync(&self, mut stream: UnixStream) -> Result<()> {
        // Increment request counter for debugging
        self.requests_processed.fetch_add(1, Ordering::SeqCst);

        // Set read timeout
        stream.set_read_timeout(Some(Duration::from_secs(30)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;

        // Read request
        let mut request_json = String::new();
        let mut reader = io::BufReader::new(&mut stream);

        reader.read_line(&mut request_json)?;

        if request_json.is_empty() {
            // Client disconnected - this is normal, not an error
            return Ok(());
        }

        // Parse request
        let message: MenuMessage =
            serde_json::from_str(request_json.trim()).context("Failed to deserialize request")?;

        // Process request and generate response (synchronously for now)
        let response = self.process_request_sync(message.payload)?;

        // Create response envelope
        let response_message = MenuResponseMessage {
            request_id: message.request_id,
            payload: response,
            timestamp: std::time::SystemTime::now(),
        };

        // Send response
        let response_json =
            serde_json::to_string(&response_message).context("Failed to serialize response")?;

        stream.write_all(response_json.as_bytes())?;
        stream.write_all(b"\n")?; // Message delimiter

        Ok(())
    }

    /// Process a menu request with scratchpad visibility management and timeout
    fn process_request_sync(&self, request: MenuRequest) -> Result<MenuResponse> {
        // Handle Show request specially for fast response
        if matches!(request, MenuRequest::Show) {
            if let Some(ref manager) = self.scratchpad_manager
                && let Err(e) = manager.show_fast()
            {
                eprintln!("Warning: Failed to show scratchpad: {e}");
            }
            return Ok(MenuResponse::ShowResult);
        }

        // Show scratchpad if configured (for interactive requests only)
        let should_manage_scratchpad = matches!(
            request,
            MenuRequest::Confirm { .. }
                | MenuRequest::Choice { .. }
                | MenuRequest::Chord { .. }
                | MenuRequest::Input { .. }
                | MenuRequest::Password { .. }
                | MenuRequest::FilePicker { .. }
                | MenuRequest::Slide(_)
        );

        if should_manage_scratchpad
            && let Some(ref manager) = self.scratchpad_manager
            && let Err(e) = manager.show()
        {
            eprintln!("Warning: Failed to show scratchpad: {e}");
        }

        // Process the request with timeout and visibility monitoring
        let response = if should_manage_scratchpad && self.scratchpad_manager.is_some() {
            self.process_request_with_integrated_monitoring(request)?
        } else {
            // Non-interactive requests or no scratchpad don't need monitoring
            self.process_request_internal(request)?
        };

        // **PERFORMANCE CRITICAL**: Hide scratchpad IMMEDIATELY after menu interaction
        // This must be the FIRST thing we do after the user completes their interaction
        // to return control to the user as fast as possible.
        //
        // NOTE: For monitored requests, monitoring is already stopped in process_request_with_integrated_monitoring
        // before this point to prevent false cancellations when we intentionally hide the scratchpad.
        if should_manage_scratchpad
            && let Some(ref manager) = self.scratchpad_manager
            && let Err(e) = manager.hide_fast()
        {
            eprintln!("Warning: Failed to hide scratchpad: {e}");
        }

        Ok(response)
    }

    /// Process a menu request internal logic using the dedicated processor
    fn process_request_internal(&self, request: MenuRequest) -> Result<MenuResponse> {
        // Handle status request specially to get server-specific information
        if matches!(request, MenuRequest::Status) {
            return Ok(self.get_status_info());
        }

        let processor =
            RequestProcessor::new(self.running.clone(), self.requests_processed.clone());
        processor.process_internal(request)
    }

    /// Process request with monitoring integrated directly (much simpler approach)
    fn process_request_with_integrated_monitoring(
        &self,
        request: MenuRequest,
    ) -> Result<MenuResponse> {
        // Start a background monitoring thread that will kill fzf processes if scratchpad becomes invisible
        let monitoring_active = Arc::new(AtomicBool::new(true));
        let monitoring_active_clone = monitoring_active.clone();
        let was_killed = Arc::new(AtomicBool::new(false));
        let was_killed_clone = was_killed.clone();

        let monitoring_handle = if let Some(ref manager) = self.scratchpad_manager {
            let compositor = manager.compositor().clone();
            let config = manager.config().clone();

            Some(thread::spawn(move || {
                let check_interval = Duration::from_millis(100);

                // Grace period for KDE: allow some time for the window to appear (250ms)
                // This prevents race conditions where the window is being shown but not reported as visible yet
                if matches!(compositor, crate::common::compositor::CompositorType::KWin) {
                    for _ in 0..5 {
                        if !monitoring_active_clone.load(Ordering::SeqCst) {
                            return;
                        }
                        thread::sleep(Duration::from_millis(50));
                    }
                }

                let mut consecutive_failures = 0;
                // Require multiple consecutive failures for KWin to handle flaky visibility reporting
                let max_failures =
                    if matches!(compositor, crate::common::compositor::CompositorType::KWin) {
                        5
                    } else {
                        1
                    };

                while monitoring_active_clone.load(Ordering::SeqCst) {
                    match compositor.provider().is_visible(&config) {
                        Ok(false) => {
                            consecutive_failures += 1;
                            if consecutive_failures >= max_failures {
                                // Scratchpad became invisible
                                // Only cancel if we actually killed external processes (like fzf)
                                // For internal TUIs (like Chord), we don't want to cancel on false positives
                                if let Ok(killed_count) = kill_active_menu_processes()
                                    && killed_count > 0
                                {
                                    println!(
                                        "Scratchpad became invisible, cancelling menu operation"
                                    );
                                    was_killed_clone.store(true, Ordering::SeqCst);
                                }
                                break;
                            }
                        }
                        Ok(true) => {
                            // Still visible, continue monitoring
                            consecutive_failures = 0;
                        }
                        Err(_) => {
                            // Continue monitoring despite error
                        }
                    }

                    thread::sleep(check_interval);
                }
            }))
        } else {
            None
        };

        // Process the request normally - if fzf gets killed, it will return cancelled
        let result = self.process_request_internal(request);

        // **CRITICAL**: Stop monitoring BEFORE hiding scratchpad to prevent false cancellations
        // The monitoring thread would detect the intentional hiding and cancel the operation
        monitoring_active.store(false, Ordering::SeqCst);

        // Wait for monitoring thread to complete (must complete before we hide the scratchpad)
        if let Some(handle) = monitoring_handle {
            let _ = handle.join();
        }

        // If the process was killed due to invisibility, return cancelled regardless of what fzf returned
        if was_killed.load(Ordering::SeqCst) {
            Ok(MenuResponse::Cancelled)
        } else {
            result
        }
    }

    /// Get server status information
    fn get_status_info(&self) -> MenuResponse {
        let status = if self.running.load(Ordering::SeqCst) {
            ServerStatus::Ready
        } else {
            ServerStatus::ShuttingDown
        };

        let uptime = self.start_time.elapsed().unwrap_or_default().as_secs();

        let start_time_str = chrono::DateTime::from_timestamp(
            self.start_time
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
            0,
        )
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "Unknown".to_string());

        let status_info = StatusInfo {
            status,
            version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_version: PROTOCOL_VERSION.to_string(),
            uptime_seconds: uptime,
            socket_path: self.socket_path.clone(),
            requests_processed: self.requests_processed.load(Ordering::SeqCst),
            start_time: start_time_str,
            compositor: self.compositor.name(),
        };

        MenuResponse::StatusResult(status_info)
    }
}

impl Default for MenuServer {
    fn default() -> Self {
        Self::new().expect("Failed to create default MenuServer")
    }
}

/// Create a scratchpad configuration for the menu server
pub fn create_menu_server_scratchpad_config() -> ScratchpadConfig {
    use crate::scratchpad::{config::ScratchpadConfig, terminal::Terminal};

    // Get current executable path for the inner command
    let current_exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| env!("CARGO_BIN_NAME").to_string());

    let inner_command = format!("{current_exe} menu server launch --inside");

    ScratchpadConfig::with_params(
        "insmenu".to_string(),
        Terminal::default(), // Use default terminal (kitty)
        Some(inner_command),
        50, // 50% width
        60, // 60% height
    )
}

/// Run the menu server in --inside mode
pub async fn run_server_inside(no_scratchpad: bool) -> Result<i32> {
    // Create server with scratchpad config for self-management
    let scratchpad_config = if no_scratchpad {
        None
    } else {
        Some(create_menu_server_scratchpad_config())
    };
    let compositor = CompositorType::detect();
    let mut server = MenuServer::with_compositor_and_scratchpad(compositor, scratchpad_config)?;

    // When running --inside, the scratchpad is initially visible
    if let Some(ref manager) = server.scratchpad_manager {
        manager.mark_visible();
    }

    // Clear screen and start server
    print!("\x1B[2J\x1B[H"); // Clear screen and move cursor to top-left
    if let Err(e) = server.start().await {
        eprintln!("Server error: {e}");
        return Ok(1);
    }

    Ok(0)
}

/// Run the menu server by launching external terminal in scratchpad
pub async fn run_server_launch(no_scratchpad: bool) -> Result<i32> {
    if no_scratchpad {
        // If no scratchpad is requested, just run the server in the current terminal.
        // This is effectively the same as running with --inside, but without a scratchpad manager.
        return run_server_inside(true).await;
    }

    let compositor = CompositorType::detect();
    let scratchpad_config = create_menu_server_scratchpad_config();

    println!("Launching menu server in scratchpad...");

    // Create and show the scratchpad with the menu server running inside
    match compositor.provider().show(&scratchpad_config) {
        Ok(()) => {
            println!("Menu server scratchpad launched successfully");
            Ok(0)
        }
        Err(e) => {
            eprintln!("Failed to launch menu server scratchpad: {e}");
            Ok(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_creation() {
        // Skip TUI initialization in tests since it requires a terminal
        let server = MenuServer {
            socket_path: default_socket_path(),
            running: Arc::new(AtomicBool::new(false)),
            start_time: std::time::SystemTime::now(),
            requests_processed: Arc::new(AtomicU64::new(0)),
            compositor: CompositorType::detect(),
            scratchpad_manager: None,
            tui: None, // Skip TUI for tests
        };
        assert!(!server.socket_path.is_empty());
    }
}
