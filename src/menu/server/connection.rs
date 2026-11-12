//! Per-connection request dispatch for the menu server.

use super::MenuServer;
use crate::menu::protocol::*;
use crate::menu::scratchpad_manager::ScratchpadSession;
use crate::menu::wire::{IncomingFrame, read_menu_frame, read_menu_message};
use crate::menu_utils::process::kill_active_menu_processes;
use anyhow::{Context, Result};
use std::io::{self, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;

impl MenuServer {
    /// Handle a client connection synchronously.
    ///
    /// Single-frame requests use the historical one-line-in/one-line-out
    /// flow. `ChoiceBegin` switches the connection into streaming mode:
    /// the server opens the menu immediately and consumes `ChoiceChunk`
    /// / `ChoiceEnd` frames on the same socket while fzf runs (see
    /// `handle_choice_stream_connection`).
    pub(super) fn handle_connection_sync(&self, mut stream: UnixStream) -> Result<()> {
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
        let Some(frame) = read_menu_frame(&mut reader)? else {
            // Client disconnected - this is normal, not an error
            return Ok(());
        };
        self.record_request();
        let message = match frame {
            IncomingFrame::VersionMismatch {
                request_id,
                received,
            } => {
                return Self::write_response(
                    &mut stream,
                    request_id,
                    MenuResponse::ProtocolMismatch {
                        received,
                        expected: PROTOCOL_VERSION.to_string(),
                    },
                );
            }
            IncomingFrame::InvalidPayload { request_id, error } => {
                return Self::write_response(&mut stream, request_id, MenuResponse::Error(error));
            }
            IncomingFrame::Message(message) => message,
        };

        // Version gating lives solely in `read_menu_frame`: non-control-plane
        // mismatches already returned as `ProtocolMismatch` above, and
        // `Status`/`Stop` form the stable control plane. No second check here
        // so the two `Status`/`Stop` lists cannot diverge.

        if let MenuRequest::ChoiceBegin { options } = message.payload {
            crate::menu::bindings::validate(&options.bindings)?;
            if options.frecency_cache.is_some() {
                return self.handle_ranked_choice_stream_connection(
                    stream,
                    reader,
                    message.request_id,
                    options,
                );
            }
            return self.handle_choice_stream_connection(
                stream,
                reader,
                message.request_id,
                options,
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
    /// `FzfBuilder::stream`; the main thread runs fzf under the standard
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
        options: ChoiceOptions,
    ) -> Result<()> {
        let scratchpad = ScratchpadSession::show(self.scratchpad_manager.as_ref());

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

        let response_result = self.process_monitored_streaming_choice(options, rx, || {
            Self::write_response(&mut stream, request_id.clone(), MenuResponse::ChoiceReady)
        });

        // Always tear down the reader, even when processing failed. Otherwise
        // the detached reader can outlive the request and cancel a later menu.
        // Mark completion before waking the reader so EOF caused by our own
        // shutdown cannot be mistaken for an aborted producer. SHUT_RD leaves
        // the write side available for the response below.
        selection_finished.store(true, Ordering::SeqCst);
        let _ = stream.shutdown(Shutdown::Read);
        let join_result = reader_handle.join();

        let response = match (response_result, join_result) {
            (Ok(response), Ok(())) => response,
            (Err(error), _) => return Err(error),
            (Ok(_), Err(_)) => anyhow::bail!("Streaming choice reader thread panicked"),
        };

        // Hide before sending the response; the guard also covers error paths.
        drop(scratchpad);

        Self::write_response(&mut stream, request_id, response)
    }

    /// Collect a complete streamed corpus before starting fzf so menu-owned
    /// frecency can establish one global order. The scratchpad is shown and
    /// `ChoiceReady` is sent first, allowing discovery to run only after the
    /// UI has claimed its place on screen.
    fn handle_ranked_choice_stream_connection(
        &self,
        mut stream: UnixStream,
        mut reader: io::BufReader<UnixStream>,
        request_id: String,
        options: ChoiceOptions,
    ) -> Result<()> {
        let scratchpad = ScratchpadSession::show(self.scratchpad_manager.as_ref());

        reader.get_ref().set_read_timeout(None)?;
        Self::write_response(&mut stream, request_id.clone(), MenuResponse::ChoiceReady)?;

        let mut items = Vec::new();
        let response = loop {
            let Some(frame) = read_menu_message(&mut reader)? else {
                break MenuResponse::Error(
                    "Ranked choice stream ended before ChoiceEnd".to_string(),
                );
            };
            if frame.request_id != request_id || frame.protocol_version != PROTOCOL_VERSION {
                break MenuResponse::Error("Invalid ranked choice stream frame".to_string());
            }
            match frame.payload {
                MenuRequest::ChoiceChunk { items: chunk } => items.extend(chunk),
                MenuRequest::ChoiceEnd => {
                    let request = MenuRequest::Choice { options, items };
                    break self.process_monitored_request(request)?;
                }
                _ => {
                    break MenuResponse::Error(
                        "Unexpected frame in ranked choice stream".to_string(),
                    );
                }
            }
        };

        // Hide before sending the response; the guard also covers error paths.
        drop(scratchpad);
        Self::write_response(&mut stream, request_id, response)
    }
}
