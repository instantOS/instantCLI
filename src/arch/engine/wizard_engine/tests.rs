use anyhow::Result;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::step_graph::StepGraph;
use super::{FlowKind, WizardEngine};
use crate::arch::engine::{
    AsyncDataProvider, DataKey, InstallContext, StepId, StepOutcome, WizardStep,
};
use crate::arch::questions::{
    BooleanQuestion, EncryptionPasswordQuestion, PartitioningMethodQuestion,
};
use crate::menu_utils::MockQueue;

struct StubOptionalQuestion {
    id: StepId,
    default: Option<String>,
}

#[async_trait::async_trait]
impl WizardStep for StubOptionalQuestion {
    fn id(&self) -> StepId {
        self.id
    }

    async fn run(&self, _context: &InstallContext) -> Result<StepOutcome> {
        Ok(StepOutcome::Answer("answered".to_string()))
    }

    fn is_optional(&self) -> bool {
        true
    }

    fn get_default(&self, _context: &InstallContext) -> Option<String> {
        self.default.clone()
    }
}

struct StubDependentQuestion {
    id: StepId,
    dependencies: Vec<StepId>,
}

#[async_trait::async_trait]
impl WizardStep for StubDependentQuestion {
    fn id(&self) -> StepId {
        self.id
    }

    async fn run(&self, _context: &InstallContext) -> Result<StepOutcome> {
        Ok(StepOutcome::Answer("answered".to_string()))
    }

    fn depends_on(&self) -> &[StepId] {
        &self.dependencies
    }
}

fn question(id: StepId, dependencies: &[StepId]) -> Box<dyn WizardStep> {
    Box::new(StubDependentQuestion {
        id,
        dependencies: dependencies.to_vec(),
    })
}

struct CompletingStep {
    id: StepId,
    dependencies: Vec<StepId>,
}

#[async_trait::async_trait]
impl WizardStep for CompletingStep {
    fn id(&self) -> StepId {
        self.id
    }

    async fn run(&self, _context: &InstallContext) -> Result<StepOutcome> {
        Ok(StepOutcome::Completed)
    }

    fn depends_on(&self) -> &[StepId] {
        &self.dependencies
    }
}

struct RetryOnceStep {
    attempts: AtomicUsize,
}

#[async_trait::async_trait]
impl WizardStep for RetryOnceStep {
    fn id(&self) -> StepId {
        StepId::PrepareDisk
    }

    async fn run(&self, _context: &InstallContext) -> Result<StepOutcome> {
        if self.attempts.fetch_add(1, Ordering::SeqCst) == 0 {
            Ok(StepOutcome::Retry("not ready".to_string()))
        } else {
            Ok(StepOutcome::Completed)
        }
    }
}

struct BackStep;

#[async_trait::async_trait]
impl WizardStep for BackStep {
    fn id(&self) -> StepId {
        StepId::PrepareDisk
    }

    async fn run(&self, _context: &InstallContext) -> Result<StepOutcome> {
        Ok(StepOutcome::back())
    }

    fn depends_on(&self) -> &[StepId] {
        &[StepId::Disk]
    }
}

struct StaleCompletionStep;

#[async_trait::async_trait]
impl WizardStep for StaleCompletionStep {
    fn id(&self) -> StepId {
        StepId::PrepareDisk
    }

    async fn run(&self, _context: &InstallContext) -> Result<StepOutcome> {
        Ok(StepOutcome::Completed)
    }

    fn completion_is_current(&self, _context: &InstallContext) -> bool {
        false
    }
}

#[tokio::test]
async fn completion_is_tracked_without_a_synthetic_answer() {
    let mut engine = WizardEngine::new(vec![Box::new(CompletingStep {
        id: StepId::LowRamWarning,
        dependencies: vec![],
    })])
    .unwrap();

    assert!(matches!(
        engine.run_step(0).await.unwrap(),
        super::StepInteraction::Completed
    ));
    assert!(engine.context.is_step_completed(StepId::LowRamWarning));
    assert!(engine.context.get_answer(&StepId::LowRamWarning).is_none());
    assert_eq!(engine.find_next_step_index(), None);
}

#[tokio::test]
async fn retry_displays_feedback_and_repeats_the_same_step() {
    let mut engine = WizardEngine::new(vec![Box::new(RetryOnceStep {
        attempts: AtomicUsize::new(0),
    })])
    .unwrap();
    let _mock = MockQueue::new().message_ack().guard();

    assert!(matches!(
        engine.run_step(0).await.unwrap(),
        super::StepInteraction::Completed
    ));
    assert!(engine.context.is_step_completed(StepId::PrepareDisk));
}

#[test]
fn stale_external_completion_is_rechecked_and_scheduled_again() {
    let mut engine = WizardEngine::new(vec![Box::new(StaleCompletionStep)]).unwrap();
    engine
        .step_graph
        .record_completion(&mut engine.context, StepId::PrepareDisk);

    assert_eq!(engine.find_next_step_index(), Some(0));
    assert!(!engine.context.is_step_completed(StepId::PrepareDisk));
}

#[tokio::test]
async fn explicit_back_skips_the_pause_menu_and_invalidates_the_previous_step() {
    let mut engine =
        WizardEngine::new(vec![question(StepId::Disk, &[]), Box::new(BackStep)]).unwrap();
    engine
        .step_graph
        .record_answer(&mut engine.context, StepId::Disk, "/dev/sda".to_string());

    assert!(matches!(
        engine.run_step(1).await.unwrap(),
        super::StepInteraction::Back { message: None }
    ));
    engine.go_back_from(1);

    assert!(engine.context.get_answer(&StepId::Disk).is_none());
    assert_eq!(engine.find_next_step_index(), Some(0));
}

#[test]
fn revisit_targets_an_earlier_step_and_invalidates_its_dependents() {
    let mut engine = WizardEngine::new(vec![
        question(StepId::Disk, &[]),
        question(StepId::PartitioningMethod, &[StepId::Disk]),
        Box::new(CompletingStep {
            id: StepId::RunCfdisk,
            dependencies: vec![StepId::Disk, StepId::PartitioningMethod],
        }),
        question(
            StepId::RootPartition,
            &[StepId::Disk, StepId::PartitioningMethod, StepId::RunCfdisk],
        ),
    ])
    .unwrap();
    engine
        .step_graph
        .record_answer(&mut engine.context, StepId::Disk, "/dev/sda".into());
    engine.step_graph.record_answer(
        &mut engine.context,
        StepId::PartitioningMethod,
        "Manual (cfdisk)".into(),
    );
    engine
        .step_graph
        .record_completion(&mut engine.context, StepId::RunCfdisk);
    engine.step_graph.record_answer(
        &mut engine.context,
        StepId::RootPartition,
        "/dev/sda2".into(),
    );

    engine.revisit_from(3, StepId::PartitioningMethod).unwrap();

    assert!(
        engine
            .context
            .get_answer(&StepId::PartitioningMethod)
            .is_none()
    );
    assert!(!engine.context.is_step_completed(StepId::RunCfdisk));
    assert!(engine.context.get_answer(&StepId::RootPartition).is_none());
    assert_eq!(engine.find_next_step_index(), Some(1));
}

#[test]
fn revisit_rejects_forward_navigation() {
    let mut engine = WizardEngine::new(vec![
        question(StepId::Disk, &[]),
        question(StepId::PartitioningMethod, &[StepId::Disk]),
    ])
    .unwrap();

    let error = engine
        .revisit_from(0, StepId::PartitioningMethod)
        .unwrap_err();

    assert!(error.to_string().contains("must precede"));
}

struct FailingProvider;

#[async_trait::async_trait]
impl AsyncDataProvider for FailingProvider {
    async fn provide(&self, _context: &InstallContext) -> Result<()> {
        anyhow::bail!("provider exploded")
    }
}

struct ProviderBackedQuestion;

#[async_trait::async_trait]
impl WizardStep for ProviderBackedQuestion {
    fn id(&self) -> StepId {
        StepId::Locale
    }

    fn required_data_keys(&self) -> Vec<String> {
        vec!["missing_provider_data".to_string()]
    }

    async fn run(&self, _context: &InstallContext) -> Result<StepOutcome> {
        Ok(StepOutcome::Answer("answer".to_string()))
    }

    fn data_providers(&self) -> Vec<Box<dyn AsyncDataProvider>> {
        vec![Box::new(FailingProvider)]
    }
}

struct ProviderReadyKey;

impl DataKey for ProviderReadyKey {
    type Value = bool;
    const KEY: &'static str = "provider_ready";
}

struct ProviderSkipKey;

impl DataKey for ProviderSkipKey {
    type Value = bool;
    const KEY: &'static str = "provider_skip";
}

struct SkippingProvider;

#[async_trait::async_trait]
impl AsyncDataProvider for SkippingProvider {
    async fn provide(&self, context: &InstallContext) -> Result<()> {
        tokio::task::yield_now().await;
        context.set::<ProviderSkipKey>(true);
        context.set::<ProviderReadyKey>(true);
        Ok(())
    }
}

struct ProviderSkippedQuestion;

#[async_trait::async_trait]
impl WizardStep for ProviderSkippedQuestion {
    fn id(&self) -> StepId {
        StepId::MirrorRegion
    }

    fn required_data_keys(&self) -> Vec<String> {
        vec![ProviderReadyKey::KEY.to_string()]
    }

    fn should_ask(&self, context: &InstallContext) -> bool {
        !context.get::<ProviderSkipKey>().unwrap_or(false)
    }

    async fn run(&self, _context: &InstallContext) -> Result<StepOutcome> {
        panic!("an irrelevant provider-backed question must not be asked")
    }

    fn data_providers(&self) -> Vec<Box<dyn AsyncDataProvider>> {
        vec![Box::new(SkippingProvider)]
    }
}

struct IncompleteProvider;

#[async_trait::async_trait]
impl AsyncDataProvider for IncompleteProvider {
    async fn provide(&self, _context: &InstallContext) -> Result<()> {
        Ok(())
    }
}

struct IncompleteProviderQuestion;

#[async_trait::async_trait]
impl WizardStep for IncompleteProviderQuestion {
    fn id(&self) -> StepId {
        StepId::Timezone
    }

    fn required_data_keys(&self) -> Vec<String> {
        vec![ProviderReadyKey::KEY.to_string()]
    }

    async fn run(&self, _context: &InstallContext) -> Result<StepOutcome> {
        panic!("a question without its required data must not be asked")
    }

    fn data_providers(&self) -> Vec<Box<dyn AsyncDataProvider>> {
        vec![Box::new(IncompleteProvider)]
    }
}

#[test]
fn install_flow_applies_optional_defaults_without_asking() {
    let mut engine = WizardEngine::for_flow(
        FlowKind::Install,
        vec![Box::new(StubOptionalQuestion {
            id: StepId::Autologin,
            default: Some("no".to_string()),
        })],
    )
    .unwrap();

    assert_eq!(engine.find_next_step_index(), None);
    assert_eq!(
        engine
            .context
            .get_answer(&StepId::Autologin)
            .map(String::as_str),
        Some("no")
    );
}

#[test]
fn setup_flow_asks_optional_questions_in_main_flow() {
    let mut engine = WizardEngine::for_flow(
        FlowKind::Setup,
        vec![Box::new(StubOptionalQuestion {
            id: StepId::Autologin,
            default: Some("no".to_string()),
        })],
    )
    .unwrap();

    assert_eq!(engine.find_next_step_index(), Some(0));
    assert!(engine.context.get_answer(&StepId::Autologin).is_none());
}

#[tokio::test]
async fn provider_failures_return_an_error_instead_of_waiting_forever() {
    let mut engine = WizardEngine::new(vec![Box::new(ProviderBackedQuestion)]).unwrap();
    let mut providers = engine.start_providers();
    let _mock = MockQueue::new().message_ack().guard();

    let error = engine
        .wait_until_ready(0, &mut providers)
        .await
        .unwrap_err();

    assert!(error.to_string().contains("provider exploded"));
    assert!(error.to_string().contains("Locale"));
}

#[tokio::test]
async fn provider_can_make_a_selected_question_irrelevant() {
    let mut engine = WizardEngine::new(vec![Box::new(ProviderSkippedQuestion)]).unwrap();
    let mut providers = engine.start_providers();

    assert_eq!(engine.find_next_step_index(), Some(0));
    assert!(matches!(
        engine.wait_until_ready(0, &mut providers).await.unwrap(),
        super::StepReadiness::Irrelevant
    ));
    assert_eq!(engine.find_next_step_index(), None);
}

#[tokio::test]
async fn settling_providers_removes_an_answer_that_becomes_irrelevant() {
    let mut engine = WizardEngine::new(vec![Box::new(ProviderSkippedQuestion)]).unwrap();
    engine
        .context
        .set_answer(StepId::MirrorRegion, "Germany".to_string());
    engine.normalize_context();
    let mut providers = engine.start_providers();

    assert_eq!(engine.find_next_step_index(), None);
    providers.finish_all().await;
    engine.normalize_context();

    assert!(engine.context.get_answer(&StepId::MirrorRegion).is_none());
}

#[tokio::test]
async fn provider_completion_without_required_data_returns_an_error() {
    let mut engine = WizardEngine::new(vec![Box::new(IncompleteProviderQuestion)]).unwrap();
    let mut providers = engine.start_providers();
    let _mock = MockQueue::new().message_ack().guard();

    let error = engine
        .wait_until_ready(0, &mut providers)
        .await
        .unwrap_err();

    assert!(error.to_string().contains("completed without supplying"));
    assert!(error.to_string().contains("Timezone"));
}

#[test]
fn normalization_removes_inactive_imported_answers() {
    let encryption = BooleanQuestion::new(
        StepId::UseEncryption,
        "Encrypt?",
        crate::ui::nerd_font::NerdFont::Lock,
    )
    .relevant_when([StepId::PartitioningMethod], |context| {
        context
            .get_answer(&StepId::PartitioningMethod)
            .is_some_and(|method| !method.contains("Manual"))
    });
    let mut engine = WizardEngine::new(vec![
        question(StepId::PartitioningMethod, &[]),
        Box::new(encryption),
    ])
    .unwrap();
    engine.context.set_answer(
        StepId::PartitioningMethod,
        "Manual Partitioning".to_string(),
    );
    engine
        .context
        .set_answer(StepId::UseEncryption, "yes".to_string());

    engine.normalize_context();

    assert!(engine.context.get_answer(&StepId::UseEncryption).is_none());
}

#[test]
fn normalization_rejects_dependent_answers_without_provenance() {
    let mut engine = WizardEngine::new(vec![
        question(StepId::Disk, &[]),
        question(StepId::PartitioningMethod, &[StepId::Disk]),
    ])
    .unwrap();
    engine
        .context
        .set_answer(StepId::Disk, "/dev/sda".to_string());
    engine.context.set_answer(
        StepId::PartitioningMethod,
        "Automatic Partitioning".to_string(),
    );

    engine.normalize_context();

    assert_eq!(
        engine.context.get_answer(&StepId::Disk).map(String::as_str),
        Some("/dev/sda")
    );
    assert!(
        engine
            .context
            .get_answer(&StepId::PartitioningMethod)
            .is_none()
    );
}

#[test]
fn normalization_reasks_a_dependent_answer_when_provenance_is_stale() {
    let questions = vec![
        question(StepId::Disk, &[]),
        question(StepId::PartitioningMethod, &[StepId::Disk]),
    ];
    let graph = StepGraph::new(&questions).unwrap();
    let mut context = InstallContext::new();
    graph.record_answer(&mut context, StepId::Disk, "/dev/sda".into());
    graph.record_answer(
        &mut context,
        StepId::PartitioningMethod,
        "Automatic Partitioning".into(),
    );
    // Simulate an external edit which bypassed the graph but left the saved
    // provenance available for detecting the inconsistency.
    context.answers.insert(StepId::Disk, "/dev/sdb".to_string());
    let mut engine = WizardEngine::new(questions).unwrap().with_context(context);

    engine.normalize_context();

    assert!(
        engine
            .context
            .get_answer(&StepId::PartitioningMethod)
            .is_none()
    );
}

#[test]
fn dependency_provenance_survives_context_serialization() {
    let questions = vec![
        question(StepId::Disk, &[]),
        question(StepId::PartitioningMethod, &[StepId::Disk]),
    ];
    let graph = StepGraph::new(&questions).unwrap();
    let mut context = InstallContext::new();
    graph.record_answer(&mut context, StepId::Disk, "/dev/sda".into());
    graph.record_answer(
        &mut context,
        StepId::PartitioningMethod,
        "Automatic Partitioning".into(),
    );

    let serialized = context.to_toml().unwrap();
    let restored: InstallContext = toml::from_str(&serialized).unwrap();
    let mut engine = WizardEngine::new(questions).unwrap().with_context(restored);
    engine.normalize_context();

    assert_eq!(
        engine
            .context
            .get_answer(&StepId::PartitioningMethod)
            .map(String::as_str),
        Some("Automatic Partitioning")
    );
}

#[test]
fn changing_an_answer_invalidates_dependents_transitively() {
    let questions = vec![
        question(StepId::Disk, &[]),
        question(StepId::DualBootPartition, &[StepId::Disk]),
        question(StepId::DualBootSize, &[StepId::DualBootPartition]),
    ];
    let graph = StepGraph::new(&questions).unwrap();
    let mut context = InstallContext::new();

    graph.record_answer(&mut context, StepId::Disk, "/dev/nvme0n1".into());
    graph.record_answer(
        &mut context,
        StepId::DualBootPartition,
        "/dev/nvme0n1p3".into(),
    );
    graph.record_answer(&mut context, StepId::DualBootSize, "800".into());
    graph.record_answer(&mut context, StepId::Disk, "/dev/sda".into());

    assert!(context.get_answer(&StepId::DualBootPartition).is_none());
    assert!(context.get_answer(&StepId::DualBootSize).is_none());
    assert_eq!(
        context.get_answer(&StepId::Disk).map(String::as_str),
        Some("/dev/sda")
    );
}

#[test]
fn invalidation_crosses_an_unanswered_intermediate_question() {
    let questions = vec![
        question(StepId::Disk, &[]),
        question(StepId::DualBootPartition, &[StepId::Disk]),
        question(StepId::DualBootSize, &[StepId::DualBootPartition]),
    ];
    let graph = StepGraph::new(&questions).unwrap();
    let mut context = InstallContext::new();

    graph.record_answer(&mut context, StepId::Disk, "/dev/sda".into());
    graph.record_answer(&mut context, StepId::DualBootSize, "800".into());
    graph.record_answer(&mut context, StepId::Disk, "/dev/sdb".into());

    assert!(context.get_answer(&StepId::DualBootSize).is_none());
}

#[test]
fn changing_disk_invalidates_real_partitioning_method_question() {
    let questions: Vec<Box<dyn WizardStep>> = vec![
        question(StepId::Disk, &[]),
        Box::new(PartitioningMethodQuestion),
    ];
    let graph = StepGraph::new(&questions).unwrap();
    let mut context = InstallContext::new();

    graph.record_answer(&mut context, StepId::Disk, "/dev/sda".into());
    graph.record_answer(&mut context, StepId::PartitioningMethod, "Dual Boot".into());
    graph.record_answer(&mut context, StepId::Disk, "/dev/sdb".into());

    assert!(context.get_answer(&StepId::PartitioningMethod).is_none());
}

#[test]
fn disabling_encryption_removes_the_stored_password() {
    let questions: Vec<Box<dyn WizardStep>> = vec![
        question(StepId::UseEncryption, &[]),
        Box::new(EncryptionPasswordQuestion),
    ];
    let graph = StepGraph::new(&questions).unwrap();
    let mut context = InstallContext::new();

    graph.record_answer(&mut context, StepId::UseEncryption, "yes".into());
    graph.record_answer(&mut context, StepId::EncryptionPassword, "secret".into());
    graph.record_answer(&mut context, StepId::UseEncryption, "no".into());

    assert!(context.get_answer(&StepId::EncryptionPassword).is_none());
}

#[test]
fn graph_validation_rejects_duplicate_questions() {
    let questions = vec![question(StepId::Disk, &[]), question(StepId::Disk, &[])];
    assert!(StepGraph::new(&questions).is_err());
}

#[test]
fn graph_validation_rejects_misordered_questions() {
    let questions = vec![
        question(StepId::PartitioningMethod, &[StepId::Disk]),
        question(StepId::Disk, &[]),
    ];
    assert!(StepGraph::new(&questions).is_err());
}

#[test]
fn graph_validation_rejects_duplicate_dependencies() {
    let questions = vec![
        question(StepId::Disk, &[]),
        question(StepId::PartitioningMethod, &[StepId::Disk, StepId::Disk]),
    ];
    assert!(StepGraph::new(&questions).is_err());
}

#[test]
fn re_answering_with_the_same_value_keeps_dependents() {
    let questions = vec![
        question(StepId::Disk, &[]),
        question(StepId::DualBootPartition, &[StepId::Disk]),
    ];
    let graph = StepGraph::new(&questions).unwrap();
    let mut context = InstallContext::new();

    graph.record_answer(&mut context, StepId::Disk, "/dev/sda".into());
    graph.record_answer(&mut context, StepId::DualBootPartition, "/dev/sda2".into());
    graph.record_answer(&mut context, StepId::Disk, "/dev/sda".into());

    assert_eq!(
        context
            .get_answer(&StepId::DualBootPartition)
            .map(String::as_str),
        Some("/dev/sda2")
    );
}

#[test]
fn removing_an_answer_invalidates_dependents() {
    let questions = vec![
        question(StepId::Disk, &[]),
        question(StepId::DualBootPartition, &[StepId::Disk]),
    ];
    let graph = StepGraph::new(&questions).unwrap();
    let mut context = InstallContext::new();

    graph.record_answer(&mut context, StepId::Disk, "/dev/sda".into());
    graph.record_answer(&mut context, StepId::DualBootPartition, "/dev/sda2".into());
    graph.drop_step_state(&mut context, StepId::Disk);

    assert!(context.get_answer(&StepId::DualBootPartition).is_none());
}

#[test]
fn changing_a_dependency_invalidates_completed_action_steps() {
    let steps = vec![
        question(StepId::Disk, &[]),
        Box::new(CompletingStep {
            id: StepId::PrepareDisk,
            dependencies: vec![StepId::Disk],
        }) as Box<dyn WizardStep>,
    ];
    let graph = StepGraph::new(&steps).unwrap();
    let mut context = InstallContext::new();

    graph.record_answer(&mut context, StepId::Disk, "/dev/sda".into());
    graph.record_completion(&mut context, StepId::PrepareDisk);
    assert!(context.is_step_completed(StepId::PrepareDisk));

    graph.record_answer(&mut context, StepId::Disk, "/dev/vda".into());

    assert!(!context.is_step_completed(StepId::PrepareDisk));
}
