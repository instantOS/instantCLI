use anyhow::Result;

use super::answer_graph::AnswerGraph;
use super::{FlowKind, QuestionEngine};
use crate::arch::engine::{
    AsyncDataProvider, DataKey, InstallContext, Question, QuestionId, QuestionResult,
};
use crate::arch::questions::{
    BooleanQuestion, EncryptionPasswordQuestion, PartitioningMethodQuestion,
};
use crate::menu_utils::MockQueue;

struct StubOptionalQuestion {
    id: QuestionId,
    default: Option<String>,
}

#[async_trait::async_trait]
impl Question for StubOptionalQuestion {
    fn id(&self) -> QuestionId {
        self.id
    }

    async fn ask(&self, _context: &InstallContext) -> Result<QuestionResult> {
        Ok(QuestionResult::Answer("answered".to_string()))
    }

    fn is_optional(&self) -> bool {
        true
    }

    fn get_default(&self, _context: &InstallContext) -> Option<String> {
        self.default.clone()
    }
}

struct StubDependentQuestion {
    id: QuestionId,
    dependencies: Vec<QuestionId>,
}

#[async_trait::async_trait]
impl Question for StubDependentQuestion {
    fn id(&self) -> QuestionId {
        self.id
    }

    async fn ask(&self, _context: &InstallContext) -> Result<QuestionResult> {
        Ok(QuestionResult::Answer("answered".to_string()))
    }

    fn depends_on(&self) -> &[QuestionId] {
        &self.dependencies
    }
}

fn question(id: QuestionId, dependencies: &[QuestionId]) -> Box<dyn Question> {
    Box::new(StubDependentQuestion {
        id,
        dependencies: dependencies.to_vec(),
    })
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
impl Question for ProviderBackedQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::Locale
    }

    fn required_data_keys(&self) -> Vec<String> {
        vec!["missing_provider_data".to_string()]
    }

    async fn ask(&self, _context: &InstallContext) -> Result<QuestionResult> {
        Ok(QuestionResult::Answer("answer".to_string()))
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
impl Question for ProviderSkippedQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::MirrorRegion
    }

    fn required_data_keys(&self) -> Vec<String> {
        vec![ProviderReadyKey::KEY.to_string()]
    }

    fn should_ask(&self, context: &InstallContext) -> bool {
        !context.get::<ProviderSkipKey>().unwrap_or(false)
    }

    async fn ask(&self, _context: &InstallContext) -> Result<QuestionResult> {
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
impl Question for IncompleteProviderQuestion {
    fn id(&self) -> QuestionId {
        QuestionId::Timezone
    }

    fn required_data_keys(&self) -> Vec<String> {
        vec![ProviderReadyKey::KEY.to_string()]
    }

    async fn ask(&self, _context: &InstallContext) -> Result<QuestionResult> {
        panic!("a question without its required data must not be asked")
    }

    fn data_providers(&self) -> Vec<Box<dyn AsyncDataProvider>> {
        vec![Box::new(IncompleteProvider)]
    }
}

#[test]
fn install_flow_applies_optional_defaults_without_asking() {
    let mut engine = QuestionEngine::for_flow(
        FlowKind::Install,
        vec![Box::new(StubOptionalQuestion {
            id: QuestionId::Autologin,
            default: Some("no".to_string()),
        })],
    )
    .unwrap();

    assert_eq!(engine.find_next_question_index(), None);
    assert_eq!(
        engine
            .context
            .get_answer(&QuestionId::Autologin)
            .map(String::as_str),
        Some("no")
    );
}

#[test]
fn setup_flow_asks_optional_questions_in_main_flow() {
    let mut engine = QuestionEngine::for_flow(
        FlowKind::Setup,
        vec![Box::new(StubOptionalQuestion {
            id: QuestionId::Autologin,
            default: Some("no".to_string()),
        })],
    )
    .unwrap();

    assert_eq!(engine.find_next_question_index(), Some(0));
    assert!(engine.context.get_answer(&QuestionId::Autologin).is_none());
}

#[tokio::test]
async fn provider_failures_return_an_error_instead_of_waiting_forever() {
    let mut engine = QuestionEngine::new(vec![Box::new(ProviderBackedQuestion)]).unwrap();
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
    let mut engine = QuestionEngine::new(vec![Box::new(ProviderSkippedQuestion)]).unwrap();
    let mut providers = engine.start_providers();

    assert_eq!(engine.find_next_question_index(), Some(0));
    assert!(matches!(
        engine.wait_until_ready(0, &mut providers).await.unwrap(),
        super::QuestionReadiness::Irrelevant
    ));
    assert_eq!(engine.find_next_question_index(), None);
}

#[tokio::test]
async fn settling_providers_removes_an_answer_that_becomes_irrelevant() {
    let mut engine = QuestionEngine::new(vec![Box::new(ProviderSkippedQuestion)]).unwrap();
    engine
        .context
        .set_answer(QuestionId::MirrorRegion, "Germany".to_string());
    engine.normalize_context();
    let mut providers = engine.start_providers();

    assert_eq!(engine.find_next_question_index(), None);
    providers.finish_all().await;
    engine.normalize_context();

    assert!(
        engine
            .context
            .get_answer(&QuestionId::MirrorRegion)
            .is_none()
    );
}

#[tokio::test]
async fn provider_completion_without_required_data_returns_an_error() {
    let mut engine = QuestionEngine::new(vec![Box::new(IncompleteProviderQuestion)]).unwrap();
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
        QuestionId::UseEncryption,
        "Encrypt?",
        crate::ui::nerd_font::NerdFont::Lock,
    )
    .relevant_when([QuestionId::PartitioningMethod], |context| {
        context
            .get_answer(&QuestionId::PartitioningMethod)
            .is_some_and(|method| !method.contains("Manual"))
    });
    let mut engine = QuestionEngine::new(vec![
        question(QuestionId::PartitioningMethod, &[]),
        Box::new(encryption),
    ])
    .unwrap();
    engine.context.set_answer(
        QuestionId::PartitioningMethod,
        "Manual Partitioning".to_string(),
    );
    engine
        .context
        .set_answer(QuestionId::UseEncryption, "yes".to_string());

    engine.normalize_context();

    assert!(
        engine
            .context
            .get_answer(&QuestionId::UseEncryption)
            .is_none()
    );
}

#[test]
fn normalization_accepts_explicit_dependent_answers_without_provenance() {
    let mut engine = QuestionEngine::new(vec![
        question(QuestionId::Disk, &[]),
        question(QuestionId::PartitioningMethod, &[QuestionId::Disk]),
    ])
    .unwrap();
    engine
        .context
        .set_answer(QuestionId::Disk, "/dev/sda".to_string());
    engine.context.set_answer(
        QuestionId::PartitioningMethod,
        "Automatic Partitioning".to_string(),
    );

    engine.normalize_context();

    assert_eq!(
        engine
            .context
            .get_answer(&QuestionId::Disk)
            .map(String::as_str),
        Some("/dev/sda")
    );
    assert_eq!(
        engine
            .context
            .get_answer(&QuestionId::PartitioningMethod)
            .map(String::as_str),
        Some("Automatic Partitioning")
    );
}

#[test]
fn normalization_reasks_a_dependent_answer_when_provenance_is_stale() {
    let questions = vec![
        question(QuestionId::Disk, &[]),
        question(QuestionId::PartitioningMethod, &[QuestionId::Disk]),
    ];
    let graph = AnswerGraph::new(&questions).unwrap();
    let mut context = InstallContext::new();
    graph.record_answer(&mut context, QuestionId::Disk, "/dev/sda".into());
    graph.record_answer(
        &mut context,
        QuestionId::PartitioningMethod,
        "Automatic Partitioning".into(),
    );
    // Simulate an external edit which bypassed the graph but left the saved
    // provenance available for detecting the inconsistency.
    context
        .answers
        .insert(QuestionId::Disk, "/dev/sdb".to_string());
    let mut engine = QuestionEngine::new(questions)
        .unwrap()
        .with_context(context);

    engine.normalize_context();

    assert!(
        engine
            .context
            .get_answer(&QuestionId::PartitioningMethod)
            .is_none()
    );
}

#[test]
fn dependency_provenance_survives_context_serialization() {
    let questions = vec![
        question(QuestionId::Disk, &[]),
        question(QuestionId::PartitioningMethod, &[QuestionId::Disk]),
    ];
    let graph = AnswerGraph::new(&questions).unwrap();
    let mut context = InstallContext::new();
    graph.record_answer(&mut context, QuestionId::Disk, "/dev/sda".into());
    graph.record_answer(
        &mut context,
        QuestionId::PartitioningMethod,
        "Automatic Partitioning".into(),
    );

    let serialized = context.to_toml().unwrap();
    let restored: InstallContext = toml::from_str(&serialized).unwrap();
    let mut engine = QuestionEngine::new(questions)
        .unwrap()
        .with_context(restored);
    engine.normalize_context();

    assert_eq!(
        engine
            .context
            .get_answer(&QuestionId::PartitioningMethod)
            .map(String::as_str),
        Some("Automatic Partitioning")
    );
}

#[test]
fn changing_an_answer_invalidates_dependents_transitively() {
    let questions = vec![
        question(QuestionId::Disk, &[]),
        question(QuestionId::DualBootPartition, &[QuestionId::Disk]),
        question(QuestionId::DualBootSize, &[QuestionId::DualBootPartition]),
    ];
    let graph = AnswerGraph::new(&questions).unwrap();
    let mut context = InstallContext::new();

    graph.record_answer(&mut context, QuestionId::Disk, "/dev/nvme0n1".into());
    graph.record_answer(
        &mut context,
        QuestionId::DualBootPartition,
        "/dev/nvme0n1p3".into(),
    );
    graph.record_answer(&mut context, QuestionId::DualBootSize, "800".into());
    graph.record_answer(&mut context, QuestionId::Disk, "/dev/sda".into());

    assert!(context.get_answer(&QuestionId::DualBootPartition).is_none());
    assert!(context.get_answer(&QuestionId::DualBootSize).is_none());
    assert_eq!(
        context.get_answer(&QuestionId::Disk).map(String::as_str),
        Some("/dev/sda")
    );
}

#[test]
fn invalidation_crosses_an_unanswered_intermediate_question() {
    let questions = vec![
        question(QuestionId::Disk, &[]),
        question(QuestionId::DualBootPartition, &[QuestionId::Disk]),
        question(QuestionId::DualBootSize, &[QuestionId::DualBootPartition]),
    ];
    let graph = AnswerGraph::new(&questions).unwrap();
    let mut context = InstallContext::new();

    graph.record_answer(&mut context, QuestionId::Disk, "/dev/sda".into());
    graph.record_answer(&mut context, QuestionId::DualBootSize, "800".into());
    graph.record_answer(&mut context, QuestionId::Disk, "/dev/sdb".into());

    assert!(context.get_answer(&QuestionId::DualBootSize).is_none());
}

#[test]
fn changing_disk_invalidates_real_partitioning_method_question() {
    let questions: Vec<Box<dyn Question>> = vec![
        question(QuestionId::Disk, &[]),
        Box::new(PartitioningMethodQuestion),
    ];
    let graph = AnswerGraph::new(&questions).unwrap();
    let mut context = InstallContext::new();

    graph.record_answer(&mut context, QuestionId::Disk, "/dev/sda".into());
    graph.record_answer(
        &mut context,
        QuestionId::PartitioningMethod,
        "Dual Boot".into(),
    );
    graph.record_answer(&mut context, QuestionId::Disk, "/dev/sdb".into());

    assert!(
        context
            .get_answer(&QuestionId::PartitioningMethod)
            .is_none()
    );
}

#[test]
fn disabling_encryption_removes_the_stored_password() {
    let questions: Vec<Box<dyn Question>> = vec![
        question(QuestionId::UseEncryption, &[]),
        Box::new(EncryptionPasswordQuestion),
    ];
    let graph = AnswerGraph::new(&questions).unwrap();
    let mut context = InstallContext::new();

    graph.record_answer(&mut context, QuestionId::UseEncryption, "yes".into());
    graph.record_answer(
        &mut context,
        QuestionId::EncryptionPassword,
        "secret".into(),
    );
    graph.record_answer(&mut context, QuestionId::UseEncryption, "no".into());

    assert!(
        context
            .get_answer(&QuestionId::EncryptionPassword)
            .is_none()
    );
}

#[test]
fn graph_validation_rejects_duplicate_questions() {
    let questions = vec![
        question(QuestionId::Disk, &[]),
        question(QuestionId::Disk, &[]),
    ];
    assert!(AnswerGraph::new(&questions).is_err());
}

#[test]
fn graph_validation_rejects_misordered_questions() {
    let questions = vec![
        question(QuestionId::PartitioningMethod, &[QuestionId::Disk]),
        question(QuestionId::Disk, &[]),
    ];
    assert!(AnswerGraph::new(&questions).is_err());
}

#[test]
fn graph_validation_rejects_duplicate_dependencies() {
    let questions = vec![
        question(QuestionId::Disk, &[]),
        question(
            QuestionId::PartitioningMethod,
            &[QuestionId::Disk, QuestionId::Disk],
        ),
    ];
    assert!(AnswerGraph::new(&questions).is_err());
}

#[test]
fn re_answering_with_the_same_value_keeps_dependents() {
    let questions = vec![
        question(QuestionId::Disk, &[]),
        question(QuestionId::DualBootPartition, &[QuestionId::Disk]),
    ];
    let graph = AnswerGraph::new(&questions).unwrap();
    let mut context = InstallContext::new();

    graph.record_answer(&mut context, QuestionId::Disk, "/dev/sda".into());
    graph.record_answer(
        &mut context,
        QuestionId::DualBootPartition,
        "/dev/sda2".into(),
    );
    graph.record_answer(&mut context, QuestionId::Disk, "/dev/sda".into());

    assert_eq!(
        context
            .get_answer(&QuestionId::DualBootPartition)
            .map(String::as_str),
        Some("/dev/sda2")
    );
}

#[test]
fn removing_an_answer_invalidates_dependents() {
    let questions = vec![
        question(QuestionId::Disk, &[]),
        question(QuestionId::DualBootPartition, &[QuestionId::Disk]),
    ];
    let graph = AnswerGraph::new(&questions).unwrap();
    let mut context = InstallContext::new();

    graph.record_answer(&mut context, QuestionId::Disk, "/dev/sda".into());
    graph.record_answer(
        &mut context,
        QuestionId::DualBootPartition,
        "/dev/sda2".into(),
    );
    graph.drop_answer(&mut context, QuestionId::Disk);

    assert!(context.get_answer(&QuestionId::DualBootPartition).is_none());
}
