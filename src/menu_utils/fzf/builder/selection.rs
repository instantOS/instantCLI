use anyhow::Result;
use serde::de::DeserializeOwned;

use super::{CommandSelection, ItemPresentation, ItemSelection, StreamSelection};
use crate::menu_utils::fzf::types::{
    DecodedStreamingMenuItem, DialogOutcome, FzfSelectable, MenuItem, MenuKeybind, MenuSelection,
};
use crate::menu_utils::fzf::wrapper::FzfWrapper;

impl<T, A> ItemSelection<T, A> {
    /// The items queued for this selection.
    pub fn items(&self) -> &[T] {
        &self.items
    }

    /// Start the cursor on the row at zero-based `index` when the menu opens.
    pub fn initial_index(self, index: usize) -> Self {
        Self {
            builder: self.builder.initial_index(index),
            items: self.items,
            keybinds: self.keybinds,
            presentation: self.presentation,
        }
    }

    /// Render spacious multiline rows instead of the default compact ones.
    ///
    /// Padded rows are prepared from the complete in-memory collection
    /// before fzf starts, together with a per-row preview manifest.
    pub fn padded(self) -> Self {
        Self {
            builder: self.builder,
            items: self.items,
            keybinds: self.keybinds,
            presentation: ItemPresentation::Padded,
        }
    }
}

impl<T> ItemSelection<T, ()> {
    /// Register typed actions without changing the selection source.
    pub fn keybinds<A: Clone>(self, keybinds: &[MenuKeybind<A>]) -> ItemSelection<T, A> {
        ItemSelection {
            builder: self.builder,
            items: self.items,
            keybinds: keybinds.to_vec(),
            presentation: self.presentation,
        }
    }
}

impl<T: FzfSelectable + Clone, A: Clone> ItemSelection<T, A> {
    /// Run a single-target menu while preserving its optional keybind action.
    pub fn select(self) -> Result<DialogOutcome<MenuSelection<T, A>>> {
        match self.presentation {
            ItemPresentation::Compact => {
                FzfWrapper::run_items(self.builder.shared, self.items, &self.keybinds, false)
            }
            ItemPresentation::Padded => {
                self.builder
                    .run_padded_items(self.items, &self.keybinds, false)
            }
        }
    }

    /// Run a menu in which the user may submit multiple items.
    pub fn select_many(self) -> Result<DialogOutcome<MenuSelection<T, A>>> {
        match self.presentation {
            ItemPresentation::Compact => {
                FzfWrapper::run_items(self.builder.shared, self.items, &self.keybinds, true)
            }
            ItemPresentation::Padded => {
                self.builder
                    .run_padded_items(self.items, &self.keybinds, true)
            }
        }
    }
}

impl<T: FzfSelectable + Clone> ItemSelection<T, ()> {
    /// Run a single-item menu and return the item directly.
    pub fn select_one(self) -> Result<DialogOutcome<T>> {
        single(self.select()?)
    }
}

impl<T: FzfSelectable + Clone> ItemSelection<MenuItem<T>, ()> {
    /// Run a grouped menu, reopening it if a pointer selects a separator.
    pub fn select_menu(mut self) -> Result<DialogOutcome<T>> {
        loop {
            match (ItemSelection {
                builder: self.builder.clone(),
                items: self.items.clone(),
                keybinds: Vec::<MenuKeybind<()>>::new(),
                presentation: self.presentation,
            })
            .select()?
            {
                DialogOutcome::Submitted(mut selection) => {
                    if selection.items.len() != 1 {
                        anyhow::bail!(
                            "expected exactly one selected menu entry, got {}",
                            selection.items.len()
                        );
                    }
                    match selection.items.pop().ok_or_else(|| {
                        anyhow::anyhow!("expected exactly one selected menu entry, got 0")
                    })? {
                        MenuItem::Entry(item) => return Ok(DialogOutcome::Submitted(item)),
                        MenuItem::Separator(_) => self.builder.shared.initial_cursor = None,
                    }
                }
                DialogOutcome::Cancelled => return Ok(DialogOutcome::Cancelled),
            }
        }
    }
}

impl<'a, T> StreamSelection<'a, T, ()> {
    /// Register typed actions without changing the selection source.
    pub fn keybinds<A: Clone>(self, keybinds: &[MenuKeybind<A>]) -> StreamSelection<'a, T, A> {
        StreamSelection {
            builder: self.builder,
            initial_items: self.initial_items,
            late_items: self.late_items,
            keybinds: keybinds.to_vec(),
            on_ready: self.on_ready,
            presentation: self.presentation,
        }
    }
}

impl<'a, T, A> StreamSelection<'a, T, A> {
    /// Add items that are available when the menu first opens.
    pub fn initial_items(mut self, items: Vec<T>) -> Self {
        self.initial_items = items;
        self
    }

    /// Render spacious multiline rows instead of the default compact ones,
    /// including rows that arrive while the menu is open.
    ///
    /// Each arriving item is appended to fzf's input together with its
    /// preview-manifest entry in the same pump step, so manifest line N
    /// always describes input row N — the index fzf reports back through
    /// its `{n}` placeholders. The padding machinery (selectable-row
    /// marker, keyword delimiter, preview manifest) is enabled
    /// unconditionally because it cannot be re-decided once fzf has
    /// spawned.
    ///
    /// No production menu streams padded rows yet; the runtime is
    /// exercised through the mock tests in `padded.rs`. Retire this
    /// allowance when the first live call site lands.
    #[allow(dead_code)]
    pub fn padded(self) -> Self {
        Self {
            builder: self.builder,
            initial_items: self.initial_items,
            late_items: self.late_items,
            keybinds: self.keybinds,
            on_ready: self.on_ready,
            presentation: ItemPresentation::Padded,
        }
    }

    pub(crate) fn on_ready<'b, F>(self, on_ready: F) -> StreamSelection<'b, T, A>
    where
        F: FnOnce() -> Result<()> + 'b,
    {
        StreamSelection {
            builder: self.builder,
            initial_items: self.initial_items,
            late_items: self.late_items,
            keybinds: self.keybinds,
            on_ready: Some(Box::new(on_ready)),
            presentation: self.presentation,
        }
    }
}

impl<T, A> StreamSelection<'_, T, A>
where
    T: FzfSelectable + Clone + Send + 'static,
    A: Clone,
{
    pub fn select(self) -> Result<DialogOutcome<MenuSelection<T, A>>> {
        self.run(false)
    }

    pub fn select_many(self) -> Result<DialogOutcome<MenuSelection<T, A>>> {
        self.run(true)
    }

    fn run(self, allow_multiple: bool) -> Result<DialogOutcome<MenuSelection<T, A>>> {
        match self.presentation {
            ItemPresentation::Compact => FzfWrapper::run_stream(
                self.builder.shared,
                self.initial_items,
                self.late_items,
                &self.keybinds,
                allow_multiple,
                self.on_ready,
            ),
            ItemPresentation::Padded => self.builder.run_padded_stream(
                self.initial_items,
                self.late_items,
                &self.keybinds,
                allow_multiple,
                self.on_ready,
            ),
        }
    }
}

impl<T> StreamSelection<'_, T, ()>
where
    T: FzfSelectable + Clone + Send + 'static,
{
    pub fn select_one(self) -> Result<DialogOutcome<T>> {
        single(self.select()?)
    }
}

impl<T> CommandSelection<T, ()> {
    /// Register typed actions without changing the selection source.
    // Kept as part of the intentionally uniform source API even though no
    // command-backed menu currently registers an action.
    #[allow(dead_code)]
    pub fn keybinds<A: Clone>(self, keybinds: &[MenuKeybind<A>]) -> CommandSelection<T, A> {
        CommandSelection {
            builder: self.builder,
            command: self.command,
            initial_rows: self.initial_rows,
            keybinds: keybinds.to_vec(),
            payload: self.payload,
        }
    }
}

impl<T, A> CommandSelection<T, A> {
    /// Add already encoded rows before rows emitted by the command.
    pub fn initial_rows(mut self, rows: impl Into<String>) -> Self {
        self.initial_rows = rows.into();
        self
    }
}

impl<T: DeserializeOwned, A: Clone> CommandSelection<T, A> {
    pub fn select(self) -> Result<DialogOutcome<MenuSelection<DecodedStreamingMenuItem<T>, A>>> {
        self.run(false)
    }

    pub fn select_many(
        self,
    ) -> Result<DialogOutcome<MenuSelection<DecodedStreamingMenuItem<T>, A>>> {
        self.run(true)
    }

    fn run(
        self,
        allow_multiple: bool,
    ) -> Result<DialogOutcome<MenuSelection<DecodedStreamingMenuItem<T>, A>>> {
        FzfWrapper::run_command(
            self.builder.shared,
            self.command,
            &self.initial_rows,
            &self.keybinds,
            allow_multiple,
        )
    }
}

impl<T: DeserializeOwned> CommandSelection<T, ()> {
    pub fn select_one(self) -> Result<DialogOutcome<DecodedStreamingMenuItem<T>>> {
        single(self.select()?)
    }
}

fn single<T>(outcome: DialogOutcome<MenuSelection<T>>) -> Result<DialogOutcome<T>> {
    match outcome {
        DialogOutcome::Submitted(selection) => {
            Ok(DialogOutcome::Submitted(selection.into_single()?))
        }
        DialogOutcome::Cancelled => Ok(DialogOutcome::Cancelled),
    }
}
