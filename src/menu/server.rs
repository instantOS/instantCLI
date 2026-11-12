use super::processing::RequestProcessor;
use super::protocol::*;
use super::scratchpad_manager::{ScratchpadManager, ScratchpadSession};
use super::tui::MenuServerTui;
use crate::common::compositor::CompositorType;
use crate::menu_utils::process::kill_active_menu_processes;
use crate::scratchpad::config::ScratchpadConfig;
use anyhow::{Context, Result};
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::thread;
use std::time::Duration;
use tokio::signal;

mod connection;

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

                            if let Err(error) = self.handle_connection_sync(std_stream) {
                                eprintln!("Menu request failed: {error:#}");
                            }

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

        self.cleanup_socket();
        Ok(())
    }

    /// Clean up socket file
    fn cleanup_socket(&self) {
        if Path::new(&self.socket_path).exists() {
            if let Err(e) = std::fs::remove_file(&self.socket_path) {
                eprintln!("Failed to remove socket file: {e}");
            } else {
                println!("Socket file cleaned up");
            }
        }
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
                | MenuRequest::FilePicker { .. }
                | MenuRequest::Slide(_)
                | MenuRequest::Message { .. }
                | MenuRequest::Toast { .. }
        );

        // The guard hides the scratchpad on every exit path, including errors.
        let scratchpad = should_manage_scratchpad
            .then(|| ScratchpadSession::show(self.scratchpad_manager.as_ref()));

        // Process the request with timeout and visibility monitoring.
        let response = if should_manage_scratchpad {
            self.process_monitored_request(request)
        } else {
            self.process_request_internal(request)
        };

        // **PERFORMANCE CRITICAL**: Hide the scratchpad as soon as the menu
        // interaction ends. Dropping the guard also covers error paths.
        drop(scratchpad);

        response
    }

    /// Process a menu request internal logic using the dedicated processor
    fn process_request_internal(&self, request: MenuRequest) -> Result<MenuResponse> {
        // Handle status request specially to get server-specific information
        if matches!(request, MenuRequest::Status) {
            return Ok(self.get_status_info());
        }

        self.processor().process_internal(request)
    }

    /// Count one accepted request for status/TUI reporting.
    fn record_request(&self) {
        self.requests_processed.fetch_add(1, Ordering::SeqCst);
    }

    /// Build a processor sharing this server's control flags.
    fn processor(&self) -> RequestProcessor {
        RequestProcessor::new(self.running.clone(), self.requests_processed.clone())
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

    /// Run `operation` while monitoring scratchpad visibility in a background
    /// thread. A menu killed by the monitor reports `Cancelled` regardless of
    /// what the renderer returned.
    fn run_monitored<F>(&self, operation: F) -> Result<MenuResponse>
    where
        F: FnOnce() -> Result<MenuResponse>,
    {
        let (monitoring_active, was_killed, monitoring_handle) = self.start_visibility_monitor();

        let result = operation();

        let killed =
            Self::finish_visibility_monitor(&monitoring_active, &was_killed, monitoring_handle);

        if killed {
            Ok(MenuResponse::Cancelled)
        } else {
            result
        }
    }

    /// Process request while monitoring scratchpad visibility in a background thread
    fn process_monitored_request(&self, request: MenuRequest) -> Result<MenuResponse> {
        self.run_monitored(|| self.process_request_internal(request))
    }

    /// Monitored variant of `RequestProcessor::handle_choice_streaming`
    /// for streaming connections (same visibility semantics as
    /// `process_monitored_request`).
    fn process_monitored_streaming_choice<F: FnOnce() -> Result<()>>(
        &self,
        options: ChoiceOptions,
        rx: crossbeam_channel::Receiver<SerializableMenuItem>,
        on_ready: F,
    ) -> Result<MenuResponse> {
        self.run_monitored(|| {
            self.processor()
                .handle_choice_streaming(options, rx, on_ready)
        })
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

/// Create a scratchpad configuration for the menu server
pub fn scratchpad_config() -> ScratchpadConfig {
    use crate::scratchpad::config::ScratchpadConfig;

    let current_exe = std::env::current_exe().unwrap_or_else(|_| env!("CARGO_BIN_NAME").into());
    let launcher = instantos_terminal::Launcher::new(current_exe)
        .args(["menu", "server", "launch", "--inside"]);
    ScratchpadConfig::with_launcher("insmenu".into(), launcher, 50, 60)
}

async fn wait_for_scratchpad_registration(
    mut registered: impl FnMut() -> Result<bool>,
) -> Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if registered()? {
            return Ok(());
        }
        anyhow::ensure!(
            tokio::time::Instant::now() < deadline,
            "menu terminal did not register as a scratchpad; expected app ID org.instantos.scratchpad.insmenu"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
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
    // The terminal can start even when it ignores its app ID. Do not leave an
    // orphan server behind when the launcher times out waiting for registration.
    if let Some(config) = scratchpad_config.as_ref() {
        wait_for_scratchpad_registration(|| compositor.provider().is_window_running(config))
            .await?;
    }
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

    #[tokio::test]
    async fn registration_wait_handles_mapping_delay_and_failure() {
        let mut attempts = 0;
        wait_for_scratchpad_registration(|| {
            attempts += 1;
            Ok(attempts == 2)
        })
        .await
        .unwrap();
        assert_eq!(attempts, 2);
        assert!(
            wait_for_scratchpad_registration(|| Ok(false))
                .await
                .is_err()
        );
        assert!(
            wait_for_scratchpad_registration(|| anyhow::bail!("IPC unavailable"))
                .await
                .is_err()
        );
    }

    use crate::menu_utils::MockQueue;
    use std::io::{self, BufRead, Write};
    use std::os::unix::net::UnixStream;

    fn test_server(running: bool) -> MenuServer {
        MenuServer {
            socket_path: default_socket_path(),
            running: Arc::new(AtomicBool::new(running)),
            start_time: std::time::SystemTime::now(),
            requests_processed: Arc::new(AtomicU64::new(0)),
            compositor: CompositorType::detect(),
            scratchpad_manager: None,
            tui: None, // Skip TUI for tests
        }
    }

    #[test]
    fn test_server_creation() {
        let server = test_server(false);
        assert!(!server.socket_path.is_empty());
    }

    #[test]
    fn request_counter_counts_one_per_connection() {
        let server = test_server(true);
        let (mut client_stream, server_stream) = UnixStream::pair().unwrap();
        let message = MenuMessage::new("count".to_string(), MenuRequest::Status);
        serde_json::to_writer(&mut client_stream, &message).unwrap();
        client_stream.write_all(b"\n").unwrap();

        server.handle_connection_sync(server_stream).unwrap();

        assert_eq!(server.requests_processed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn request_counter_does_not_double_count_processed_requests() {
        let server = test_server(true);
        let (mut client_stream, server_stream) = UnixStream::pair().unwrap();
        let message = MenuMessage::new(
            "count-choice".to_string(),
            MenuRequest::Choice {
                options: ChoiceOptions::new("Pick"),
                items: Vec::new(),
            },
        );
        serde_json::to_writer(&mut client_stream, &message).unwrap();
        client_stream.write_all(b"\n").unwrap();

        server.handle_connection_sync(server_stream).unwrap();

        assert_eq!(server.requests_processed.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn empty_connection_does_not_count_as_request() {
        let server = test_server(true);
        let (client_stream, server_stream) = UnixStream::pair().unwrap();
        drop(client_stream);

        server.handle_connection_sync(server_stream).unwrap();

        assert_eq!(server.requests_processed.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn completed_stream_joins_reader_while_producer_remains_open() {
        let server = test_server(true);
        let (mut client_stream, server_stream) = UnixStream::pair().unwrap();
        let request_id = "open-producer".to_string();
        let begin = MenuMessage::new(
            request_id.clone(),
            MenuRequest::ChoiceBegin {
                options: ChoiceOptions::new("Pick"),
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
    fn ranked_stream_preserves_bindings() {
        let server = test_server(true);
        let (mut client_stream, server_stream) = UnixStream::pair().unwrap();
        let request_id = "ranked".to_string();
        let server_thread = std::thread::spawn(move || {
            let _guard = MockQueue::new().keybind_action("ctrl-e", vec![]).guard();
            server.handle_connection_sync(server_stream).unwrap();
        });

        let begin = MenuMessage::new(
            request_id.clone(),
            MenuRequest::ChoiceBegin {
                options: ChoiceOptions::new("Pick")
                    .with_frecency_cache(Some("server_test".to_string()))
                    .with_bindings(vec!["ctrl-e:Edit".parse().unwrap()]),
            },
        );
        serde_json::to_writer(&mut client_stream, &begin).unwrap();
        client_stream.write_all(b"\n").unwrap();

        let read_stream = client_stream.try_clone().unwrap();
        let mut reader = io::BufReader::new(read_stream);
        let ready = read_menu_message_response(&mut reader);
        assert!(matches!(ready.payload, MenuResponse::ChoiceReady));

        let chunk = MenuMessage::new(
            request_id.clone(),
            MenuRequest::ChoiceChunk {
                items: vec![SerializableMenuItem::plain("item")],
            },
        );
        serde_json::to_writer(&mut client_stream, &chunk).unwrap();
        client_stream.write_all(b"\n").unwrap();
        let end = MenuMessage::new(request_id.clone(), MenuRequest::ChoiceEnd);
        serde_json::to_writer(&mut client_stream, &end).unwrap();
        client_stream.write_all(b"\n").unwrap();

        let response = read_menu_message_response(&mut reader);
        assert_eq!(response.request_id, request_id);
        assert!(
            matches!(response.payload, MenuResponse::ChoiceResult { action: Some(action), items }
                if action == "ctrl-e" && items.is_empty())
        );
        server_thread.join().unwrap();
    }

    fn read_menu_message_response(reader: &mut impl io::BufRead) -> MenuResponseMessage {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        serde_json::from_str(line.trim()).unwrap()
    }

    #[test]
    fn legacy_application_request_receives_protocol_error() {
        let server = test_server(true);
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
        let server = test_server(true);
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

    #[test]
    fn old_input_payload_with_old_version_yields_mismatch_not_drop() {
        // 7.x wire (`secret` bool, no `kind`) sent with an old version must
        // get a clean `ProtocolMismatch`, not a dropped connection.
        let server = test_server(true);
        let (mut client_stream, server_stream) = UnixStream::pair().unwrap();
        let raw = serde_json::json!({
            "request_id": "old-input",
            "protocol_version": "7.0",
            "payload": {"Input": {"options": {"prompt": "x", "secret": true}}},
            "timestamp": std::time::SystemTime::now(),
        });
        serde_json::to_writer(&mut client_stream, &raw).unwrap();
        client_stream.write_all(b"\n").unwrap();

        server.handle_connection_sync(server_stream).unwrap();

        let mut response = String::new();
        io::BufReader::new(client_stream)
            .read_line(&mut response)
            .unwrap();
        assert!(!response.trim().is_empty(), "server must reply, not drop");
        let response: MenuResponseMessage = serde_json::from_str(response.trim()).unwrap();
        assert!(matches!(
            response.payload,
            MenuResponse::ProtocolMismatch { received, .. } if received == "7.0"
        ));
    }

    #[test]
    fn same_version_invalid_input_yields_error_not_drop() {
        // `password` + `initial_text` with the current version fails closed
        // with an `Error` response carrying the request id.
        let server = test_server(true);
        let (mut client_stream, server_stream) = UnixStream::pair().unwrap();
        let raw = serde_json::json!({
            "request_id": "bad-input",
            "protocol_version": PROTOCOL_VERSION,
            "payload": {"Input": {"options": {"prompt": "p", "kind": "password", "initial_text": "x"}}},
            "timestamp": std::time::SystemTime::now(),
        });
        serde_json::to_writer(&mut client_stream, &raw).unwrap();
        client_stream.write_all(b"\n").unwrap();

        server.handle_connection_sync(server_stream).unwrap();

        let mut response = String::new();
        io::BufReader::new(client_stream)
            .read_line(&mut response)
            .unwrap();
        let response: MenuResponseMessage = serde_json::from_str(response.trim()).unwrap();
        assert_eq!(response.request_id, "bad-input");
        assert!(matches!(response.payload, MenuResponse::Error(_)));
    }
}
