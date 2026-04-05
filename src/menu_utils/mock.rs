//! Mock queue for testing FZF menu interactions.
//!
//! In tests, use `MockQueue::new().select_index(0).confirm_yes().guard()`
//! to intercept FZF calls with scripted responses.
//! The guard automatically clears the queue when dropped.
//!
//! ## Production build
//!
//! `pop_mock()` returns `Option<Infallible>` which can never be `Some`.
//! The compiler elides the entire `if let Some(resp) = pop_mock()` block.

use std::cell::RefCell;
use std::collections::VecDeque;

// ---------------------------------------------------------------------------
// pop_mock() — the function called from production code
// ---------------------------------------------------------------------------

/// Production stub: returns `Option<Infallible>` which can never be `Some`.
/// The compiler elides the entire `if let Some(resp) = pop_mock()` block.
#[cfg(not(test))]
pub(crate) fn pop_mock() -> Option<std::convert::Infallible> {
    None
}

/// Test implementation: pops from the thread-local queue.
#[cfg(test)]
pub(crate) fn pop_mock() -> Option<MockResponse> {
    MOCK_QUEUE.with(|cell| cell.borrow_mut().pop_front())
}

// ---------------------------------------------------------------------------
// Test-only items below
// ---------------------------------------------------------------------------

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
