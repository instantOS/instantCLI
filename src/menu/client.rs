use super::MenuCommands;
use super::protocol::*;
use crate::common::compositor::CompositorType;
use anyhow::{Context, Result};
use colored::*;
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{OnceLock, RwLock};
use std::time::Duration;
use tempfile::tempdir;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuTransport {
    ScratchpadServer,
    KittyTransient,
}

fn transport_override() -> &'static RwLock<Option<MenuTransport>> {
    static MENU_TRANSPORT_OVERRIDE: OnceLock<RwLock<Option<MenuTransport>>> = OnceLock::new();
    MENU_TRANSPORT_OVERRIDE.get_or_init(|| RwLock::new(None))
}

impl MenuTransport {
    fn detect() -> Self {
        if let Ok(guard) = transport_override().read()
            && let Some(override_transport) = *guard
        {
            return override_transport;
        }

        let compositor = CompositorType::detect();
        if compositor.provider().supports_scratchpad() {
            MenuTransport::ScratchpadServer
        } else {
            MenuTransport::KittyTransient
        }
    }
}

/// Menu client for communicating with the menu server
pub struct MenuClient {
    socket_path: String,
    transport: MenuTransport,
}

impl MenuClient {
    /// Create a new menu client
    pub fn new() -> Self {
        let transport = MenuTransport::detect();

        Self {
            socket_path: default_socket_path(),
            transport,
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

    pub fn is_fallback(&self) -> bool {
        self.transport == MenuTransport::KittyTransient
    }

    /// Spawn server if not running using scratchpad architecture
    pub fn ensure_server_running(&self) -> Result<()> {
        if self.transport != MenuTransport::ScratchpadServer {
            return Ok(());
        }

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
        match self.transport {
            MenuTransport::ScratchpadServer => self.send_request_via_server(request),
            MenuTransport::KittyTransient => self.send_request_via_fallback(request),
        }
    }

    fn send_request_via_server(&self, request: MenuRequest) -> Result<MenuResponse> {
        self.ensure_server_running()?;

        let mut stream = self.connect()?;

        let message = MenuMessage {
            request_id: generate_request_id(),
            payload: request,
            timestamp: std::time::SystemTime::now(),
        };

        let request_json =
            serde_json::to_string(&message).context("Failed to serialize request")?;

        stream.write_all(request_json.as_bytes())?;
        stream.write_all(b"\n")?;

        let mut response_json = String::new();
        let mut reader = io::BufReader::new(&stream);

        reader.read_line(&mut response_json)?;

        if response_json.is_empty() {
            anyhow::bail!("Received empty response from server");
        }

        let response_message: MenuResponseMessage =
            serde_json::from_str(response_json.trim()).context("Failed to deserialize response")?;

        if response_message.request_id != message.request_id {
            anyhow::bail!("Request ID mismatch in response");
        }

        Ok(response_message.payload)
    }

    fn send_request_via_fallback(&self, request: MenuRequest) -> Result<MenuResponse> {
        match request {
            MenuRequest::Show => Ok(MenuResponse::ShowResult),
            MenuRequest::Status => Ok(MenuResponse::StatusResult(self.fallback_status_info())),
            MenuRequest::Stop => Ok(MenuResponse::Error(
                "Menu server is not running in fallback mode".to_string(),
            )),
            _ => self.invoke_kitty_worker(request),
        }
    }

    fn fallback_status_info(&self) -> StatusInfo {
        let compositor_name = CompositorType::detect().name();

        StatusInfo {
            status: ServerStatus::Ready,
            version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_version: PROTOCOL_VERSION.to_string(),
            uptime_seconds: 0,
            socket_path: "N/A (fallback)".to_string(),
            requests_processed: 0,
            start_time: "N/A (fallback)".to_string(),
            compositor: format!("Fallback ({compositor_name})"),
        }
    }

    fn invoke_kitty_worker(&self, request: MenuRequest) -> Result<MenuResponse> {
        let current_exe = std::env::current_exe()
            .context("Failed to determine current executable for menu fallback")?;

        let temp_dir = tempdir().context("Failed to create fallback menu temp directory")?;
        let request_path = temp_dir.path().join("request.json");
        let response_path = temp_dir.path().join("response.json");

        let request_json =
            serde_json::to_string(&request).context("Failed to serialize fallback menu request")?;
        fs::write(&request_path, request_json)
            .context("Failed to write fallback menu request file")?;

        let args = vec![
            "menu".to_string(),
            "fallback-worker".to_string(),
            "--request-file".to_string(),
            request_path.to_string_lossy().to_string(),
            "--response-file".to_string(),
            response_path.to_string_lossy().to_string(),
        ];

        let status =
            crate::common::terminal::TerminalLauncher::new(current_exe.to_string_lossy().as_ref())
                .class("insmenu-fallback")
                .title("InstantCLI Menu")
                .args(&args)
                .launch_and_wait()?;

        if !status.success() {
            anyhow::bail!("Fallback menu terminal exited with status {status}");
        }

        let response_json = fs::read_to_string(&response_path)
            .context("Fallback menu did not produce a response")?;

        serde_json::from_str(&response_json).context("Failed to deserialize fallback menu response")
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

    /// Show password dialog via server
    pub fn password(&self, prompt: String) -> Result<String> {
        match self.send_request(MenuRequest::Password { prompt })? {
            MenuResponse::PasswordResult(text) => Ok(text),
            MenuResponse::Error(error) => anyhow::bail!("Server error: {}", error),
            MenuResponse::Cancelled => Ok(String::new()),
            _ => anyhow::bail!("Unexpected response type for password request"),
        }
    }

    /// Launch file picker via server
    pub fn file_picker(
        &self,
        start: Option<String>,
        scope: FilePickerScope,
        multi: bool,
    ) -> Result<Vec<PathBuf>> {
        match self.send_request(MenuRequest::FilePicker {
            start,
            scope,
            multi,
        })? {
            MenuResponse::FilePickerResult(paths) => Ok(paths),
            MenuResponse::Error(error) => anyhow::bail!("Server error: {}", error),
            MenuResponse::Cancelled => Ok(Vec::new()),
            _ => anyhow::bail!("Unexpected response type for file picker request"),
        }
    }

    /// Show chord navigator via server
    pub fn chord(&self, chords: Vec<String>) -> Result<Option<String>> {
        if chords.is_empty() {
            anyhow::bail!("Chord request must include at least one chord");
        }

        match self.send_request(MenuRequest::Chord { chords })? {
            MenuResponse::ChordResult(sequence) => Ok(Some(sequence)),
            MenuResponse::Cancelled => Ok(None),
            MenuResponse::Error(error) => anyhow::bail!("Server error: {}", error),
            _ => anyhow::bail!("Unexpected response type for chord request"),
        }
    }

    /// Show slider dialog via server
    pub fn slide(&self, request: SliderRequest) -> Result<Option<i64>> {
        match self.send_request(MenuRequest::Slide(request))? {
            MenuResponse::SlideResult(value) => Ok(Some(value)),
            MenuResponse::Cancelled => Ok(None),
            MenuResponse::Error(error) => anyhow::bail!("Server error: {}", error),
            _ => anyhow::bail!("Unexpected response type for slide request"),
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
        if self.transport != MenuTransport::ScratchpadServer {
            anyhow::bail!("Menu server is not active in fallback mode");
        }

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

/// Force all future menu clients to run in fallback mode.
pub fn force_fallback_mode() {
    if let Ok(mut guard) = transport_override().write() {
        *guard = Some(MenuTransport::KittyTransient);
    }
}

/// Clear any forced transport mode override.
pub fn reset_forced_transport() {
    if let Ok(mut guard) = transport_override().write() {
        *guard = None;
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
        MenuCommands::Password { prompt, gui: true } => {
            match client.password(prompt.clone()) {
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
        MenuCommands::Pick {
            start,
            dirs,
            files,
            multi,
            gui: true,
        } => {
            let scope = match (*dirs, *files) {
                (true, false) => FilePickerScope::Directories,
                (false, true) => FilePickerScope::Files,
                (true, true) => FilePickerScope::FilesAndDirectories,
                (false, false) => FilePickerScope::Files,
            };

            match client.file_picker(start.clone(), scope, *multi) {
                Ok(paths) => {
                    if paths.is_empty() {
                        Ok(1)
                    } else {
                        for path in paths {
                            println!("{}", path.display());
                        }
                        Ok(0)
                    }
                }
                Err(e) => {
                    eprintln!("GUI menu error: {e}");
                    Ok(3)
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

    #[test]
    fn test_fallback_status_info() {
        force_fallback_mode();

        let client = MenuClient::new();
        assert!(client.is_fallback());

        let status = client.status().expect("fallback status should succeed");
        assert_eq!(status.socket_path, "N/A (fallback)");

        reset_forced_transport();
    }
}
