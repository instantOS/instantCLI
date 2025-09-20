use super::MenuCommands;
use super::protocol::*;
use anyhow::{Context, Result};
use colored::*;
use std::io::{self, BufRead, Read, Write};
use std::os::unix::net::UnixStream;
use std::process::Command;
use std::time::Duration;

/// Menu client for communicating with the menu server
pub struct MenuClient {
    socket_path: String,
}

impl MenuClient {
    /// Create a new menu client
    pub fn new() -> Self {
        Self {
            socket_path: default_socket_path(),
        }
    }

    /// Try to connect to the server with timeout
    pub fn connect(&self) -> Result<UnixStream> {
        let stream = UnixStream::connect(&self.socket_path).context(format!(
            "Failed to connect to socket at {}",
            self.socket_path
        ))?;

        // Set read timeout
        stream.set_read_timeout(Some(Duration::from_secs(30)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;

        Ok(stream)
    }

    /// Check if server is running
    pub fn is_server_running(&self) -> bool {
        self.connect().is_ok()
    }

    /// Spawn server if not running using scratchpad architecture
    pub fn ensure_server_running(&self) -> Result<()> {
        if self.is_server_running() {
            return Ok(());
        }

        // Server is not running, spawn it in a scratchpad
        let current_exe =
            std::env::current_exe().context("Failed to get current executable path")?;

        let output = Command::new(current_exe)
            .args(["menu", "server", "launch"])
            .output()
            .context("Failed to spawn menu server in scratchpad")?;

        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to spawn menu server in scratchpad: {}", error_msg);
        }

        // Wait a moment for server to start
        std::thread::sleep(Duration::from_millis(1000)); // Slightly longer wait for scratchpad setup

        // Check if server is now running
        if !self.is_server_running() {
            anyhow::bail!("Server failed to start after spawning in scratchpad");
        }

        Ok(())
    }

    /// Send a request and receive response
    pub fn send_request(&self, request: MenuRequest) -> Result<MenuResponse> {
        // Ensure server is running
        self.ensure_server_running()?;

        // Connect to server
        let mut stream = self.connect()?;

        // Create message envelope
        let message = MenuMessage {
            request_id: generate_request_id(),
            payload: request,
            timestamp: std::time::SystemTime::now(),
        };

        // Serialize and send request
        let request_json =
            serde_json::to_string(&message).context("Failed to serialize request")?;

        stream.write_all(request_json.as_bytes())?;
        stream.write_all(b"\n")?; // Message delimiter

        // Read response
        let mut response_json = String::new();
        let mut reader = io::BufReader::new(&stream);

        reader.read_line(&mut response_json)?;

        if response_json.is_empty() {
            anyhow::bail!("Received empty response from server");
        }

        // Deserialize response
        let response_message: MenuResponseMessage =
            serde_json::from_str(response_json.trim()).context("Failed to deserialize response")?;

        // Verify request ID matches
        if response_message.request_id != message.request_id {
            anyhow::bail!("Request ID mismatch in response");
        }

        Ok(response_message.payload)
    }

    /// Show confirmation dialog via server
    pub fn confirm(&self, message: String) -> Result<ConfirmResult> {
        match self.send_request(MenuRequest::Confirm { message })? {
            MenuResponse::ConfirmResult(result) => Ok(result),
            MenuResponse::Error(error) => anyhow::bail!("Server error: {}", error),
            MenuResponse::Cancelled => Ok(ConfirmResult::Cancelled),
            _ => anyhow::bail!("Unexpected response type for confirm request"),
        }
    }

    /// Show choice dialog via server
    pub fn choice(
        &self,
        prompt: String,
        items: Vec<SerializableMenuItem>,
        multi: bool,
    ) -> Result<Vec<SerializableMenuItem>> {
        match self.send_request(MenuRequest::Choice {
            prompt,
            items,
            multi,
        })? {
            MenuResponse::ChoiceResult(selected) => Ok(selected),
            MenuResponse::Error(error) => anyhow::bail!("Server error: {}", error),
            MenuResponse::Cancelled => Ok(vec![]),
            _ => anyhow::bail!("Unexpected response type for choice request"),
        }
    }

    /// Show input dialog via server
    pub fn input(&self, prompt: String) -> Result<String> {
        match self.send_request(MenuRequest::Input { prompt })? {
            MenuResponse::InputResult(text) => Ok(text),
            MenuResponse::Error(error) => anyhow::bail!("Server error: {}", error),
            MenuResponse::Cancelled => Ok(String::new()),
            _ => anyhow::bail!("Unexpected response type for input request"),
        }
    }

    /// Show the scratchpad without any other action
    pub fn show(&self) -> Result<()> {
        match self.send_request(MenuRequest::Show)? {
            MenuResponse::ShowResult => Ok(()),
            MenuResponse::Error(error) => anyhow::bail!("Server error: {}", error),
            _ => anyhow::bail!("Unexpected response type for show request"),
        }
    }

    /// Get server status information
    pub fn status(&self) -> Result<StatusInfo> {
        match self.send_request(MenuRequest::Status)? {
            MenuResponse::StatusResult(status_info) => Ok(status_info),
            MenuResponse::Error(error) => anyhow::bail!("Server error: {}", error),
            _ => anyhow::bail!("Unexpected response type for status request"),
        }
    }

    /// Stop the server
    pub fn stop(&self) -> Result<()> {
        // Check if server is running first
        if !self.is_server_running() {
            anyhow::bail!("Server is not running");
        }

        let mut stream = self.connect()?;

        // Create message envelope
        let message = MenuMessage {
            request_id: generate_request_id(),
            payload: MenuRequest::Stop,
            timestamp: std::time::SystemTime::now(),
        };

        // Serialize and send request
        let request_json =
            serde_json::to_string(&message).context("Failed to serialize stop request")?;

        stream.write_all(request_json.as_bytes())?;
        stream.write_all(b"\n")?; // Message delimiter

        // Read response
        let mut response_json = String::new();
        let mut reader = io::BufReader::new(&stream);

        reader.read_line(&mut response_json)?;

        if response_json.is_empty() {
            anyhow::bail!("Received empty response from server");
        }

        // Deserialize response
        let response_message: MenuResponseMessage = serde_json::from_str(response_json.trim())
            .context("Failed to deserialize stop response")?;

        // Verify request ID matches
        if response_message.request_id != message.request_id {
            anyhow::bail!("Request ID mismatch in stop response");
        }

        match response_message.payload {
            MenuResponse::StopResult => Ok(()),
            MenuResponse::Error(error) => anyhow::bail!("Server error: {}", error),
            _ => anyhow::bail!("Unexpected response type for stop request"),
        }
    }
}

impl Default for MenuClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle GUI menu requests by routing through client
pub fn handle_gui_request(command: &MenuCommands) -> Result<i32> {
    let client = MenuClient::new();

    match command {
        MenuCommands::Confirm { message, gui: true } => {
            match client.confirm(message.clone()) {
                Ok(result) => Ok(result.into()),
                Err(e) => {
                    eprintln!("GUI menu error: {e}");
                    Ok(3) // Error exit code
                }
            }
        }
        MenuCommands::Choice {
            prompt,
            items,
            multi,
            gui: true,
        } => {
            let item_list: Vec<SerializableMenuItem> = if items.is_empty() {
                // Read from stdin if items is empty
                let mut buffer = String::new();
                io::stdin()
                    .read_to_string(&mut buffer)
                    .map_err(|e| anyhow::anyhow!("Failed to read from stdin: {}", e))?;
                buffer
                    .lines()
                    .map(|s| SerializableMenuItem {
                        display_text: s.to_string(),
                        preview: FzfPreview::None,
                        metadata: None,
                    })
                    .collect()
            } else {
                // Split space-separated items from command line
                items
                    .split(' ')
                    .map(|s| SerializableMenuItem {
                        display_text: s.to_string(),
                        preview: FzfPreview::None,
                        metadata: None,
                    })
                    .collect()
            };

            match client.choice(prompt.clone(), item_list, *multi) {
                Ok(selected) => {
                    if selected.is_empty() {
                        Ok(1) // Cancelled
                    } else {
                        for item in selected {
                            println!("{}", item.display_text);
                        }
                        Ok(0) // Success
                    }
                }
                Err(e) => {
                    eprintln!("GUI menu error: {e}");
                    Ok(3) // Error exit code
                }
            }
        }
        MenuCommands::Input { prompt, gui: true } => {
            match client.input(prompt.clone()) {
                Ok(text) => {
                    println!("{text}");
                    Ok(0) // Success
                }
                Err(e) => {
                    eprintln!("GUI menu error: {e}");
                    Ok(3) // Error exit code
                }
            }
        }
        _ => anyhow::bail!("Not a GUI menu command"),
    }
}

/// Print formatted status information
pub fn print_status_info(status: &StatusInfo) {
    println!("{}", "InstantCLI Menu Server Status".bold().underline());

    // Status with color coding
    let status_text = match status.status {
        ServerStatus::Ready => "Ready".green(),
        ServerStatus::Busy => "Busy".yellow(),
        ServerStatus::ShuttingDown => "Shutting Down".red(),
    };

    println!("Status:           {status_text}");
    println!("Version:          {}", status.version.blue());
    println!("Protocol:         {}", status.protocol_version.blue());
    println!("Compositor:       {}", status.compositor.yellow());
    println!("Socket:           {}", status.socket_path);
    println!(
        "Requests:         {}",
        status.requests_processed.to_string().cyan()
    );
    println!(
        "Uptime:           {} seconds",
        status.uptime_seconds.to_string().cyan()
    );
    println!("Started:          {}", status.start_time);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = MenuClient::new();
        assert!(!client.socket_path.is_empty());
    }

    #[test]
    fn test_request_id_generation() {
        let id1 = generate_request_id();
        let id2 = generate_request_id();
        assert_ne!(id1, id2);
        assert!(id1.starts_with("req_"));
    }
}
