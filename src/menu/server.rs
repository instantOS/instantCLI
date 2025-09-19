use super::processing::RequestProcessor;
use super::protocol::*;
use super::scratchpad_manager::ScratchpadManager;
use crate::common::compositor::CompositorType;
use crate::scratchpad::{
    config::ScratchpadConfig, show_scratchpad, visibility::is_scratchpad_visible,
};
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

/// Global registry for tracking active fzf processes that can be killed
/// when scratchpad becomes invisible
static ACTIVE_FZF_PROCESSES: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// Register a process ID as an active fzf process
pub fn register_fzf_process(pid: u32) -> Result<()> {
    let mut processes = ACTIVE_FZF_PROCESSES
        .lock()
        .map_err(|e| anyhow::anyhow!("Failed to acquire process lock: {}", e))?;
    processes.push(pid);
    Ok(())
}

/// Unregister a process ID (called when process completes normally)
pub fn unregister_fzf_process(pid: u32) {
    if let Ok(mut processes) = ACTIVE_FZF_PROCESSES.lock() {
        processes.retain(|&p| p != pid);
    }
}

/// Kill all registered fzf processes (called when scratchpad becomes invisible)
/// Uses SIGINT to simulate the same behavior as pressing ESC
pub fn kill_active_fzf_processes() -> Result<()> {
    let processes = if let Ok(mut procs) = ACTIVE_FZF_PROCESSES.lock() {
        let current = procs.clone();
        procs.clear(); // Clear the list since we're killing them all
        current
    } else {
        return Ok(()); // If we can't lock, just return
    };

    for pid in processes {
        // Use SIGINT (same as Ctrl+C/ESC) instead of SIGTERM to match normal cancellation behavior
        let _ = std::process::Command::new("kill")
            .arg("-INT")
            .arg(pid.to_string())
            .output();
    }

    Ok(())
}

/// Menu server for handling GUI menu requests
pub struct MenuServer {
    socket_path: String,
    running: Arc<AtomicBool>,
    start_time: std::time::SystemTime,
    requests_processed: Arc<AtomicU64>,
    compositor: CompositorType,
    scratchpad_manager: Option<ScratchpadManager>,
}

impl MenuServer {
    /// Create a new menu server
    pub fn new() -> Self {
        Self {
            socket_path: default_socket_path(),
            running: Arc::new(AtomicBool::new(false)),
            start_time: std::time::SystemTime::now(),
            requests_processed: Arc::new(AtomicU64::new(0)),
            compositor: CompositorType::detect(),
            scratchpad_manager: None,
        }
    }

    /// Create a menu server with compositor type and optional scratchpad config
    pub fn with_compositor_and_scratchpad(
        compositor: CompositorType,
        scratchpad_config: Option<ScratchpadConfig>,
    ) -> Self {
        let scratchpad_manager =
            scratchpad_config.map(|config| ScratchpadManager::new(compositor.clone(), config));

        Self {
            socket_path: default_socket_path(),
            running: Arc::new(AtomicBool::new(false)),
            start_time: std::time::SystemTime::now(),
            requests_processed: Arc::new(AtomicU64::new(0)),
            compositor,
            scratchpad_manager,
        }
    }

    /// Start the server
    pub async fn start(&mut self) -> Result<()> {
        // Remove existing socket file if it exists
        if Path::new(&self.socket_path).exists() {
            std::fs::remove_file(&self.socket_path)
                .context("Failed to remove existing socket file")?;
        }

        // Create Unix domain socket listener
        let listener = UnixListener::bind(&self.socket_path)
            .context(format!("Failed to bind to socket at {}", self.socket_path))?;

        println!("Menu server listening on {}", self.socket_path);
        self.running.store(true, Ordering::SeqCst);

        // Set up signal handling for graceful shutdown
        let running_clone = self.running.clone();
        let socket_path_clone = self.socket_path.clone();

        tokio::spawn(async move {
            let mut sigint = signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
                .expect("Failed to setup SIGINT handler");
            let mut sigterm = signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("Failed to setup SIGTERM handler");

            tokio::select! {
                _ = sigint.recv() => {
                    println!("\nReceived SIGINT (Ctrl+C), shutting down gracefully...");
                }
                _ = sigterm.recv() => {
                    println!("\nReceived SIGTERM, shutting down gracefully...");
                }
            }

            running_clone.store(false, Ordering::SeqCst);

            // Clean up socket file
            if Path::new(&socket_path_clone).exists() {
                if let Err(e) = std::fs::remove_file(&socket_path_clone) {
                    eprintln!("Failed to remove socket file during shutdown: {e}");
                }
            }

            println!("Server shutdown complete");
        });

        // Main server loop
        while self.running.load(Ordering::SeqCst) {
            // Set non-blocking mode for the listener to check running flag
            listener.set_nonblocking(true)?;

            match listener.accept() {
                Ok((stream, addr)) => {
                    // Handle connection synchronously for now to avoid ownership issues
                    if let Err(e) = self.handle_connection_sync(stream) {
                        eprintln!("Error handling connection from {addr:?}: {e}");
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // No incoming connections, wait a bit before trying again
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    continue;
                }
                Err(e) => {
                    eprintln!("Error accepting connection: {e}");
                    // Brief pause to avoid busy loop on persistent errors
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }

        // Final cleanup
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
            if let Some(ref manager) = self.scratchpad_manager {
                if let Err(e) = manager.show_fast() {
                    eprintln!("Warning: Failed to show scratchpad: {e}");
                }
            }
            return Ok(MenuResponse::ShowResult);
        }

        // Show scratchpad if configured (for interactive requests only)
        let should_manage_scratchpad = matches!(
            request,
            MenuRequest::Confirm { .. } | MenuRequest::Choice { .. } | MenuRequest::Input { .. }
        );

        if should_manage_scratchpad {
            if let Some(ref manager) = self.scratchpad_manager {
                if let Err(e) = manager.show() {
                    eprintln!("Warning: Failed to show scratchpad: {e}");
                }
            }
        }

        // Process the request with timeout and visibility monitoring
        let response = if should_manage_scratchpad && self.scratchpad_manager.is_some() {
            self.process_request_with_integrated_monitoring(request)?
        } else {
            // Non-interactive requests or no scratchpad don't need monitoring
            self.process_request_internal(request)?
        };

        // Hide scratchpad after processing (for interactive requests only)
        if should_manage_scratchpad {
            if let Some(ref manager) = self.scratchpad_manager {
                if let Err(e) = manager.hide() {
                    eprintln!("Warning: Failed to hide scratchpad: {e}");
                }
            }
        }

        Ok(response)
    }

    /// Process a menu request internal logic using the dedicated processor
    fn process_request_internal(&self, request: MenuRequest) -> Result<MenuResponse> {
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

                while monitoring_active_clone.load(Ordering::SeqCst) {
                    match is_scratchpad_visible(&compositor, &config) {
                        Ok(false) => {
                            // Scratchpad became invisible, kill all fzf processes
                            println!("Scratchpad became invisible, cancelling menu operation");
                            was_killed_clone.store(true, Ordering::SeqCst);
                            let _ = kill_active_fzf_processes();
                            break;
                        }
                        Ok(true) => {
                            // Still visible, continue monitoring
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

        // Stop monitoring
        monitoring_active.store(false, Ordering::SeqCst);

        // Wait for monitoring thread to complete
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

    /// Stop the server
    pub async fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        self.cleanup_socket().await;
    }

    /// Get the detected compositor type
    pub fn compositor(&self) -> &CompositorType {
        &self.compositor
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
        Self::new()
    }
}

/// Create a scratchpad configuration for the menu server
pub fn create_menu_server_scratchpad_config() -> ScratchpadConfig {
    use crate::scratchpad::{config::ScratchpadConfig, terminal::Terminal};

    // Get current executable path for the inner command
    let current_exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "instant".to_string());

    let inner_command = format!("{current_exe} menu server launch --inside");

    ScratchpadConfig::with_params(
        "instantmenu".to_string(),
        Terminal::default(), // Use default terminal (kitty)
        Some(inner_command),
        50, // 50% width
        60, // 60% height
    )
}

/// Run the menu server in --inside mode
pub async fn run_server_inside() -> Result<i32> {
    // Create server with scratchpad config for self-management
    let scratchpad_config = create_menu_server_scratchpad_config();
    let compositor = CompositorType::detect();
    let mut server =
        MenuServer::with_compositor_and_scratchpad(compositor, Some(scratchpad_config));

    // When running --inside, the scratchpad is initially visible
    if let Some(ref manager) = server.scratchpad_manager {
        manager.mark_visible();
    }

    println!("Starting InstantCLI Menu Server in --inside mode");
    println!("Press Ctrl+C to stop the server");

    // Clear screen and start server
    print!("\x1B[2J\x1B[H"); // Clear screen and move cursor to top-left
    if let Err(e) = server.start().await {
        eprintln!("Server error: {e}");
        return Ok(1);
    }

    Ok(0)
}

/// Run the menu server by launching external terminal in scratchpad
pub fn run_server_launch() -> Result<i32> {
    let compositor = CompositorType::detect();
    let scratchpad_config = create_menu_server_scratchpad_config();

    println!("Launching menu server in scratchpad...");

    // Create and show the scratchpad with the menu server running inside
    match show_scratchpad(&compositor, &scratchpad_config) {
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
        let server = MenuServer::new();
        assert!(!server.socket_path.is_empty());
    }
}
