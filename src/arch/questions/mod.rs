use crate::arch::engine::{FlowKind, InstallContext, WizardEngine, WizardStep, read_audit};
use crate::menu_utils::{DialogOutcome, FzfSelectable, ItemSelection};
use anyhow::Result;

pub mod boolean;
pub mod console_font;
pub mod disk;
pub mod display_manager;
pub mod dualboot;
pub mod filesystem;
pub mod partition;
pub mod resize_instructions;
pub mod system;
pub mod text_input;
pub mod warnings;

/// Inputs that more than one step consumes, so no step can own them.
///
/// These are registered once per wizard via
/// [`crate::arch::engine::WizardEngine::with_data_sources`] and awaited by the
/// first step that declares one of their slots, rather than being attached to
/// one step and read behind its back from another. A source is only registered
/// when some step in the given flow actually declares what it publishes, so a
/// flow that asks none of the consumers never pays for the lookup.
///
/// A new shared input must be added here.
pub(crate) fn shared_data_sources(
    steps: &[Box<dyn WizardStep>],
) -> Vec<Box<dyn crate::arch::engine::AsyncDataProvider>> {
    use crate::arch::engine::{AsyncDataProvider, KeyId};
    use crate::arch::geo::{GeoLocationKey, GeoLocationProvider};
    use crate::arch::timezones::{TimezoneCountriesKey, TimezoneCountriesProvider};

    let consumers_of = |key: KeyId| {
        steps
            .iter()
            .filter(|step| {
                step.required_data_keys().contains(&key) || step.optional_data_keys().contains(&key)
            })
            .map(|step| step.id())
            .collect::<Vec<_>>()
    };

    let mut sources: Vec<Box<dyn AsyncDataProvider>> = Vec::new();
    let geo_consumers = consumers_of(KeyId::of::<GeoLocationKey>());
    if !geo_consumers.is_empty() {
        sources.push(Box::new(GeoLocationProvider::new(geo_consumers)));
    }
    if !consumers_of(KeyId::of::<TimezoneCountriesKey>()).is_empty() {
        sources.push(Box::new(TimezoneCountriesProvider));
    }
    sources
}

/// Build an application wizard with every shared source its steps consume.
/// Keep this as the single entry point for install, setup, and single-question
/// flows so none can silently lose a suggestion source.
pub(crate) fn wizard_engine(
    flow: FlowKind,
    steps: Vec<Box<dyn WizardStep>>,
) -> Result<WizardEngine> {
    let sources = shared_data_sources(&steps);
    let engine = match flow {
        FlowKind::Install => WizardEngine::new(steps)?,
        FlowKind::Setup => WizardEngine::for_flow(flow, steps)?,
    };
    Ok(engine.with_data_sources(sources))
}

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
pub use console_font::ConsoleFontQuestion;
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

#[cfg(test)]
mod tests {
    use super::{LocaleQuestion, MirrorRegionQuestion, TimezoneQuestion, shared_data_sources};
    use crate::arch::engine::{KeyId, WizardStep};
    use crate::arch::geo::GeoLocationKey;
    use crate::arch::timezones::TimezoneCountriesKey;

    #[test]
    fn single_question_flows_register_their_shared_suggestion_sources() {
        let cases: Vec<(Box<dyn WizardStep>, Vec<KeyId>)> = vec![
            (
                Box::new(MirrorRegionQuestion),
                vec![KeyId::of::<GeoLocationKey>()],
            ),
            (
                Box::new(TimezoneQuestion),
                vec![
                    KeyId::of::<GeoLocationKey>(),
                    KeyId::of::<TimezoneCountriesKey>(),
                ],
            ),
            (
                Box::new(LocaleQuestion),
                vec![
                    KeyId::of::<GeoLocationKey>(),
                    KeyId::of::<TimezoneCountriesKey>(),
                ],
            ),
        ];

        for (step, expected) in cases {
            let actual: Vec<_> = shared_data_sources(&[step])
                .iter()
                .flat_map(|provider| provider.publishes())
                .collect();
            assert_eq!(actual, expected);
        }
    }
}
