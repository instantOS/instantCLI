use super::processing::RequestProcessor;
use super::protocol::*;
use super::scratchpad_manager::ScratchpadManager;
use super::tui::MenuServerTui;
use crate::common::compositor::CompositorType;
use crate::scratchpad::config::ScratchpadConfig;
use anyhow::{Context, Result};
use std::io::{self, BufRead, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
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

/// Read one newline-delimited protocol frame without replacing the caller's
/// buffer. Returning `None` means the peer closed its write side cleanly.
fn read_menu_message<R: BufRead>(reader: &mut R) -> Result<Option<MenuMessage>> {
    let mut json = String::new();
    if reader.read_line(&mut json)? == 0 {
        return Ok(None);
    }

    let message = serde_json::from_str(json.trim_end())
        .context("Failed to deserialize menu protocol frame")?;
    Ok(Some(message))
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

        let listener = tokio::net::UnixListener::bind(&self.socket_path)
            .context(format!("Failed to bind to socket at {}", self.socket_path))?;

        self.running.store(true, Ordering::SeqCst);

        let mut sigint = signal::unix::signal(signal::unix::SignalKind::interrupt())
            .context("Failed to setup SIGINT handler")?;
        let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
            .context("Failed to setup SIGTERM handler")?;

        // Initial draw of the status screen
        if let Some(ref mut tui) = self.tui {
            let has_scratchpad = self.scratchpad_manager.is_some();
            let requests_processed = self.requests_processed.load(Ordering::SeqCst);
            tui.draw_status_screen(has_scratchpad, requests_processed, self.start_time)?;
        }

        // Tick once per second to refresh the uptime display.
        // Skip missed ticks so we don't burn a backlog after long-running connections.
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // Consume the immediate first tick — we just drew above.
        tick.tick().await;

        let mut last_requests = self.requests_processed.load(Ordering::SeqCst);
        let mut last_uptime = self.start_time.elapsed().unwrap_or_default().as_secs();

        loop {
            tokio::select! {
                biased;
                _ = sigint.recv() => break,
                _ = sigterm.recv() => break,
                accept_res = listener.accept() => {
                    match accept_res {
                        Ok((stream, _addr)) => {
                            // Convert to a blocking std stream so the existing
                            // synchronous handler keeps working unchanged.
                            let std_stream = match stream.into_std() {
                                Ok(s) => s,
                                Err(e) => {
                                    eprintln!("Failed to convert accepted stream: {e}");
                                    continue;
                                }
                            };
                            std_stream.set_nonblocking(false)?;

                            if let Some(ref mut tui) = self.tui {
                                tui.suspend()?;
                            }

                            let _ = self.handle_connection_sync(std_stream);

                            if let Some(ref mut tui) = self.tui {
                                tui.resume()?;
                                let has_scratchpad = self.scratchpad_manager.is_some();
                                last_requests = self.requests_processed.load(Ordering::SeqCst);
                                last_uptime = self.start_time.elapsed().unwrap_or_default().as_secs();
                                tui.draw_status_screen(has_scratchpad, last_requests, self.start_time)?;
                            }

                            // A Stop request can flip `running` from inside the handler.
                            if !self.running.load(Ordering::SeqCst) {
                                break;
                            }
                        }
                        Err(e) => {
                            eprintln!("accept error: {e}");
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                    }
                }
                _ = tick.tick() => {
                    // Only redraw when something actually changed.
                    let current_requests = self.requests_processed.load(Ordering::SeqCst);
                    let current_uptime = self.start_time.elapsed().unwrap_or_default().as_secs();
                    if current_uptime != last_uptime || current_requests != last_requests {
                        if let Some(ref mut tui) = self.tui {
                            let has_scratchpad = self.scratchpad_manager.is_some();
                            tui.draw_status_screen(has_scratchpad, current_requests, self.start_time)?;
                        }
                        last_uptime = current_uptime;
                        last_requests = current_requests;
                    }
                }
            }
        }

        self.running.store(false, Ordering::SeqCst);

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

    /// Handle a client connection synchronously.
    ///
    /// Single-frame requests use the historical one-line-in/one-line-out
    /// flow. `ChoiceBegin` switches the connection into streaming mode:
    /// the server opens the menu immediately and consumes `ChoiceChunk`
    /// / `ChoiceEnd` frames on the same socket while fzf runs (see
    /// `handle_choice_stream_connection`).
    fn handle_connection_sync(&self, mut stream: UnixStream) -> Result<()> {
        // Increment request counter for debugging
        self.requests_processed.fetch_add(1, Ordering::SeqCst);

        // Set read timeout
        stream.set_read_timeout(Some(Duration::from_secs(30)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;

        // Use one buffered reader for the entire connection. Replacing a
        // BufReader after the first frame can discard later frames that were
        // read into its internal buffer at the same time as the first one.
        let reader_stream = stream
            .try_clone()
            .context("Failed to clone menu socket for request reader")?;
        let mut reader = io::BufReader::new(reader_stream);
        let Some(message) = read_menu_message(&mut reader)? else {
            // Client disconnected - this is normal, not an error
            return Ok(());
        };

        // Status and Stop form a deliberately stable control plane so a new
        // client can identify and replace an older daemon. All application
        // requests require an exact protocol match.
        let is_version_control_request =
            matches!(&message.payload, MenuRequest::Status | MenuRequest::Stop);
        if message.protocol_version != PROTOCOL_VERSION && !is_version_control_request {
            let response = MenuResponse::ProtocolMismatch {
                received: message.protocol_version,
                expected: PROTOCOL_VERSION.to_string(),
            };
            return Self::write_response(&mut stream, message.request_id, response);
        }

        if let MenuRequest::ChoiceBegin {
            prompt,
            allow_multiple,
        } = message.payload
        {
            return self.handle_choice_stream_connection(
                stream,
                reader,
                message.request_id,
                prompt,
                allow_multiple,
            );
        }
        if matches!(
            message.payload,
            MenuRequest::ChoiceChunk { .. } | MenuRequest::ChoiceEnd
        ) {
            let response = MenuResponse::Error(
                "Streaming choice frames require an opening ChoiceBegin".to_string(),
            );
            return Self::write_response(&mut stream, message.request_id, response);
        }

        // Process request and generate response (synchronously for now)
        let response = self.process_request_sync(message.payload)?;

        Self::write_response(&mut stream, message.request_id, response)
    }

    fn write_response(
        stream: &mut UnixStream,
        request_id: String,
        response: MenuResponse,
    ) -> Result<()> {
        // Create response envelope
        let response_message = MenuResponseMessage {
            request_id,
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

    /// Streaming choice connection: menu opens on `ChoiceBegin`, chunks
    /// are pumped into fzf live, response is sent after selection.
    ///
    /// Concurrency: a reader thread owns the connection's buffered reader
    /// and forwards `ChoiceChunk` items into a bounded mpsc channel consumed by
    /// `select_streaming`; the main thread runs fzf under the standard
    /// visibility monitor. After fzf exits, the main thread shuts down the
    /// connection's read side and joins the reader before returning to the
    /// accept loop. This keeps reader ownership scoped to one request even
    /// for infinite producers (`tail -f | ins menu choice`). Unexpected EOF
    /// before selection or `ChoiceEnd` kills fzf to avoid a ghost scratchpad
    /// nobody will read a response from.
    fn handle_choice_stream_connection(
        &self,
        mut stream: UnixStream,
        mut reader: io::BufReader<UnixStream>,
        request_id: String,
        prompt: String,
        allow_multiple: bool,
    ) -> Result<()> {
        if let Some(ref manager) = self.scratchpad_manager
            && let Err(e) = manager.show()
        {
            eprintln!("Warning: Failed to show scratchpad: {e}");
        }

        let (tx, rx) =
            crossbeam_channel::bounded::<SerializableMenuItem>(STREAM_ITEM_BUFFER_CAPACITY);
        // Selection time and producer idle time are both intentionally
        // unbounded. The initial frame still uses the normal request timeout.
        reader.get_ref().set_read_timeout(None)?;
        let expected_id = request_id.clone();

        // The reader is request-owned even though it must run concurrently
        // with fzf. Once selection finishes we shut down the socket's read
        // side and join this thread before returning to the accept loop. This
        // prevents a late EOF from an old connection cancelling a later menu.
        let selection_finished = Arc::new(AtomicBool::new(false));
        let reader_selection_finished = Arc::clone(&selection_finished);
        let reader_handle = thread::spawn(move || {
            let mut saw_end = false;
            loop {
                let frame = match read_menu_message(&mut reader) {
                    Ok(Some(frame)) => frame,
                    Ok(None) | Err(_) => break,
                };
                if frame.request_id != expected_id || frame.protocol_version != PROTOCOL_VERSION {
                    break;
                }
                match frame.payload {
                    MenuRequest::ChoiceChunk { items } => {
                        let mut closed = false;
                        for item in items {
                            if tx.send(item).is_err() {
                                closed = true;
                                break;
                            }
                        }
                        if closed {
                            break;
                        }
                    }
                    MenuRequest::ChoiceEnd => {
                        saw_end = true;
                        break;
                    }
                    _ => break,
                }
            }
            if !saw_end && !reader_selection_finished.load(Ordering::SeqCst) {
                let _ = kill_active_menu_processes();
            }
        });

        let response = if self.scratchpad_manager.is_some() {
            self.process_monitored_streaming_choice(prompt, allow_multiple, rx, || {
                Self::write_response(&mut stream, request_id.clone(), MenuResponse::ChoiceReady)
            })?
        } else {
            let processor =
                RequestProcessor::new(self.running.clone(), self.requests_processed.clone());
            processor.handle_choice_streaming_with_ready(prompt, allow_multiple, rx, || {
                Self::write_response(&mut stream, request_id.clone(), MenuResponse::ChoiceReady)
            })?
        };

        // Mark completion before waking the reader so EOF caused by our own
        // shutdown cannot be mistaken for an aborted producer. SHUT_RD leaves
        // the write side available for the response below.
        selection_finished.store(true, Ordering::SeqCst);
        let _ = stream.shutdown(Shutdown::Read);
        reader_handle
            .join()
            .map_err(|_| anyhow::anyhow!("Streaming choice reader thread panicked"))?;

        if let Some(ref manager) = self.scratchpad_manager
            && let Err(e) = manager.hide_fast()
        {
            eprintln!("Warning: Failed to hide scratchpad: {e}");
        }

        Self::write_response(&mut stream, request_id, response)
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

        // Show scratchpad if configured (for interactive requests only).
        // `ChoiceBegin` is listed for completeness — streaming choices
        // normally bypass this via `handle_choice_stream_connection`,
        // which manages visibility itself. Bare `ChoiceChunk`/`ChoiceEnd`
        // are protocol errors and must not pop the scratchpad.
        let should_manage_scratchpad = matches!(
            request,
            MenuRequest::Confirm { .. }
                | MenuRequest::Choice { .. }
                | MenuRequest::ChoiceBegin { .. }
                | MenuRequest::Chord { .. }
                | MenuRequest::Input { .. }
                | MenuRequest::Password { .. }
                | MenuRequest::FilePicker { .. }
                | MenuRequest::Slide(_)
                | MenuRequest::Message { .. }
                | MenuRequest::Toast { .. }
        );

        if should_manage_scratchpad
            && let Some(ref manager) = self.scratchpad_manager
            && let Err(e) = manager.show()
        {
            eprintln!("Warning: Failed to show scratchpad: {e}");
        }

        // Process the request with timeout and visibility monitoring
        let response = if should_manage_scratchpad && self.scratchpad_manager.is_some() {
            self.process_monitored_request(request)?
        } else {
            // Non-interactive requests or no scratchpad don't need monitoring
            self.process_request_internal(request)?
        };

        // **PERFORMANCE CRITICAL**: Hide scratchpad IMMEDIATELY after menu interaction
        // This must be the FIRST thing we do after the user completes their interaction
        // to return control to the user as fast as possible.
        //
        // NOTE: For monitored requests, monitoring is already stopped in process_monitored_request
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

    /// Start the scratchpad visibility monitor. Returns the flags and the
    /// thread handle; stop it with `finish_visibility_monitor` BEFORE
    /// hiding the scratchpad to avoid false cancellations.
    fn start_visibility_monitor(
        &self,
    ) -> (
        Arc<AtomicBool>,
        Arc<AtomicBool>,
        Option<thread::JoinHandle<()>>,
    ) {
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

        (monitoring_active, was_killed, monitoring_handle)
    }

    /// Stop the monitor started by `start_visibility_monitor`. Returns true
    /// when the menu was killed due to scratchpad invisibility, in which
    /// case callers must report `Cancelled` regardless of fzf's result.
    fn finish_visibility_monitor(
        monitoring_active: &Arc<AtomicBool>,
        was_killed: &Arc<AtomicBool>,
        handle: Option<thread::JoinHandle<()>>,
    ) -> bool {
        // **CRITICAL**: Stop monitoring BEFORE hiding scratchpad to prevent false cancellations
        // The monitoring thread would detect the intentional hiding and cancel the operation
        monitoring_active.store(false, Ordering::SeqCst);

        // Wait for monitoring thread to complete (must complete before we hide the scratchpad)
        if let Some(handle) = handle {
            let _ = handle.join();
        }

        was_killed.load(Ordering::SeqCst)
    }

    /// Process request while monitoring scratchpad visibility in a background thread
    fn process_monitored_request(&self, request: MenuRequest) -> Result<MenuResponse> {
        let (monitoring_active, was_killed, monitoring_handle) = self.start_visibility_monitor();

        // Process the request normally - if fzf gets killed, it will return cancelled
        let result = self.process_request_internal(request);

        let killed =
            Self::finish_visibility_monitor(&monitoring_active, &was_killed, monitoring_handle);

        // If the process was killed due to invisibility, return cancelled regardless of what fzf returned
        if killed {
            Ok(MenuResponse::Cancelled)
        } else {
            result
        }
    }

    /// Monitored variant of `RequestProcessor::handle_choice_streaming`
    /// for streaming connections (same visibility semantics as
    /// `process_monitored_request`).
    fn process_monitored_streaming_choice<F: FnOnce() -> Result<()>>(
        &self,
        prompt: String,
        allow_multiple: bool,
        rx: crossbeam_channel::Receiver<SerializableMenuItem>,
        on_ready: F,
    ) -> Result<MenuResponse> {
        let (monitoring_active, was_killed, monitoring_handle) = self.start_visibility_monitor();

        let processor =
            RequestProcessor::new(self.running.clone(), self.requests_processed.clone());
        let result =
            processor.handle_choice_streaming_with_ready(prompt, allow_multiple, rx, on_ready);

        let killed =
            Self::finish_visibility_monitor(&monitoring_active, &was_killed, monitoring_handle);

        if killed {
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

        let status_info = MenuStatus {
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
pub fn scratchpad_config() -> ScratchpadConfig {
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
        Some(scratchpad_config())
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
    let scratchpad_config = scratchpad_config();

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
    use crate::menu_utils::MockQueue;

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

    #[test]
    fn framed_reader_preserves_coalesced_stream_frames() {
        let (mut writer, reader) = UnixStream::pair().unwrap();
        let request_id = "coalesced".to_string();
        let messages = [
            MenuMessage::new(
                request_id.clone(),
                MenuRequest::ChoiceBegin {
                    prompt: "Pick".to_string(),
                    allow_multiple: false,
                },
            ),
            MenuMessage::new(
                request_id.clone(),
                MenuRequest::ChoiceChunk {
                    items: vec![SerializableMenuItem::plain("first")],
                },
            ),
            MenuMessage::new(request_id, MenuRequest::ChoiceEnd),
        ];
        let wire = messages
            .iter()
            .map(|message| serde_json::to_string(message).unwrap() + "\n")
            .collect::<String>();
        writer.write_all(wire.as_bytes()).unwrap();
        writer.shutdown(Shutdown::Write).unwrap();

        let mut reader = io::BufReader::new(reader);
        assert!(matches!(
            read_menu_message(&mut reader).unwrap().unwrap().payload,
            MenuRequest::ChoiceBegin { .. }
        ));
        assert!(matches!(
            read_menu_message(&mut reader).unwrap().unwrap().payload,
            MenuRequest::ChoiceChunk { items } if items[0].display_text == "first"
        ));
        assert!(matches!(
            read_menu_message(&mut reader).unwrap().unwrap().payload,
            MenuRequest::ChoiceEnd
        ));
        assert!(read_menu_message(&mut reader).unwrap().is_none());
    }

    #[test]
    fn completed_stream_joins_reader_while_producer_remains_open() {
        let server = MenuServer {
            socket_path: default_socket_path(),
            running: Arc::new(AtomicBool::new(true)),
            start_time: std::time::SystemTime::now(),
            requests_processed: Arc::new(AtomicU64::new(0)),
            compositor: CompositorType::detect(),
            scratchpad_manager: None,
            tui: None,
        };
        let (mut client_stream, server_stream) = UnixStream::pair().unwrap();
        let request_id = "open-producer".to_string();
        let begin = MenuMessage::new(
            request_id.clone(),
            MenuRequest::ChoiceBegin {
                prompt: "Pick".to_string(),
                allow_multiple: false,
            },
        );
        serde_json::to_writer(&mut client_stream, &begin).unwrap();
        client_stream.write_all(b"\n").unwrap();

        // Keep client_stream's write side open to model an endless producer.
        // The handler must still stop and join its reader after selection.
        let _guard = MockQueue::new().cancel_selection().guard();
        server.handle_connection_sync(server_stream).unwrap();

        let mut reader = io::BufReader::new(client_stream);
        let mut ready = String::new();
        reader.read_line(&mut ready).unwrap();
        let ready: MenuResponseMessage = serde_json::from_str(ready.trim()).unwrap();
        assert_eq!(ready.request_id, request_id);
        assert!(matches!(ready.payload, MenuResponse::ChoiceReady));

        let mut response = String::new();
        reader.read_line(&mut response).unwrap();
        let response: MenuResponseMessage = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(response.request_id, request_id);
        assert!(matches!(response.payload, MenuResponse::Cancelled));
    }

    #[test]
    fn legacy_application_request_receives_protocol_error() {
        let server = MenuServer {
            socket_path: default_socket_path(),
            running: Arc::new(AtomicBool::new(true)),
            start_time: std::time::SystemTime::now(),
            requests_processed: Arc::new(AtomicU64::new(0)),
            compositor: CompositorType::detect(),
            scratchpad_manager: None,
            tui: None,
        };
        let (mut client_stream, server_stream) = UnixStream::pair().unwrap();
        let message = MenuMessage::new("legacy".to_string(), MenuRequest::Show);
        let mut value = serde_json::to_value(message).unwrap();
        value.as_object_mut().unwrap().remove("protocol_version");
        serde_json::to_writer(&mut client_stream, &value).unwrap();
        client_stream.write_all(b"\n").unwrap();

        server.handle_connection_sync(server_stream).unwrap();

        let mut response = String::new();
        io::BufReader::new(client_stream)
            .read_line(&mut response)
            .unwrap();
        let response: MenuResponseMessage = serde_json::from_str(response.trim()).unwrap();
        assert!(matches!(
            response.payload,
            MenuResponse::ProtocolMismatch { received, expected }
                if received == "1.0" && expected == PROTOCOL_VERSION
        ));
    }

    #[test]
    fn legacy_status_request_can_inspect_current_protocol() {
        let server = MenuServer {
            socket_path: default_socket_path(),
            running: Arc::new(AtomicBool::new(true)),
            start_time: std::time::SystemTime::now(),
            requests_processed: Arc::new(AtomicU64::new(0)),
            compositor: CompositorType::detect(),
            scratchpad_manager: None,
            tui: None,
        };
        let (mut client_stream, server_stream) = UnixStream::pair().unwrap();
        let message = MenuMessage::new("legacy-status".to_string(), MenuRequest::Status);
        let mut value = serde_json::to_value(message).unwrap();
        value.as_object_mut().unwrap().remove("protocol_version");
        serde_json::to_writer(&mut client_stream, &value).unwrap();
        client_stream.write_all(b"\n").unwrap();

        server.handle_connection_sync(server_stream).unwrap();

        let mut response = String::new();
        io::BufReader::new(client_stream)
            .read_line(&mut response)
            .unwrap();
        let response: MenuResponseMessage = serde_json::from_str(response.trim()).unwrap();
        assert!(matches!(
            response.payload,
            MenuResponse::StatusResult(status) if status.protocol_version == PROTOCOL_VERSION
        ));
    }
}
