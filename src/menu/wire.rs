//! NDJSON framing for the menu protocol.
//!
//! Frames are one JSON envelope per line. The server decodes the envelope
//! version before the payload so a breaking payload change still yields a
//! clean `ProtocolMismatch` instead of a dropped connection.

use super::protocol::{MenuMessage, PROTOCOL_VERSION};
use anyhow::{Context, Result};
use std::io::{BufRead, BufWriter, Write};
use std::os::unix::net::UnixStream;

/// Read one newline-delimited protocol frame without replacing the caller's
/// buffer. Returning `None` means the peer closed its write side cleanly.
pub(crate) fn read_menu_message<R: BufRead>(reader: &mut R) -> Result<Option<MenuMessage>> {
    match read_menu_frame(reader)? {
        Some(IncomingFrame::Message(message)) => Ok(Some(message)),
        // Legacy helper: version-gated callers handle mismatches explicitly;
        // direct readers treat them as a decode failure.
        Some(IncomingFrame::VersionMismatch { received, .. }) => {
            // Surface as an error so streaming readers break cleanly.
            // `handle_connection_sync` uses `read_menu_frame` directly and
            // replies with `ProtocolMismatch` instead.
            anyhow::bail!("Menu protocol mismatch: received {received}")
        }
        Some(IncomingFrame::InvalidPayload { error, .. }) => {
            anyhow::bail!("{error}")
        }
        None => Ok(None),
    }
}

/// Two-phase frame: envelope version is decoded before the payload so a
/// breaking payload change still yields a clean `ProtocolMismatch` instead of
/// a dropped connection.
pub(crate) enum IncomingFrame {
    Message(MenuMessage),
    VersionMismatch {
        request_id: String,
        received: String,
    },
    InvalidPayload {
        request_id: String,
        error: String,
    },
}

#[derive(serde::Deserialize)]
struct RawEnvelope {
    request_id: String,
    #[serde(default = "legacy_frame_version")]
    protocol_version: String,
    payload: serde_json::Value,
    timestamp: std::time::SystemTime,
}

fn legacy_frame_version() -> String {
    "1.0".to_string()
}

/// `Status`/`Stop` form the stable control plane: unit variants decode as a
/// bare string (`"Status"`), struct variants as a single-key map.
fn is_control_plane(payload: &serde_json::Value) -> bool {
    if let Some(name) = payload.as_str() {
        return name == "Status" || name == "Stop";
    }
    if let Some(object) = payload.as_object() {
        return object.contains_key("Status") || object.contains_key("Stop");
    }
    false
}

pub(crate) fn read_menu_frame<R: BufRead>(reader: &mut R) -> Result<Option<IncomingFrame>> {
    let mut json = String::new();
    if reader.read_line(&mut json)? == 0 {
        return Ok(None);
    }

    let raw: RawEnvelope = serde_json::from_str(json.trim_end())
        .context("Failed to deserialize menu protocol frame")?;
    if raw.protocol_version != PROTOCOL_VERSION && !is_control_plane(&raw.payload) {
        return Ok(Some(IncomingFrame::VersionMismatch {
            request_id: raw.request_id,
            received: raw.protocol_version,
        }));
    }
    let payload = match serde_json::from_value(raw.payload) {
        Ok(payload) => payload,
        Err(error) => {
            return Ok(Some(IncomingFrame::InvalidPayload {
                request_id: raw.request_id,
                error: format!("Failed to deserialize menu payload: {error}"),
            }));
        }
    };
    Ok(Some(IncomingFrame::Message(MenuMessage {
        request_id: raw.request_id,
        protocol_version: raw.protocol_version,
        payload,
        timestamp: raw.timestamp,
    })))
}

/// Write one NDJSON `MenuMessage` frame to a menu socket.
pub(crate) fn write_menu_message(stream: &UnixStream, message: &MenuMessage) -> Result<()> {
    write_menu_message_buffered(&mut BufWriter::new(stream), message)
}

/// Write one NDJSON frame via an existing buffered writer (flushes per
/// frame so chunks arrive with minimal latency).
pub(crate) fn write_menu_message_buffered<W: Write>(
    writer: &mut BufWriter<W>,
    message: &MenuMessage,
) -> Result<()> {
    serde_json::to_writer(&mut *writer, message).context("Failed to serialize menu message")?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::protocol::{ChoiceOptions, MenuRequest, SerializableMenuItem};
    use std::io::{self, Write};
    use std::net::Shutdown;
    use std::os::unix::net::UnixStream;

    #[test]
    fn framed_reader_preserves_coalesced_stream_frames() {
        let (mut writer, reader) = UnixStream::pair().unwrap();
        let request_id = "coalesced".to_string();
        let messages = [
            MenuMessage::new(
                request_id.clone(),
                MenuRequest::ChoiceBegin {
                    options: ChoiceOptions::new("Pick"),
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
}
