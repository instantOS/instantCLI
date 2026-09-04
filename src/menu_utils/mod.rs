//! In-process interactive UI primitives for InstantCLI features.
//!
//! Use [`FzfWrapper`] when a feature should render in the caller's current
//! terminal. Use [`crate::menu::client::HostedMenuClient`] when the interaction
//! should be hosted outside that terminal. User cancellation is returned as a
//! [`DialogOutcome`]; renderer failures are returned through `anyhow::Result`.

mod cursor;
mod file_picker;
mod fzf;
mod keychord;
mod mock;
mod path_input;
mod slider;
mod text_input;

pub use crate::ui::preview::FzfPreview;
pub use cursor::MenuCursor;
pub use file_picker::{FilePickerResult, FilePickerScope, MenuWrapper};
pub use fzf::{
    ChecklistAction, ChecklistResult, ConfirmResult, DecodedStreamingMenuItem, DialogOutcome,
    FzfResult, FzfSelectable, FzfWrapper, Header, HeaderBuilder, MenuItem, MenuPresentation,
    StreamingCommand, StreamingMenuItem, default_fzf_key,
};
pub use keychord::{KeyChord, KeyChordAction, KeyChordChild, KeyChordNode};
pub use path_input::{PathInputBuilder, PathInputSelection, SuggestionProducer};
pub use slider::{SliderCommand, SliderConfig};
pub use text_input::{TextEditOutcome, TextEditPrompt, prompt_text_edit};

#[cfg(test)]
pub use mock::MockQueue;
