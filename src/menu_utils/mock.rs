//! Mock queue for testing FZF menu interactions.
//!
//! In tests, use `MockQueue::new().select_index(0).confirm_yes().guard()`
//! to intercept FZF calls with scripted responses.
//! The guard automatically clears the queue when dropped.
//!
//! ## Production build
//!
//! `pop_mock()` is `#[cfg(test)]`-only and call sites are guarded with
//! `#[cfg(test)]`, so production builds have zero mock overhead.

// ---------------------------------------------------------------------------
// pop_mock() — test-only, called from #[cfg(test)] blocks in fzf dialogs
// ---------------------------------------------------------------------------

/// Test implementation: pops from the thread-local queue.
#[cfg(test)]
pub(crate) fn pop_mock() -> Option<MockResponse> {
    MOCK_QUEUE.with(|cell| cell.borrow_mut().pop_front())
}

// ---------------------------------------------------------------------------
// Test-only items below
// ---------------------------------------------------------------------------

#[cfg(test)]
use std::cell::RefCell;
#[cfg(test)]
use std::collections::VecDeque;

#[cfg(test)]
use crate::menu_utils::fzf::{DialogOutcome, MenuKeybind, MenuSelection};

#[cfg(test)]
thread_local! {
    pub(crate) static MOCK_QUEUE: RefCell<VecDeque<MockResponse>> = const { RefCell::new(VecDeque::new()) };
}

#[cfg(test)]
#[derive(Debug)]
pub(crate) enum MockResponse {
    // Selection dialogs
    SelectIndex(usize),
    MultiSelectIndices(Vec<usize>),
    CancelSelection,
    /// A registered menu keybind was pressed; the token is matched against
    /// the menu's registered binds at resolve time.
    KeybindAction {
        token: String,
        selected_indices: Vec<usize>,
    },

    // Confirmation
    ConfirmYes,
    ConfirmNo,
    ConfirmCancelled,

    // Input
    InputString(String),
    InputCancelled,

    // Message
    MessageAck,

    // Password
    PasswordString(String),
    PasswordCancelled,

    // Checklist
    ChecklistConfirm(Vec<usize>),
    ChecklistAction(String),
    ChecklistCancelled,

    // File picker
    FilePickerSelected(String),
    FilePickerMultiSelected(Vec<String>),
    FilePickerCancelled,
}

// ---------------------------------------------------------------------------
// MockQueue builder (test-only)
// ---------------------------------------------------------------------------

#[cfg(test)]
pub struct MockQueue {
    responses: VecDeque<MockResponse>,
}

#[cfg(test)]
impl MockQueue {
    pub fn new() -> Self {
        Self {
            responses: VecDeque::new(),
        }
    }

    // -- Selection --

    pub fn select_index(mut self, index: usize) -> Self {
        self.responses.push_back(MockResponse::SelectIndex(index));
        self
    }

    pub fn multi_select(mut self, indices: Vec<usize>) -> Self {
        self.responses
            .push_back(MockResponse::MultiSelectIndices(indices));
        self
    }

    pub fn cancel_selection(mut self) -> Self {
        self.responses.push_back(MockResponse::CancelSelection);
        self
    }

    /// Script a keybind press over the given selection. `token` must match one
    /// of the menu's registered keys. For a single-select menu, pass the
    /// focused row as a one-element index vector; an empty vector models a
    /// keybind pressed while there are no matches.
    pub fn keybind_action(
        mut self,
        token: impl Into<String>,
        selected_indices: Vec<usize>,
    ) -> Self {
        self.responses.push_back(MockResponse::KeybindAction {
            token: token.into(),
            selected_indices,
        });
        self
    }

    // -- Confirmation --

    pub fn confirm_yes(mut self) -> Self {
        self.responses.push_back(MockResponse::ConfirmYes);
        self
    }

    pub fn confirm_no(mut self) -> Self {
        self.responses.push_back(MockResponse::ConfirmNo);
        self
    }

    pub fn confirm_cancelled(mut self) -> Self {
        self.responses.push_back(MockResponse::ConfirmCancelled);
        self
    }

    // -- Input --

    pub fn input_string(mut self, s: impl Into<String>) -> Self {
        self.responses
            .push_back(MockResponse::InputString(s.into()));
        self
    }

    pub fn input_cancelled(mut self) -> Self {
        self.responses.push_back(MockResponse::InputCancelled);
        self
    }

    // -- Message --

    pub fn message_ack(mut self) -> Self {
        self.responses.push_back(MockResponse::MessageAck);
        self
    }

    // -- Password --

    pub fn password(mut self, s: impl Into<String>) -> Self {
        self.responses
            .push_back(MockResponse::PasswordString(s.into()));
        self
    }

    pub fn password_cancelled(mut self) -> Self {
        self.responses.push_back(MockResponse::PasswordCancelled);
        self
    }

    // -- Checklist --

    pub fn checklist_confirm(mut self, indices: Vec<usize>) -> Self {
        self.responses
            .push_back(MockResponse::ChecklistConfirm(indices));
        self
    }

    pub fn checklist_action(mut self, key: impl Into<String>) -> Self {
        self.responses
            .push_back(MockResponse::ChecklistAction(key.into()));
        self
    }

    pub fn checklist_cancelled(mut self) -> Self {
        self.responses.push_back(MockResponse::ChecklistCancelled);
        self
    }

    // -- File picker --

    pub fn file_picker(mut self, path: impl Into<String>) -> Self {
        self.responses
            .push_back(MockResponse::FilePickerSelected(path.into()));
        self
    }

    pub fn file_picker_multi(mut self, paths: Vec<String>) -> Self {
        self.responses
            .push_back(MockResponse::FilePickerMultiSelected(paths));
        self
    }

    pub fn file_picker_cancelled(mut self) -> Self {
        self.responses.push_back(MockResponse::FilePickerCancelled);
        self
    }

    /// Install this queue into the thread-local and return the RAII guard.
    /// Panics if a queue is already active (no nesting allowed).
    pub fn guard(self) -> MockQueueGuard {
        MOCK_QUEUE.with(|cell| {
            let mut queue = cell.borrow_mut();
            assert!(
                queue.is_empty(),
                "MockQueueGuard: queue already has responses. Don't nest guards."
            );
            queue.extend(self.responses);
        });
        MockQueueGuard { _private: () }
    }
}

/// Resolve a selection-style response against `items`, shared by every
/// selection dialog so the index handling lives in one place.
///
/// `keybinds` are the binds the menu registered; a scripted
/// [`MockResponse::KeybindAction`] resolves its token against them so the
/// mocked action is the menu's real typed action. Panics when `response` is
/// not a selection response.
#[cfg(test)]
pub(crate) fn resolve_selection<T: Clone, A: Clone>(
    response: MockResponse,
    items: Vec<T>,
    keybinds: &[MenuKeybind<A>],
) -> DialogOutcome<MenuSelection<T, A>> {
    use crate::menu_utils::DialogOutcome;

    match response {
        MockResponse::SelectIndex(i) => DialogOutcome::Submitted(MenuSelection {
            items: vec![
                items
                    .into_iter()
                    .nth(i)
                    .unwrap_or_else(|| panic!("MockResponse::SelectIndex({i}) out of bounds")),
            ],
            action: None,
        }),
        MockResponse::MultiSelectIndices(indices) => DialogOutcome::Submitted(MenuSelection {
            items: indices
                .into_iter()
                .map(|i| {
                    items.get(i).cloned().unwrap_or_else(|| {
                        panic!("MockResponse::MultiSelectIndices({i}) out of bounds")
                    })
                })
                .collect(),
            action: None,
        }),
        MockResponse::KeybindAction {
            token,
            selected_indices,
        } => {
            let bind = keybinds
                .iter()
                .find(|bind| bind.key.as_str() == token)
                .unwrap_or_else(|| {
                    panic!(
                        "MockResponse::KeybindAction(\"{token}\") does not match any registered menu keybind"
                    )
                });
            DialogOutcome::Submitted(MenuSelection {
                items: selected_indices
                    .into_iter()
                    .map(|i| {
                        items.get(i).cloned().unwrap_or_else(|| {
                            panic!("MockResponse::KeybindAction index {i} out of bounds")
                        })
                    })
                    .collect(),
                action: Some(bind.action.clone()),
            })
        }
        MockResponse::CancelSelection => DialogOutcome::Cancelled,
        other => panic!("Mock: expected select response, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// MockQueueGuard (RAII, test-only)
// ---------------------------------------------------------------------------

#[cfg(test)]
pub struct MockQueueGuard {
    _private: (),
}

#[cfg(test)]
impl Drop for MockQueueGuard {
    fn drop(&mut self) {
        MOCK_QUEUE.with(|cell| cell.borrow_mut().clear());
    }
}
