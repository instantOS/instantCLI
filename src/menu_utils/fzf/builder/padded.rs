use anyhow::Result;
use std::collections::HashSet;
use std::io::Write;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

use base64::{Engine as _, engine::general_purpose};
use crossbeam_channel::{TryRecvError, select};

use super::FzfBuilder;
use super::SharedConfig;
use super::shared::{
    FzfCommandOptions, apply_fzf_command_options, base_fzf_command, build_padded_item_from_lines,
    default_header_text, run_fzf_with_input,
};
use crate::menu_utils::fzf::types::{
    DialogOutcome, FzfPreview, FzfSelectable, InitialCursor, MenuKeybind, MenuSelection,
};
use crate::menu_utils::fzf::wrapper::{finish_menu_child, fzf_was_cancelled, spawn_menu_child};

/// Invisible marker used to keep non-selectable padded rows visible while fzf
/// navigation only visits actual menu actions.
const SELECTABLE_MARKER: &str = "\u{2060}";

/// Horizontal offset that keeps the hidden `\x1f`-delimited keyword field
/// off-screen on typical terminals.
const HIDDEN_PADDING: &str = "                                                                                                    ";
/// Wider offset used when no row has a preview, so keywords stay hidden even
/// on very wide terminals.
const EXTRA_WIDE_PADDING: &str = "                                                                                                                                                                                                                                                                    ";

impl FzfBuilder {
    pub(crate) fn run_padded_items<T: FzfSelectable + Clone, A: Clone>(
        mut self,
        items: Vec<T>,
        keybinds: &[MenuKeybind<A>],
        allow_multiple: bool,
    ) -> Result<DialogOutcome<MenuSelection<T, A>>> {
        super::super::keybind::validate(keybinds)?;
        #[cfg(test)]
        if let Some(resp) = crate::menu_utils::mock::pop_mock() {
            return Ok(crate::menu_utils::mock::resolve_selection(
                resp, items, keybinds,
            ));
        }

        if items.is_empty() && keybinds.is_empty() {
            return Ok(DialogOutcome::Cancelled);
        }

        let has_non_selectable = items.iter().any(|item| !item.fzf_is_selectable());
        if has_non_selectable {
            let requested_index = self
                .shared
                .initial_cursor
                .as_ref()
                .map(|InitialCursor::Index(index)| *index);
            match nearest_selectable_index(&items, requested_index) {
                Some(initial_index) => {
                    self.shared.initial_cursor = Some(InitialCursor::Index(initial_index));
                }
                None if keybinds.is_empty() => return Ok(DialogOutcome::Cancelled),
                None => self.shared.initial_cursor = None,
            }
        }

        let has_keywords = items
            .iter()
            .any(|item| !item.fzf_search_keywords().is_empty());

        let input_text = prepare_padded_input(&items, has_non_selectable);
        let has_preview = items
            .iter()
            .any(|item| !matches!(item.fzf_preview(), FzfPreview::None));
        let preview_manifest = if has_preview {
            Some(prepare_padded_preview_manifest(&items)?)
        } else {
            None
        };

        let result = loop {
            let cmd = configure_padded_cmd(
                &self.shared,
                preview_manifest.as_ref().map(tempfile::NamedTempFile::path),
                has_keywords,
                has_non_selectable,
                keybinds,
                allow_multiple,
            );
            let output = run_fzf_with_input(cmd, input_text.as_bytes())?;

            if fzf_was_cancelled(&output)? {
                break DialogOutcome::Cancelled;
            }
            if !output.status.success() {
                break DialogOutcome::Cancelled;
            }

            let response = parse_padded_response(&output.stdout, &items, keybinds)?;
            match response {
                DialogOutcome::Submitted(selection)
                    if selection.action.is_some() || !selection.items.is_empty() =>
                {
                    break DialogOutcome::Submitted(selection);
                }
                DialogOutcome::Cancelled => break DialogOutcome::Cancelled,
                DialogOutcome::Submitted(_) => {}
            }
            // Pointer selection can still land on a raw, non-matching row.
            // Reopen instead of returning a header as if it were an action.
        };

        Ok(result)
    }

    /// Run a padded menu whose items arrive over a channel.
    ///
    /// Compact streams append encoded rows directly to fzf's input; padded
    /// streams must append two aligned things per item: the multiline input
    /// record and the item's preview-manifest entry, because manifest line N
    /// always describes input row N — the index fzf reports back through its
    /// `{n}` placeholders. The pump writes both in one step, manifest first,
    /// so a preview triggered by a freshly appended row always finds its
    /// entry.
    ///
    /// Unlike the static padded menu, the padding machinery is enabled
    /// unconditionally: whether late rows carry keywords, separators, or
    /// previews cannot be known when fzf is spawned. Rows without keywords
    /// simply carry an empty keyword field.
    ///
    /// The cursor is positioned against the initial rows only. A submit
    /// whose indices all landed on padding rows cannot be reopened against
    /// a closed stream, so it resolves to [`DialogOutcome::Cancelled`].
    pub(crate) fn run_padded_stream<'a, T, A>(
        mut self,
        initial_items: Vec<T>,
        late_items: crossbeam_channel::Receiver<T>,
        keybinds: &[MenuKeybind<A>],
        allow_multiple: bool,
        on_ready: Option<Box<dyn FnOnce() -> Result<()> + 'a>>,
    ) -> Result<DialogOutcome<MenuSelection<T, A>>>
    where
        T: FzfSelectable + Clone + Send + 'static,
        A: Clone,
    {
        super::super::keybind::validate(keybinds)?;
        #[cfg(test)]
        if let Some(resp) = crate::menu_utils::mock::pop_mock() {
            if let Some(on_ready) = on_ready {
                on_ready()?;
            }
            let mut items = initial_items;
            while let Ok(item) = late_items.try_recv() {
                items.push(item);
            }
            return Ok(crate::menu_utils::mock::resolve_selection(
                resp, items, keybinds,
            ));
        }

        let mark_selectable = true;

        // The cursor can only land on rows fzf already has, so preselecting
        // snaps to the nearest selectable initial row.
        if !initial_items.is_empty() {
            let requested_index = self
                .shared
                .initial_cursor
                .as_ref()
                .map(|InitialCursor::Index(index)| *index);
            match nearest_selectable_index(&initial_items, requested_index) {
                Some(initial_index) => {
                    self.shared.initial_cursor = Some(InitialCursor::Index(initial_index));
                }
                None => self.shared.initial_cursor = None,
            }
        }

        let mut store = PaddedStreamStore {
            items: initial_items,
            seen_keys: HashSet::new(),
        };
        store.seen_keys = store.items.iter().map(FzfSelectable::fzf_key).collect();

        // The manifest must exist for the whole session: late rows may carry
        // previews even when the initial rows do not.
        let mut manifest = tempfile::NamedTempFile::new()?;
        for entry in store.items.iter().map(manifest_entry) {
            manifest.write_all(entry.as_bytes())?;
        }

        let cmd = configure_padded_cmd(
            &self.shared,
            Some(manifest.path()),
            // Keyword delimiter and no-hscroll are harmless for rows without
            // keywords and protect late keyword rows.
            true,
            mark_selectable,
            keybinds,
            allow_multiple,
        );

        let mut child = spawn_menu_child(cmd)?;

        let mut stdin = child
            .inner_mut()
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to capture fzf stdin"))?;
        // Every record is NUL-terminated so the first streamed item cannot
        // glue onto the last initial row.
        for record in store
            .items
            .iter()
            .map(|item| build_padded_record(item, false, mark_selectable))
        {
            stdin.write_all(record.as_bytes())?;
            stdin.write_all(b"\0")?;
        }
        stdin.flush()?;
        if let Some(on_ready) = on_ready {
            on_ready()?;
        }

        let pump_store = Arc::new(Mutex::new(store));
        let pump_items = Arc::clone(&pump_store);
        let (cancel_tx, cancel_rx) = crossbeam_channel::bounded::<()>(1);
        let pump = thread::spawn(move || {
            let mut stdin = stdin;
            let mut manifest = manifest;
            loop {
                let first = select! {
                    recv(cancel_rx) -> _ => break,
                    recv(late_items) -> item => match item {
                        Ok(item) => item,
                        Err(_) => break,
                    },
                };

                let mut batch = vec![first];
                while batch.len() < 64 {
                    match late_items.try_recv() {
                        Ok(item) => batch.push(item),
                        Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
                    }
                }

                let mut manifest_batch = String::new();
                let mut record_batch = String::new();
                let Ok(mut guard) = pump_items.lock() else {
                    break;
                };
                for item in batch {
                    let key = item.fzf_key();
                    if guard.seen_keys.contains(&key) {
                        continue;
                    }
                    guard.seen_keys.insert(key);
                    manifest_batch.push_str(&manifest_entry(&item));
                    record_batch.push_str(&build_padded_record(&item, false, mark_selectable));
                    record_batch.push('\0');
                    guard.items.push(item);
                }
                drop(guard);

                // The manifest entry must be readable before the row becomes
                // visible, so flush it before feeding the records.
                let manifest_ok = manifest
                    .write_all(manifest_batch.as_bytes())
                    .and_then(|_| manifest.flush());
                let records_ok = stdin
                    .write_all(record_batch.as_bytes())
                    .and_then(|_| stdin.flush());
                if manifest_ok.is_err() || records_ok.is_err() || cancel_rx.try_recv().is_ok() {
                    break;
                }
            }
        });

        let output = finish_menu_child(child);
        let _ = cancel_tx.try_send(());
        let _ = pump.join();
        let output = output?;

        if fzf_was_cancelled(&output)? || !output.status.success() {
            return Ok(DialogOutcome::Cancelled);
        }

        let guard = pump_store
            .lock()
            .map_err(|_| anyhow::anyhow!("padded streaming item store poisoned"))?;
        match parse_padded_response(&output.stdout, &guard.items, keybinds)? {
            DialogOutcome::Submitted(selection)
                if selection.action.is_some() || !selection.items.is_empty() =>
            {
                Ok(DialogOutcome::Submitted(selection))
            }
            // A pointer selection on a padding row yields an empty submit;
            // unlike the static menu there is nothing to reopen.
            _ => Ok(DialogOutcome::Cancelled),
        }
    }
}

/// Items handed to fzf so far, plus the keys already fed. The pump skips
/// late duplicates so input indices and manifest lines stay aligned.
struct PaddedStreamStore<T> {
    items: Vec<T>,
    seen_keys: HashSet<String>,
}

fn nearest_selectable_index<T: FzfSelectable>(
    items: &[T],
    requested_index: Option<usize>,
) -> Option<usize> {
    if items.is_empty() {
        return None;
    }

    let requested = requested_index.unwrap_or(0).min(items.len() - 1);
    if items[requested].fzf_is_selectable() {
        return Some(requested);
    }
    if let Some(offset) = items[requested..]
        .iter()
        .position(FzfSelectable::fzf_is_selectable)
    {
        return Some(requested + offset);
    }
    items[..requested]
        .iter()
        .rposition(FzfSelectable::fzf_is_selectable)
}

fn prepare_padded_input<T: FzfSelectable>(items: &[T], mark_selectable: bool) -> String {
    let has_previews = items
        .iter()
        .any(|item| !matches!(item.fzf_preview(), FzfPreview::None));

    items
        .iter()
        .map(|item| build_padded_record(item, has_previews, mark_selectable))
        .collect::<Vec<_>>()
        .join("\0")
}

/// Render one item as a complete multiline NUL record.
///
/// With `mark_selectable`, selectable rows carry the invisible marker that
/// the `--raw` query filters for. `has_previews` only chooses how far the
/// hidden keyword field sits off-screen; it never changes what is searched.
fn build_padded_record<T: FzfSelectable>(
    item: &T,
    has_previews: bool,
    mark_selectable: bool,
) -> String {
    let display = item.fzf_display_text();
    let keywords = item.fzf_search_keywords().join(" ");

    let mut middle_line = if keywords.is_empty() {
        format!("  {display}")
    } else if has_previews {
        format!("  {display}{HIDDEN_PADDING}\x1f{keywords}")
    } else {
        format!("  {display}{EXTRA_WIDE_PADDING}\x1f{keywords}")
    };
    if mark_selectable && item.fzf_is_selectable() {
        middle_line = format!("{SELECTABLE_MARKER}{middle_line}");
    }

    build_padded_item_from_lines(&display, &middle_line)
}

/// One preview-manifest line: kind, base64 content, base64 item key. The
/// line's position in the file must match the row's fzf input index.
fn manifest_entry<T: FzfSelectable>(item: &T) -> String {
    let (kind, content) = match item.fzf_preview() {
        FzfPreview::Text(text) => ("T", text),
        FzfPreview::Command(command) => ("C", command),
        FzfPreview::None => ("N", String::new()),
    };
    format!(
        "{kind}\t{}\t{}\n",
        general_purpose::STANDARD.encode(content),
        general_purpose::STANDARD.encode(item.fzf_key())
    )
}

fn prepare_padded_preview_manifest<T: FzfSelectable>(
    items: &[T],
) -> Result<tempfile::NamedTempFile> {
    let mut manifest = tempfile::NamedTempFile::new()?;
    for item in items {
        manifest.write_all(manifest_entry(item).as_bytes())?;
    }
    Ok(manifest)
}

fn configure_padded_cmd<A>(
    shared: &SharedConfig,
    preview_manifest: Option<&std::path::Path>,
    has_keywords: bool,
    has_non_selectable: bool,
    keybinds: &[MenuKeybind<A>],
    allow_multiple: bool,
) -> Command {
    let mut cmd = base_fzf_command();

    cmd.arg("--read0");
    cmd.arg("--ansi");
    cmd.arg("--highlight-line");
    cmd.arg("--layout=reverse");
    cmd.arg("--tiebreak=index");
    cmd.arg("--info=inline-right");

    if has_keywords {
        cmd.arg("--delimiter=\x1f").arg("--no-hscroll");
    }

    if allow_multiple {
        cmd.arg("--multi");
    }

    // Do not parse the rendered multiline records. Emit a small response
    // envelope containing fzf's stable input indices instead.
    cmd.arg("--bind")
        .arg("enter:become(printf '%s\\n' submit {+n})");
    for bind in keybinds {
        let token = general_purpose::STANDARD.encode(bind.key.as_str());
        cmd.arg("--bind").arg(format!(
            "{}:become(printf '%s\\n' action {token} {{+n}})",
            bind.key
        ));
    }

    if has_non_selectable {
        cmd.arg("--raw")
            .arg(format!("--query={SELECTABLE_MARKER}"))
            .arg("--gutter-raw= ")
            .arg("--bind")
            .arg(
                [
                    "up:up-match",
                    "down:down-match",
                    "ctrl-p:up-match",
                    "ctrl-n:down-match",
                    "ctrl-k:up-match",
                    "ctrl-j:down-match",
                    "result:best",
                ]
                .join(","),
            );
    }

    if let Some(manifest) = preview_manifest {
        cmd.arg("--preview").arg(padded_preview_command(manifest));
    }

    let cursor = shared
        .initial_cursor
        .as_ref()
        .map(|InitialCursor::Index(index)| *index);
    let header = padded_header_text(shared, keybinds);
    apply_fzf_command_options(
        &mut cmd,
        shared,
        FzfCommandOptions {
            prompt_suffix: Some(" > "),
            header,
            include_additional_args: true,
            cursor,
            responsive_layout: true,
        },
    );

    cmd
}

fn padded_header_text<A>(shared: &SharedConfig, keybinds: &[MenuKeybind<A>]) -> Option<String> {
    let hint = (!keybinds.is_empty())
        .then(|| super::super::keybind::render_hint(keybinds, shared.responsive_layout));
    match (default_header_text(shared), hint) {
        (Some(mut header), Some(hint)) => {
            header.push('\n');
            header.push_str(&hint);
            Some(header)
        }
        (Some(header), None) => Some(header),
        (None, hint) => hint,
    }
}

fn parse_padded_response<T: FzfSelectable + Clone, A: Clone>(
    stdout: &[u8],
    items: &[T],
    keybinds: &[MenuKeybind<A>],
) -> Result<DialogOutcome<MenuSelection<T, A>>> {
    let text = String::from_utf8_lossy(stdout);
    let mut lines = text.lines();
    let Some(kind) = lines.next() else {
        return Ok(DialogOutcome::Cancelled);
    };
    let action = match kind {
        "submit" => None,
        "action" => {
            let encoded = lines
                .next()
                .ok_or_else(|| anyhow::anyhow!("fzf returned an action without a token"))?;
            let token = general_purpose::STANDARD.decode(encoded).map_err(|error| {
                anyhow::anyhow!("fzf returned an invalid action token: {error}")
            })?;
            let token = String::from_utf8(token).map_err(|error| {
                anyhow::anyhow!("fzf returned a non-UTF-8 action token: {error}")
            })?;
            Some(super::super::keybind::resolve_action(&token, keybinds)?)
        }
        other => anyhow::bail!("fzf returned an unknown padded response kind {other:?}"),
    };

    let selected = lines
        .filter(|line| !line.is_empty())
        .map(|line| {
            let index = line
                .parse::<usize>()
                .map_err(|error| anyhow::anyhow!("fzf returned an invalid item index: {error}"))?;
            items
                .get(index)
                .ok_or_else(|| anyhow::anyhow!("fzf returned out-of-range item index {index}"))
        })
        .filter_map(|result| match result {
            Ok(item) if item.fzf_is_selectable() => Some(Ok(item.clone())),
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(DialogOutcome::Submitted(MenuSelection {
        items: selected,
        action,
    }))
}

fn padded_preview_command(manifest: &std::path::Path) -> String {
    format!(
        "row=$(sed -n \"$(({{n}} + 1))p\" {manifest}); \
         kind=$(printf '%s' \"$row\" | cut -f1); \
         content=$(printf '%s' \"$row\" | cut -f2 | base64 -d); \
         if [ \"$kind\" = C ]; then \
             key=$(printf '%s' \"$row\" | cut -f3 | base64 -d); \
             printf '%s' \"$content\" | bash -s -- \"$key\"; \
         elif [ \"$kind\" = T ]; then printf '%s' \"$content\"; fi",
        manifest = crate::common::shell::shell_quote(&manifest.display().to_string())
    )
}

#[cfg(test)]
mod mock_tests {
    use base64::{Engine as _, engine::general_purpose};

    use crate::menu_utils::MockQueue;
    use crate::menu_utils::{
        DialogOutcome, FzfPreview, FzfSelectable, MenuKey, MenuKeybind, MenuSelection,
    };

    #[derive(Clone)]
    struct Item {
        label: &'static str,
        selectable: bool,
    }

    #[derive(Clone)]
    struct PreviewItem {
        label: &'static str,
        preview: FzfPreview,
    }

    impl FzfSelectable for PreviewItem {
        fn fzf_display_text(&self) -> String {
            self.label.to_string()
        }

        fn fzf_preview(&self) -> FzfPreview {
            self.preview.clone()
        }
    }

    impl FzfSelectable for Item {
        fn fzf_display_text(&self) -> String {
            self.label.to_string()
        }

        fn fzf_is_selectable(&self) -> bool {
            self.selectable
        }
    }

    #[test]
    fn padded_presentation_returns_selected_item() {
        let _guard = MockQueue::new().select_index(0).guard();
        let items = vec!["first".to_string(), "second".to_string()];
        let result = crate::menu_utils::FzfWrapper::builder()
            .items(items)
            .padded()
            .select()
            .unwrap();
        match result {
            crate::menu_utils::DialogOutcome::Submitted(s) => {
                assert_eq!(s.items, vec!["first".to_string()])
            }
            other => panic!("Expected Submitted, got {other:?}"),
        }
    }

    #[test]
    fn padded_stream_resolves_drained_items() {
        let _guard = MockQueue::new().select_index(0).guard();
        let (tx, rx) = crossbeam_channel::unbounded::<String>();
        tx.send("first".to_string()).unwrap();
        tx.send("second".to_string()).unwrap();
        drop(tx);

        let result = crate::menu_utils::FzfWrapper::builder()
            .stream(rx)
            .padded()
            .select()
            .unwrap();
        match result {
            crate::menu_utils::DialogOutcome::Submitted(s) => {
                assert_eq!(s.items, vec!["first".to_string()])
            }
            other => panic!("Expected Submitted, got {other:?}"),
        }
    }

    #[test]
    fn padded_stream_select_one_unwraps_item() {
        let _guard = MockQueue::new().select_index(1).guard();
        let (tx, rx) = crossbeam_channel::unbounded::<String>();
        tx.send("first".to_string()).unwrap();
        tx.send("second".to_string()).unwrap();
        drop(tx);

        let result = crate::menu_utils::FzfWrapper::builder()
            .stream(rx)
            .padded()
            .select_one()
            .unwrap();
        match result {
            crate::menu_utils::DialogOutcome::Submitted(item) => assert_eq!(item, "second"),
            other => panic!("Expected Submitted, got {other:?}"),
        }
    }

    #[test]
    fn padded_stream_invokes_on_ready_before_resolving() {
        let _guard = MockQueue::new().select_index(0).guard();
        let (tx, rx) = crossbeam_channel::unbounded::<String>();
        tx.send("only".to_string()).unwrap();
        drop(tx);
        let (ready_tx, ready_rx) = crossbeam_channel::bounded::<()>(1);

        let result = crate::menu_utils::FzfWrapper::builder()
            .stream(rx)
            .padded()
            .on_ready(move || {
                let _ = ready_tx.try_send(());
                Ok(())
            })
            .select()
            .unwrap();
        match result {
            crate::menu_utils::DialogOutcome::Submitted(s) => {
                assert_eq!(s.items, vec!["only".to_string()])
            }
            other => panic!("Expected Submitted, got {other:?}"),
        }
        assert!(
            ready_rx.try_recv().is_ok(),
            "on_ready must run even under the mock"
        );
    }

    #[test]
    fn padded_previews_share_one_manifest() {
        let items = vec![
            PreviewItem {
                label: "text",
                preview: FzfPreview::Text("hello\nworld".to_string()),
            },
            PreviewItem {
                label: "command",
                preview: FzfPreview::Command("printf command".to_string()),
            },
            PreviewItem {
                label: "none",
                preview: FzfPreview::None,
            },
        ];

        let manifest = super::prepare_padded_preview_manifest(&items).unwrap();
        assert!(manifest.path().is_file());

        let rows = std::fs::read_to_string(manifest.path()).unwrap();
        let rows = rows.lines().collect::<Vec<_>>();
        assert_eq!(rows.len(), items.len());
        assert!(rows[0].starts_with("T\t"));
        assert!(rows[1].starts_with("C\t"));
        assert!(rows[2].starts_with("N\t"));
    }

    #[test]
    fn padded_preview_command_reads_selected_row_and_passes_key() {
        let items = vec![
            PreviewItem {
                label: "text",
                preview: FzfPreview::Text("first\npreview".to_string()),
            },
            PreviewItem {
                label: "command-key",
                preview: FzfPreview::Command("printf 'command:%s' \"$1\"".to_string()),
            },
            PreviewItem {
                label: "none",
                preview: FzfPreview::None,
            },
        ];
        let manifest = super::prepare_padded_preview_manifest(&items).unwrap();
        let command = super::padded_preview_command(manifest.path());

        let run_row = |index: usize| {
            std::process::Command::new("bash")
                .arg("-c")
                .arg(command.replace("{n}", &index.to_string()))
                .output()
                .unwrap()
        };

        let text = run_row(0);
        assert!(text.status.success());
        assert_eq!(text.stdout, b"first\npreview");

        let dynamic = run_row(1);
        assert!(dynamic.status.success());
        assert_eq!(dynamic.stdout, b"command:command-key");

        let none = run_row(2);
        assert!(none.status.success());
        assert!(none.stdout.is_empty());
    }

    #[test]
    fn initial_cursor_skips_non_selectable_rows() {
        let items = vec![
            Item {
                label: "header",
                selectable: false,
            },
            Item {
                label: "first action",
                selectable: true,
            },
            Item {
                label: "second action",
                selectable: true,
            },
        ];

        assert_eq!(super::nearest_selectable_index(&items, None), Some(1));
        assert_eq!(super::nearest_selectable_index(&items, Some(0)), Some(1));
        assert_eq!(super::nearest_selectable_index(&items, Some(2)), Some(2));
    }

    #[test]
    fn no_cursor_exists_when_every_row_is_non_selectable() {
        let items = vec![Item {
            label: "header",
            selectable: false,
        }];

        assert_eq!(super::nearest_selectable_index(&items, None), None);
    }

    #[test]
    fn padded_response_decodes_multiple_indices() {
        let items = vec!["zero".to_string(), "one".to_string(), "two".to_string()];
        let response =
            super::parse_padded_response::<_, ()>(b"submit\n0\n2\n", &items, &[]).unwrap();

        assert_eq!(
            response,
            DialogOutcome::Submitted(MenuSelection {
                items: vec!["zero".to_string(), "two".to_string()],
                action: None,
            })
        );
    }

    #[test]
    fn padded_response_decodes_typed_action_without_an_item() {
        let binds = [MenuKeybind::new(
            MenuKey::new("ctrl-e").unwrap(),
            "edit",
            42,
        )];
        let token = general_purpose::STANDARD.encode("ctrl-e");
        let response = super::parse_padded_response(
            format!("action\n{token}\n").as_bytes(),
            &["zero".to_string()],
            &binds,
        )
        .unwrap();

        assert_eq!(
            response,
            DialogOutcome::Submitted(MenuSelection {
                items: Vec::new(),
                action: Some(42),
            })
        );
    }

    #[test]
    fn padded_response_drops_non_selectable_rows() {
        let items = vec![
            Item {
                label: "header",
                selectable: false,
            },
            Item {
                label: "action",
                selectable: true,
            },
        ];
        let response =
            super::parse_padded_response::<_, ()>(b"submit\n0\n1\n", &items, &[]).unwrap();

        let DialogOutcome::Submitted(selection) = response else {
            panic!("expected submitted selection");
        };
        assert_eq!(selection.items.len(), 1);
        assert_eq!(selection.items[0].label, "action");
    }

    #[test]
    fn padded_command_enables_multi_and_emits_framed_actions() {
        let builder = super::FzfBuilder::new();
        let binds = [MenuKeybind::new(
            MenuKey::new("ctrl-e").unwrap(),
            "edit",
            (),
        )];
        let command =
            super::configure_padded_cmd(&builder.shared, None, false, false, &binds, true);
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert!(args.iter().any(|arg| arg == "--multi"));
        assert!(
            args.iter()
                .any(|arg| arg == "enter:become(printf '%s\\n' submit {+n})")
        );
        let encoded = general_purpose::STANDARD.encode("ctrl-e");
        assert!(args.iter().any(|arg| {
            arg == &format!("ctrl-e:become(printf '%s\\n' action {encoded} {{+n}})")
        }));
    }
}
