//! Shared stdin pumps for streaming choice menus.
//!
//! Every streaming choice entry point (`menu choice` over TUI, hosted
//! server, and keybind paths, plus the instantmenu pipe) reads
//! newline-delimited plain items from stdin on a detached background
//! thread. The caller keeps rendering while stdin is still open, so these
//! handles are intentionally never joined — a short-lived CLI process
//! exiting kills the pump.

use super::protocol::{STREAM_ITEM_BUFFER_CAPACITY, SerializableMenuItem};
use std::io::{self, BufRead, BufReader, IsTerminal};
use std::thread::{self, JoinHandle};

/// Batch size ceiling when draining already-buffered stdin lines. It never
/// waits to fill a batch — one blocking read, then drain whatever complete
/// lines are already buffered — preserving the latency of sparse producers
/// while amortizing framing for bursty ones.
const STREAM_CHUNK_MAX_ITEMS: usize = 64;

pub(crate) struct StdinItemPumpOptions {
    /// Drain already-buffered lines in batches (used by the hosted client,
    /// which forwards each batch as one `ChoiceChunk` frame).
    pub batched: bool,
    /// Skip pumping when stdin is a terminal (used by the keybind path, so
    /// an interactive shell never blocks the menu).
    pub skip_when_terminal: bool,
}

/// Spawn a detached thread that reads newline-delimited plain items from
/// stdin into a bounded channel.
pub(crate) fn spawn_stdin_item_pump(
    options: StdinItemPumpOptions,
) -> crossbeam_channel::Receiver<SerializableMenuItem> {
    let (sender, receiver) =
        crossbeam_channel::bounded::<SerializableMenuItem>(STREAM_ITEM_BUFFER_CAPACITY);
    thread::spawn(move || {
        if options.skip_when_terminal && io::stdin().is_terminal() {
            return;
        }
        let stdin = io::stdin();
        let mut reader = BufReader::new(stdin.lock());
        loop {
            let items = if options.batched {
                match read_plain_choice_chunk(&mut reader) {
                    Ok(Some(items)) => items,
                    Ok(None) | Err(_) => return,
                }
            } else {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => return,
                    Ok(_) => vec![SerializableMenuItem::plain(
                        line.trim_end_matches(['\r', '\n']),
                    )],
                    Err(_) => return,
                }
            };
            for item in items {
                if sender.send(item).is_err() {
                    return;
                }
            }
        }
    });
    receiver
}

/// Spawn a detached thread that forwards stdin lines verbatim (newline
/// included) into `writer`. Used by the instantmenu streaming path, where
/// the child grows its list as bytes arrive.
pub(crate) fn spawn_stdin_to_writer_pump<W>(writer: W) -> JoinHandle<()>
where
    W: io::Write + Send + 'static,
{
    thread::spawn(move || {
        use std::io::Write;
        let mut writer = io::BufWriter::new(writer);
        let stdin = io::stdin();
        let mut reader = BufReader::new(stdin.lock());
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    if writer.write_all(line.as_bytes()).is_err() || writer.flush().is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    })
}

/// Read at least one choice line, then collect only additional complete lines
/// already held by `BufReader`. It never waits to fill a batch, preserving the
/// latency of sparse producers while amortizing framing for bursty ones.
pub(crate) fn read_plain_choice_chunk<R: io::Read>(
    reader: &mut BufReader<R>,
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
