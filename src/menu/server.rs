use super::protocol::*;
use crate::common::compositor::CompositorType;
use crate::fzf_wrapper::{FzfOptions, FzfWrapper};
use crate::scratchpad::{config::ScratchpadConfig, hide_scratchpad, show_scratchpad};
use anyhow::{Context, Result};
use std::io::{self, BufRead, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;
use tokio::signal;

/// Menu server for handling GUI menu requests
pub struct MenuServer {
    socket_path: String,
    running: Arc<AtomicBool>,
    start_time: std::time::SystemTime,
    requests_processed: Arc<AtomicU64>,
    compositor: CompositorType,
    scratchpad_config: Option<ScratchpadConfig>,
    scratchpad_visible: Arc<AtomicBool>,
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
            scratchpad_config: None,
            scratchpad_visible: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Create a menu server with custom socket path
    pub fn with_socket_path(socket_path: String) -> Self {
        Self {
            socket_path,
            running: Arc::new(AtomicBool::new(false)),
            start_time: std::time::SystemTime::now(),
            requests_processed: Arc::new(AtomicU64::new(0)),
            compositor: CompositorType::detect(),
            scratchpad_config: None,
            scratchpad_visible: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Create a menu server with compositor type and optional scratchpad config
    pub fn with_compositor_and_scratchpad(
        compositor: CompositorType,
        scratchpad_config: Option<ScratchpadConfig>,
    ) -> Self {
        Self {
            socket_path: default_socket_path(),
            running: Arc::new(AtomicBool::new(false)),
            start_time: std::time::SystemTime::now(),
            requests_processed: Arc::new(AtomicU64::new(0)),
            compositor,
            scratchpad_config,
            scratchpad_visible: Arc::new(AtomicBool::new(false)),
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
                    eprintln!("Failed to remove socket file during shutdown: {}", e);
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
                    if let Err(e) = self.handle_connection(stream) {
                        eprintln!("Error handling connection from {:?}: {}", addr, e);
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // No incoming connections, wait a bit before trying again
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    continue;
                }
                Err(e) => {
                    eprintln!("Error accepting connection: {}", e);
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
                eprintln!("Failed to remove socket file: {}", e);
            } else {
                println!("Socket file cleaned up");
            }
        }
    }

    /// Handle a client connection
    fn handle_connection(&self, mut stream: UnixStream) -> Result<()> {
        // Increment request counter
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
            serde_json::from_str(&request_json.trim()).context("Failed to deserialize request")?;

        // Process request and generate response
        let response = self.process_request(message.payload)?;

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

    /// Process a menu request with scratchpad visibility management
    fn process_request(&self, request: MenuRequest) -> Result<MenuResponse> {
        // Show scratchpad if configured (for interactive requests only)
        let should_manage_scratchpad = matches!(
            request,
            MenuRequest::Confirm { .. } | MenuRequest::Choice { .. } | MenuRequest::Input { .. }
        );

        if should_manage_scratchpad {
            if let Err(e) = self.show_scratchpad() {
                eprintln!("Warning: Failed to show scratchpad: {}", e);
            }
        }

        // Process the request
        let response = match request {
            MenuRequest::Confirm { message } => match FzfWrapper::confirm(&message) {
                Ok(result) => Ok(MenuResponse::ConfirmResult(result.into())),
                Err(e) => Ok(MenuResponse::Error(format!(
                    "Failed to show confirm dialog: {}",
                    e
                ))),
            },
            MenuRequest::Choice {
                prompt,
                items,
                multi,
            } => self.handle_choice_request(prompt, items, multi),
            MenuRequest::Input { prompt } => match FzfWrapper::input(&prompt) {
                Ok(input) => Ok(MenuResponse::InputResult(input)),
                Err(e) => Ok(MenuResponse::Error(format!(
                    "Failed to show input dialog: {}",
                    e
                ))),
            },
            MenuRequest::Status => Ok(self.get_status_info()),
        };

        // Hide scratchpad after processing (for interactive requests only)
        if should_manage_scratchpad {
            if let Err(e) = self.hide_scratchpad() {
                eprintln!("Warning: Failed to hide scratchpad: {}", e);
            }
        }

        response
    }

    /// Handle choice request with item selection
    fn handle_choice_request(
        &self,
        prompt: String,
        items: Vec<SerializableMenuItem>,
        multi: bool,
    ) -> Result<MenuResponse> {
        if items.is_empty() {
            return Ok(MenuResponse::Error("No items to choose from".to_string()));
        }

        let wrapper = FzfWrapper::with_options(FzfOptions {
            prompt: Some(prompt),
            multi_select: multi,
            ..Default::default()
        });

        match wrapper.select(items) {
            Ok(crate::fzf_wrapper::FzfResult::Selected(item)) => {
                Ok(MenuResponse::ChoiceResult(vec![item]))
            }
            Ok(crate::fzf_wrapper::FzfResult::MultiSelected(items)) => {
                Ok(MenuResponse::ChoiceResult(items))
            }
            Ok(crate::fzf_wrapper::FzfResult::Cancelled) => Ok(MenuResponse::Cancelled),
            Ok(crate::fzf_wrapper::FzfResult::Error(e)) => {
                Ok(MenuResponse::Error(format!("Selection error: {}", e)))
            }
            Err(e) => Ok(MenuResponse::Error(format!(
                "Failed to show selection dialog: {}",
                e
            ))),
        }
    }

    /// Stop the server
    pub async fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        self.cleanup_socket().await;
    }

    /// Check if server is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Get the detected compositor type
    pub fn compositor(&self) -> &CompositorType {
        &self.compositor
    }

    /// Show the scratchpad if configured and not already visible
    fn show_scratchpad(&self) -> Result<()> {
        if let Some(ref config) = self.scratchpad_config {
            // Check if already visible to avoid unnecessary operations
            if !self.scratchpad_visible.load(Ordering::SeqCst) {
                show_scratchpad(&self.compositor, config)
                    .context("Failed to show menu server scratchpad")?;
                self.scratchpad_visible.store(true, Ordering::SeqCst);
            }
        }
        Ok(())
    }

    /// Hide the scratchpad if configured and currently visible
    fn hide_scratchpad(&self) -> Result<()> {
        if let Some(ref config) = self.scratchpad_config {
            // Check if currently visible to avoid unnecessary operations
            if self.scratchpad_visible.load(Ordering::SeqCst) {
                hide_scratchpad(&self.compositor, config)
                    .context("Failed to hide menu server scratchpad")?;
                self.scratchpad_visible.store(false, Ordering::SeqCst);
            }
        }
        Ok(())
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

    let inner_command = format!("{} menu server launch --inside", current_exe);

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
    server.scratchpad_visible.store(true, Ordering::SeqCst);

    println!("Starting InstantCLI Menu Server in --inside mode");
    println!("Press Ctrl+C to stop the server");

    // Clear screen and start server
    print!("\x1B[2J\x1B[H"); // Clear screen and move cursor to top-left
    if let Err(e) = server.start().await {
        eprintln!("Server error: {}", e);
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
            eprintln!("Failed to launch menu server scratchpad: {}", e);
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
        assert!(!server.is_running());
    }

    #[test]
    fn test_custom_socket_path() {
        let server = MenuServer::with_socket_path("/tmp/test.sock".to_string());
        assert_eq!(server.socket_path, "/tmp/test.sock");
    }
}
