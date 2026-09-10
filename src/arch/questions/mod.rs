use crate::arch::engine::{InstallContext, StepId};
use crate::menu_utils::{DialogOutcome, FzfBuilder, FzfSelectable};
use anyhow::Result;

pub mod boolean;
pub mod disk;
pub mod display_manager;
pub mod dualboot;
pub mod filesystem;
pub mod partition;
pub mod resize_instructions;
pub mod system;
pub mod text_input;
pub mod warnings;

/// Present a select-one dialog whose cursor starts on the row the user chose
/// the last time this question was answered, if that choice is still among
/// `items`.
///
/// Items are matched by [`FzfSelectable::fzf_key`], the same machine-readable
/// value stored as the step's answer. When there is no previous answer or it
/// no longer exists in the list, the dialog behaves exactly like
/// [`FzfBuilder::items`] followed by `select_one`.
pub(crate) fn select_one_with_preselect<T: FzfSelectable + Clone>(
    context: &InstallContext,
    id: StepId,
    builder: FzfBuilder,
    items: Vec<T>,
) -> Result<DialogOutcome<T>> {
    select_one_preselecting(context.previous_answer(&id).cloned(), builder, items)
}

/// Like [`select_one_with_preselect`], but with an explicit target value.
///
/// Questions use this to fall back to a sensible guess when no answer has
/// been recorded yet — e.g. placing the cursor on the system's detected
/// timezone.
pub(crate) fn select_one_preselecting<T: FzfSelectable + Clone>(
    preselect: Option<String>,
    builder: FzfBuilder,
    items: Vec<T>,
) -> Result<DialogOutcome<T>> {
    let index = preselect.and_then(|answer| items.iter().position(|item| item.fzf_key() == answer));

    let builder = match index {
        Some(index) => builder.initial_index(index),
        None => builder,
    };
    builder.items(items).select_one()
}

// Re-exports
pub use boolean::BooleanQuestion;
pub use disk::{DiskQuestion, PartitioningMethodQuestion, PrepareDiskStep, RunCfdiskStep};
pub use display_manager::DisplayManagerQuestion;
pub use dualboot::{DualBootPartitionQuestion, DualBootSizeQuestion};
pub use filesystem::{BtrfsCompressionQuestion, RootFilesystemQuestion};
pub use partition::{EspPartitionValidator, PartitionSelectorQuestion};
pub use resize_instructions::ResizeWorkflowStep;
pub use system::{
    DesktopEnvironmentQuestion, EncryptionPasswordQuestion, KernelQuestion, KeymapQuestion,
    LocaleQuestion, MirrorRegionQuestion, PasswordQuestion, TimezoneQuestion, hostname_question,
    username_question,
};
pub use warnings::{DualBootEspWarning, VirtualBoxWarning, WeakPasswordWarning};
