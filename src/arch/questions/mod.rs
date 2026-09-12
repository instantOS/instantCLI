use crate::arch::engine::{InstallContext, WizardStep, read_audit};
use crate::menu_utils::{DialogOutcome, FzfSelectable, ItemSelection};
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

/// Present a select-one dialog for a wizard step.
///
/// The cursor starts on the previous answer when it remains selectable, then
/// falls back to the step's [`WizardStep::preselect_answer`]. Items are matched
/// by [`FzfSelectable::fzf_key`], the machine-readable value stored as the
/// step's answer. With neither match, the selection uses its normal initial row.
pub(crate) fn select_one_for_step<T: FzfSelectable + Clone>(
    context: &InstallContext,
    step: &dyn WizardStep,
    selection: ItemSelection<T>,
) -> Result<DialogOutcome<T>> {
    let preselect = context
        .previous_answer(&step.id())
        .cloned()
        .or_else(|| read_audit::hook(step, "preselect_answer", || step.preselect_answer(context)));
    let index = preselect.and_then(|answer| {
        selection
            .items()
            .iter()
            .position(|item| item.fzf_key() == answer)
    });

    match index {
        Some(index) => selection.initial_index(index).select_one(),
        None => selection.select_one(),
    }
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
