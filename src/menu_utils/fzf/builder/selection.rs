use anyhow::Result;
use serde::de::DeserializeOwned;

use super::{CommandSelection, ItemSelection, StreamSelection};
use crate::menu_utils::fzf::types::{
    DecodedStreamingMenuItem, DialogOutcome, FzfSelectable, MenuItem, MenuKeybind,
    MenuPresentation, MenuSelection,
};
use crate::menu_utils::fzf::wrapper::FzfWrapper;

impl<T> ItemSelection<T, ()> {
    /// Register typed actions without changing the selection source.
    pub fn keybinds<A: Clone>(self, keybinds: &[MenuKeybind<A>]) -> ItemSelection<T, A> {
        ItemSelection {
            builder: self.builder,
            items: self.items,
            keybinds: keybinds.to_vec(),
        }
    }
}

impl<T: FzfSelectable + Clone, A: Clone> ItemSelection<T, A> {
    /// Run a single-target menu while preserving its optional keybind action.
    pub fn select(self) -> Result<DialogOutcome<MenuSelection<T, A>>> {
        match self.builder.shared.presentation {
            MenuPresentation::Compact => {
                FzfWrapper::from_builder(self.builder).run_items(self.items, &self.keybinds, false)
            }
            MenuPresentation::Padded => {
                self.builder
                    .run_padded_items(self.items, &self.keybinds, false)
            }
        }
    }

    /// Run a menu in which the user may submit multiple items.
    pub fn select_many(self) -> Result<DialogOutcome<MenuSelection<T, A>>> {
        match self.builder.shared.presentation {
            MenuPresentation::Compact => {
                FzfWrapper::from_builder(self.builder).run_items(self.items, &self.keybinds, true)
            }
            MenuPresentation::Padded => {
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
        }
    }
}

impl<'a, T, A> StreamSelection<'a, T, A> {
    /// Add items that are available when the menu first opens.
    pub fn initial_items(mut self, items: Vec<T>) -> Self {
        self.initial_items = items;
        self
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
        ensure_streamable(self.builder.shared.presentation, "channel")?;
        FzfWrapper::from_builder(self.builder).run_stream(
            self.initial_items,
            self.late_items,
            &self.keybinds,
            allow_multiple,
            self.on_ready,
        )
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
        ensure_streamable(self.builder.shared.presentation, "command")?;
        FzfWrapper::from_builder(self.builder).run_command(
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

fn ensure_streamable(presentation: MenuPresentation, source: &str) -> Result<()> {
    if presentation != MenuPresentation::Compact {
        anyhow::bail!(
            "padded presentation requires a complete item collection; {source} streaming sources must use compact presentation"
        );
    }
    Ok(())
}

fn single<T>(outcome: DialogOutcome<MenuSelection<T>>) -> Result<DialogOutcome<T>> {
    match outcome {
        DialogOutcome::Submitted(selection) => {
            Ok(DialogOutcome::Submitted(selection.into_single()?))
        }
        DialogOutcome::Cancelled => Ok(DialogOutcome::Cancelled),
    }
}
