use super::protocol::*;
use crate::fzf_wrapper::{FzfOptions, FzfSelectable, FzfWrapper};
use anyhow::{Context, Result};
use std::io::{self, BufRead, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::signal;

/// Menu server for handling GUI menu requests
pub struct MenuServer {
    socket_path: String,
    running: Arc<AtomicBool>,
}

impl MenuServer {
    /// Create a new menu server
    pub fn new() -> Self {
        Self {
            socket_path: default_socket_path(),
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Create a menu server with custom socket path
    pub fn with_socket_path(socket_path: String) -> Self {
        Self {
            socket_path,
            running: Arc::new(AtomicBool::new(false)),
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

    /// Process a menu request
    fn process_request(&self, request: MenuRequest) -> Result<MenuResponse> {
        match request {
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
        }
    }

    /// Handle choice request with item selection
    fn handle_choice_request(
        &self,
        prompt: String,
        items: Vec<String>,
        multi: bool,
    ) -> Result<MenuResponse> {
        if items.is_empty() {
            return Ok(MenuResponse::Error("No items to choose from".to_string()));
        }

        #[derive(Debug, Clone)]
        struct SelectItem {
            text: String,
        }

        impl FzfSelectable for SelectItem {
            fn fzf_display_text(&self) -> String {
                self.text.clone()
            }
        }

        let select_items: Vec<SelectItem> =
            items.into_iter().map(|text| SelectItem { text }).collect();

        let wrapper = FzfWrapper::with_options(FzfOptions {
            prompt: Some(prompt),
            multi_select: multi,
            additional_args: vec![],
            ..Default::default()
        });

        match wrapper.select(select_items) {
            Ok(crate::fzf_wrapper::FzfResult::Selected(item)) => {
                Ok(MenuResponse::ChoiceResult(vec![item.text]))
            }
            Ok(crate::fzf_wrapper::FzfResult::MultiSelected(items)) => {
                let selected_texts = items.into_iter().map(|item| item.text).collect();
                Ok(MenuResponse::ChoiceResult(selected_texts))
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
}

impl Default for MenuServer {
    fn default() -> Self {
        Self::new()
    }
}

/// Run the menu server in --inside mode
pub async fn run_server_inside() -> Result<i32> {
    let mut server = MenuServer::new();

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

/// Run the menu server by launching external terminal
pub fn run_server_launch() -> Result<i32> {
    // For now, just print a message since we need terminal integration
    println!("Menu server launch mode not implemented yet");
    println!(
        "In the future, this will launch a terminal with: instant menu server launch --inside"
    );
    Ok(1)
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
