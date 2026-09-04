use super::protocol::*;
use crate::common::compositor::CompositorType;
use crate::menu_utils::DialogOutcome;
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

const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(30);
const STREAM_CHUNK_MAX_ITEMS: usize = 64;

fn decode_dialog_response<T>(
    response: MenuResponse,
    operation: &str,
    value: impl FnOnce(MenuResponse) -> Option<T>,
) -> Result<DialogOutcome<T>> {
    match response {
        MenuResponse::Cancelled => Ok(DialogOutcome::Cancelled),
        MenuResponse::Error(error) => anyhow::bail!("Server error: {error}"),
        response => value(response)
            .map(DialogOutcome::Submitted)
            .ok_or_else(|| anyhow::anyhow!("Unexpected response type for {operation} request")),
    }
}

fn read_timeout_for_request(request: &MenuRequest) -> Duration {
    let MenuRequest::Toast { duration, .. } = request else {
        return DEFAULT_READ_TIMEOUT;
    };
    if !duration.is_finite() || *duration <= 25.0 {
        return DEFAULT_READ_TIMEOUT;
    }

    Duration::from_secs((duration.ceil() as u64).saturating_add(5))
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

/// Client for rendering dialogs outside the caller's current terminal.
///
/// Requests use the scratchpad server when supported and otherwise run in a
/// transient terminal. Typed request methods prepare the host automatically;
/// call [`Self::prepare`] only to warm it up ahead of the first request.
#[derive(Clone)]
pub struct HostedMenuClient {
    socket_path: String,
    transport: MenuTransport,
}

impl HostedMenuClient {
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
        stream.set_read_timeout(Some(DEFAULT_READ_TIMEOUT))?;
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
    pub fn prepare(&self) -> Result<()> {
        if self.transport != MenuTransport::ScratchpadServer {
            return Ok(());
        }

        if let Ok(stream) = self.connect() {
            let status = self.status_from_stream(stream)?;
            if status.protocol_version == PROTOCOL_VERSION {
                return Ok(());
            }

            self.stop_connected_server().with_context(|| {
                format!(
                    "Failed to stop menu server using protocol {} before upgrading to {}",
                    status.protocol_version, PROTOCOL_VERSION
                )
            })?;

            let stop_deadline = std::time::Instant::now() + Duration::from_secs(5);
            while self.is_server_running() {
                if std::time::Instant::now() >= stop_deadline {
                    anyhow::bail!(
                        "Menu server using protocol {} did not stop",
                        status.protocol_version
                    );
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }

        self.launch_server_in_scratchpad()?;

        // Poll for the server to become available. The scratchpad creation
        // (terminal spawn + window manager registration) can take several
        // seconds on some compositors (e.g. instantwm), and only after that
        // does the inner server process start and bind the socket.
        let poll_interval = Duration::from_millis(200);
        let max_wait = Duration::from_secs(10);
        let start = std::time::Instant::now();

        while start.elapsed() < max_wait {
            std::thread::sleep(poll_interval);
            if let Ok(status) = self.status_from_connected_server()
                && status.protocol_version == PROTOCOL_VERSION
            {
                return Ok(());
            }
        }

        anyhow::bail!("Server failed to start after spawning in scratchpad");
    }

    fn launch_server_in_scratchpad(&self) -> Result<()> {
        let current_exe =
            std::env::current_exe().context("Failed to get current executable path")?;
        let output = Command::new(current_exe)
            .args(["menu", "server", "launch"])
            .output()
            .context("Failed to spawn menu server in scratchpad")?;
        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to spawn menu server in scratchpad: {error_msg}");
        }
        Ok(())
    }

    /// Connect to the streaming server without a separate status probe.
    ///
    /// A running server receives `ChoiceBegin` on this first connection. If
    /// no server is listening, launch it and return the first connection that
    /// succeeds. The latency-sensitive streaming path deliberately sends the
    /// application request before doing any compatibility work.
    fn connect_or_start_streaming_server(&self) -> Result<UnixStream> {
        if let Ok(stream) = self.connect() {
            return Ok(stream);
        }

        self.launch_server_in_scratchpad()?;

        self.wait_for_streaming_server()
    }

    fn wait_for_streaming_server(&self) -> Result<UnixStream> {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(stream) = self.connect() {
                return Ok(stream);
            }
            if std::time::Instant::now() >= deadline {
                anyhow::bail!("Server failed to start after spawning in scratchpad");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn restart_server_for_streaming(&self) -> Result<UnixStream> {
        if let Err(error) = self.stop_connected_server()
            && self.is_server_running()
        {
            return Err(error).context("Failed to stop incompatible menu server");
        }

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while self.is_server_running() {
            if std::time::Instant::now() >= deadline {
                anyhow::bail!("Incompatible menu server did not stop");
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        self.launch_server_in_scratchpad()?;
        self.wait_for_streaming_server()
    }

    fn open_streaming_choice(
        &self,
        prompt: String,
        allow_multiple: bool,
    ) -> Result<(UnixStream, io::BufReader<UnixStream>, String)> {
        let mut restarted = false;
        let mut prepared_stream = None;

        loop {
            let request_id = generate_request_id();
            // This is intentionally the first successful server connection:
            // send ChoiceBegin immediately instead of paying for Status.
            let write_stream = match prepared_stream.take() {
                Some(stream) => stream,
                None => self.connect_or_start_streaming_server()?,
            };
            write_stream.set_write_timeout(None)?;
            let read_stream = write_stream
                .try_clone()
                .context("Failed to clone menu socket for streaming response")?;

            let begin = MenuMessage::new(
                request_id.clone(),
                MenuRequest::ChoiceBegin {
                    prompt: prompt.clone(),
                    allow_multiple,
                },
            );
            write_menu_message(&write_stream, &begin)?;

            let mut response_reader = io::BufReader::new(read_stream);
            let mut response_json = String::new();
            response_reader.read_line(&mut response_json)?;
            let incompatible = response_json.is_empty();
            let response = if incompatible {
                None
            } else {
                Some(
                    serde_json::from_str::<MenuResponseMessage>(response_json.trim())
                        .context("Failed to deserialize streaming readiness response")?,
                )
            };

            if let Some(response) = response {
                if response.request_id != request_id {
                    anyhow::bail!("Request ID mismatch in streaming readiness response");
                }
                match response.payload {
                    MenuResponse::ChoiceReady => {
                        response_reader.get_ref().set_read_timeout(None)?;
                        return Ok((write_stream, response_reader, request_id));
                    }
                    MenuResponse::ProtocolMismatch { .. } => {}
                    MenuResponse::Error(error) => anyhow::bail!("Server error: {error}"),
                    _ => anyhow::bail!("Unexpected streaming readiness response"),
                }
            }

            if restarted {
                anyhow::bail!("Menu server rejected streaming protocol after restart");
            }
            restarted = true;
            prepared_stream = Some(self.restart_server_for_streaming()?);
        }
    }

    /// Send a request and receive response
    fn send_request(&self, request: MenuRequest) -> Result<MenuResponse> {
        match self.transport {
            MenuTransport::ScratchpadServer => self.send_request_via_server(request),
            MenuTransport::KittyTransient => self.send_request_via_fallback(request),
        }
    }

    fn send_request_via_server(&self, request: MenuRequest) -> Result<MenuResponse> {
        if matches!(
            request,
            MenuRequest::ChoiceBegin { .. }
                | MenuRequest::ChoiceChunk { .. }
                | MenuRequest::ChoiceEnd
        ) {
            anyhow::bail!("Streaming choice frames require a streaming connection");
        }
        self.prepare()?;

        self.send_request_to_connected_server(request)
    }

    fn send_request_to_connected_server(&self, request: MenuRequest) -> Result<MenuResponse> {
        let stream = self.connect()?;
        self.send_request_on_stream(stream, request)
    }

    fn send_request_on_stream(
        &self,
        mut stream: UnixStream,
        request: MenuRequest,
    ) -> Result<MenuResponse> {
        stream.set_read_timeout(Some(read_timeout_for_request(&request)))?;

        let message = MenuMessage::new(generate_request_id(), request);

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

    fn status_from_connected_server(&self) -> Result<MenuStatus> {
        self.status_from_stream(self.connect()?)
    }

    fn status_from_stream(&self, stream: UnixStream) -> Result<MenuStatus> {
        match self.send_request_on_stream(stream, MenuRequest::Status)? {
            MenuResponse::StatusResult(status) => Ok(status),
            MenuResponse::Error(error) => anyhow::bail!("Server error: {error}"),
            _ => anyhow::bail!("Unexpected response type for status request"),
        }
    }

    fn stop_connected_server(&self) -> Result<()> {
        match self.send_request_to_connected_server(MenuRequest::Stop)? {
            MenuResponse::StopResult => Ok(()),
            MenuResponse::Error(error) => anyhow::bail!("Server error: {error}"),
            _ => anyhow::bail!("Unexpected response type for stop request"),
        }
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

    fn fallback_status_info(&self) -> MenuStatus {
        let compositor_name = CompositorType::detect().name();

        MenuStatus {
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

    /// Show message dialog via server
    pub fn message(&self, title: String, message: String) -> Result<()> {
        match self.send_request(MenuRequest::Message { title, message })? {
            MenuResponse::MessageResult => Ok(()),
            MenuResponse::Error(error) => anyhow::bail!("Server error: {}", error),
            MenuResponse::Cancelled => Ok(()),
            _ => anyhow::bail!("Unexpected response type for message request"),
        }
    }

    /// Show choice dialog via server
    pub fn choice(
        &self,
        prompt: String,
        items: Vec<SerializableMenuItem>,
        allow_multiple: bool,
    ) -> Result<DialogOutcome<Vec<SerializableMenuItem>>> {
        let response = self.send_request(MenuRequest::Choice {
            prompt,
            items,
            allow_multiple,
        })?;
        decode_dialog_response(response, "choice", |response| match response {
            MenuResponse::ChoiceResult(selected) => Some(selected),
            _ => None,
        })
    }

    /// Show streaming choice dialog, reading plain items line-by-line from
    /// stdin. The menu opens immediately (no pre-buffering); each stdin
    /// line becomes an item as it arrives. Used by `ins menu choice` when
    /// items come from a pipe.
    ///
    /// Wire protocol (one connection, NDJSON): `ChoiceBegin` with a fresh
    /// `request_id`; `ChoiceReady` after the renderer starts; then
    /// latency-preserving batches of `ChoiceChunk` items and `ChoiceEnd`.
    /// stdin is not consumed until `ChoiceReady`, making a one-time daemon
    /// restart safe on protocol mismatch. The sender runs on a background
    /// thread with blocking writes (backpressure via socket buffers); the
    /// caller waits for the final response with no timeout (user selection
    /// time is unbounded). Early server exit
    /// (user selects before `EOF`) surfaces as `EPIPE` in the sender,
    /// which stops quietly — the response is still read normally.
    /// The sender may stay blocked on stdin for infinite producers;
    /// the short-lived CLI process exiting kills it, so the handle is
    /// intentionally detached, not joined.
    pub fn choice_from_stdin_streaming(
        &self,
        prompt: String,
        allow_multiple: bool,
    ) -> Result<DialogOutcome<Vec<SerializableMenuItem>>> {
        if self.transport != MenuTransport::ScratchpadServer {
            let mut buffer = String::new();
            io::stdin()
                .read_to_string(&mut buffer)
                .map_err(|e| anyhow::anyhow!("Failed to read from stdin: {}", e))?;
            let items = plain_choice_items_from_input(&buffer);
            return self.choice(prompt, items, allow_multiple);
        }

        let (write_stream, mut response_reader, request_id) =
            self.open_streaming_choice(prompt, allow_multiple)?;

        let stream_request_id = request_id.clone();
        std::thread::spawn(move || {
            let mut writer = io::BufWriter::new(write_stream);
            let stdin = io::stdin();
            let mut reader = io::BufReader::new(stdin.lock());
            loop {
                let items = match read_plain_choice_chunk(&mut reader) {
                    Ok(Some(items)) => items,
                    Ok(None) => break,
                    // Dropping the write half without ChoiceEnd tells the
                    // server this was an aborted producer, not clean EOF.
                    Err(_) => return,
                };

                let chunk = MenuMessage::new(
                    stream_request_id.clone(),
                    MenuRequest::ChoiceChunk { items },
                );
                if write_menu_message_buffered(&mut writer, &chunk).is_err() {
                    return;
                }
            }
            let end = MenuMessage::new(stream_request_id, MenuRequest::ChoiceEnd);
            let _ = write_menu_message_buffered(&mut writer, &end);
        });

        let mut response_json = String::new();
        response_reader.read_line(&mut response_json)?;
        if response_json.is_empty() {
            anyhow::bail!("Received empty response from server");
        }
        let response_message: MenuResponseMessage = serde_json::from_str(response_json.trim())
            .context("Failed to deserialize streaming choice response")?;
        if response_message.request_id != request_id {
            anyhow::bail!("Request ID mismatch in streaming choice response");
        }
        decode_dialog_response(response_message.payload, "streaming choice", |response| {
            match response {
                MenuResponse::ChoiceResult(selected) => Some(selected),
                _ => None,
            }
        })
    }

    /// Show input dialog via server
    pub fn input(&self, prompt: String) -> Result<DialogOutcome<String>> {
        let response = self.send_request(MenuRequest::Input { prompt })?;
        decode_dialog_response(response, "input", |response| match response {
            MenuResponse::InputResult(text) => Some(text),
            _ => None,
        })
    }

    /// Show password dialog via server
    pub fn password(&self, prompt: String) -> Result<DialogOutcome<String>> {
        let response = self.send_request(MenuRequest::Password { prompt })?;
        decode_dialog_response(response, "password", |response| match response {
            MenuResponse::PasswordResult(text) => Some(text),
            _ => None,
        })
    }

    /// Launch file picker via server
    pub fn file_picker(
        &self,
        start: Option<String>,
        scope: FilePickerScope,
        allow_multiple: bool,
    ) -> Result<DialogOutcome<Vec<PathBuf>>> {
        let response = self.send_request(MenuRequest::FilePicker {
            start,
            scope,
            allow_multiple,
        })?;
        decode_dialog_response(response, "file picker", |response| match response {
            MenuResponse::FilePickerResult(paths) => Some(paths),
            _ => None,
        })
    }

    /// Show chord navigator via server
    pub fn chord(&self, chords: Vec<String>) -> Result<DialogOutcome<String>> {
        if chords.is_empty() {
            anyhow::bail!("Chord request must include at least one chord");
        }

        let response = self.send_request(MenuRequest::Chord { chords })?;
        decode_dialog_response(response, "chord", |response| match response {
            MenuResponse::ChordResult(sequence) => Some(sequence),
            _ => None,
        })
    }

    /// Show slider dialog via server
    pub fn slide(&self, request: SliderRequest) -> Result<DialogOutcome<i64>> {
        let response = self.send_request(MenuRequest::Slide(request))?;
        decode_dialog_response(response, "slide", |response| match response {
            MenuResponse::SlideResult(value) => Some(value),
            _ => None,
        })
    }

    /// Show the scratchpad without any other action
    pub fn show(&self) -> Result<()> {
        match self.send_request(MenuRequest::Show)? {
            MenuResponse::ShowResult => Ok(()),
            MenuResponse::Error(error) => anyhow::bail!("Server error: {}", error),
            _ => anyhow::bail!("Unexpected response type for show request"),
        }
    }

    /// Show toast notification popup via server
    pub fn toast(&self, message: String, duration: f64) -> Result<()> {
        match self.send_request(MenuRequest::Toast { message, duration })? {
            MenuResponse::ToastResult | MenuResponse::MessageResult => Ok(()),
            MenuResponse::Error(error) => anyhow::bail!("Server error: {}", error),
            _ => anyhow::bail!("Unexpected response type for toast request"),
        }
    }

    /// Get server status information
    pub fn status(&self) -> Result<MenuStatus> {
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

        self.stop_connected_server()
    }
}

impl Default for HostedMenuClient {
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

/// Write one NDJSON `MenuMessage` frame to a menu socket.
fn write_menu_message(stream: &UnixStream, message: &MenuMessage) -> Result<()> {
    let json = serde_json::to_string(message).context("Failed to serialize menu message")?;
    let mut writer = io::BufWriter::new(stream);
    writer.write_all(json.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

/// Write one NDJSON frame via an existing buffered writer (flushes per
/// frame so chunks arrive with minimal latency).
fn write_menu_message_buffered<W: io::Write>(
    writer: &mut io::BufWriter<W>,
    message: &MenuMessage,
) -> Result<()> {
    serde_json::to_writer(&mut *writer, message).context("Failed to serialize menu message")?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

/// Read at least one choice line, then collect only additional complete lines
/// already held by `BufReader`. It never waits to fill a batch, preserving the
/// latency of sparse producers while amortizing framing for bursty ones.
fn read_plain_choice_chunk<R: io::Read>(
    reader: &mut io::BufReader<R>,
) -> io::Result<Option<Vec<SerializableMenuItem>>> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(None);
    }

    let mut items = vec![SerializableMenuItem::plain(
        line.trim_end_matches(['\r', '\n']),
    )];
    while items.len() < STREAM_CHUNK_MAX_ITEMS && reader.buffer().contains(&b'\n') {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        items.push(SerializableMenuItem::plain(
            line.trim_end_matches(['\r', '\n']),
        ));
    }

    Ok(Some(items))
}

/// Print formatted status information
pub fn print_status_info(status: &MenuStatus) {
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
    use std::os::unix::net::UnixListener;

    #[test]
    fn test_client_creation() {
        let client = HostedMenuClient::new();
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
    fn dialog_response_distinguishes_empty_submission_from_cancellation() {
        let submitted = decode_dialog_response(
            MenuResponse::InputResult(String::new()),
            "input",
            |response| match response {
                MenuResponse::InputResult(value) => Some(value),
                _ => None,
            },
        )
        .unwrap();
        let cancelled =
            decode_dialog_response(MenuResponse::Cancelled, "input", |_| None::<String>).unwrap();

        assert_eq!(submitted, DialogOutcome::Submitted(String::new()));
        assert_eq!(cancelled, DialogOutcome::Cancelled);
    }

    #[test]
    fn dialog_response_preserves_server_errors() {
        let error =
            decode_dialog_response(MenuResponse::Error("boom".to_string()), "input", |_| {
                None::<String>
            })
            .unwrap_err();

        assert_eq!(error.to_string(), "Server error: boom");
    }

    #[test]
    fn long_toast_extends_the_server_read_timeout() {
        let request = MenuRequest::Toast {
            message: "Still here".to_string(),
            duration: 60.0,
        };

        assert_eq!(read_timeout_for_request(&request), Duration::from_secs(65));
        assert_eq!(
            read_timeout_for_request(&MenuRequest::Status),
            DEFAULT_READ_TIMEOUT
        );
    }

    #[test]
    fn streaming_chunks_batch_buffered_complete_lines() {
        let input = "first\nsecond\r\nthird-without-newline";
        let mut reader = io::BufReader::new(input.as_bytes());

        let first = read_plain_choice_chunk(&mut reader).unwrap().unwrap();
        assert_eq!(
            first
                .iter()
                .map(|item| item.display_text.as_str())
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
        let second = read_plain_choice_chunk(&mut reader).unwrap().unwrap();
        assert_eq!(second[0].display_text, "third-without-newline");
        assert!(read_plain_choice_chunk(&mut reader).unwrap().is_none());
    }

    #[test]
    fn streaming_choice_sends_begin_on_first_connection() {
        let socket_dir = tempdir().unwrap();
        let socket_path = socket_dir.path().join("menu.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();

        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let reader_stream = stream.try_clone().unwrap();
            let mut request_json = String::new();
            io::BufReader::new(reader_stream)
                .read_line(&mut request_json)
                .unwrap();
            let request: MenuMessage = serde_json::from_str(request_json.trim()).unwrap();
            let response = MenuResponseMessage {
                request_id: request.request_id.clone(),
                payload: MenuResponse::ChoiceReady,
                timestamp: std::time::SystemTime::now(),
            };
            let completed = MenuResponseMessage {
                request_id: request.request_id.clone(),
                payload: MenuResponse::Cancelled,
                timestamp: std::time::SystemTime::now(),
            };
            let wire = format!(
                "{}\n{}\n",
                serde_json::to_string(&response).unwrap(),
                serde_json::to_string(&completed).unwrap()
            );
            stream.write_all(wire.as_bytes()).unwrap();
            request.payload
        });

        let client = HostedMenuClient {
            socket_path: socket_path.to_string_lossy().into_owned(),
            transport: MenuTransport::ScratchpadServer,
        };
        let (write_stream, mut response_reader, request_id) = client
            .open_streaming_choice("Pick".to_string(), false)
            .unwrap();
        drop(write_stream);
        let mut completed = String::new();
        response_reader.read_line(&mut completed).unwrap();
        let completed: MenuResponseMessage = serde_json::from_str(completed.trim()).unwrap();
        assert_eq!(completed.request_id, request_id);
        assert!(matches!(completed.payload, MenuResponse::Cancelled));
        assert!(matches!(
            server.join().unwrap(),
            MenuRequest::ChoiceBegin { .. }
        ));
    }
}
