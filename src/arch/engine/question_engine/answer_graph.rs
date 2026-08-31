use std::collections::{HashMap, HashSet};

use anyhow::{Result, bail};
use sha2::{Digest, Sha256};

use super::super::{InstallContext, Question, QuestionId};

/// Validated dependency relationships between wizard questions.
///
/// Besides validating question order at construction time, this owns the
/// answer invalidation rules. Keeping mutations here makes it impossible for
/// the engine to update an answer without also invalidating derived answers.
pub(super) struct AnswerGraph {
    dependencies: HashMap<QuestionId, Vec<QuestionId>>,
    dependents: HashMap<QuestionId, Vec<QuestionId>>,
}

impl AnswerGraph {
    pub(super) fn new(questions: &[Box<dyn Question>]) -> Result<Self> {
        let mut positions = HashMap::new();
        for (index, question) in questions.iter().enumerate() {
            let id = question.id();
            if positions.insert(id, index).is_some() {
                bail!("question graph contains duplicate id {id:?}");
            }
        }

        let mut dependents: HashMap<QuestionId, Vec<QuestionId>> = HashMap::new();
        let mut dependencies_by_question = HashMap::new();
        for (index, question) in questions.iter().enumerate() {
            let mut dependencies = HashSet::new();
            for dependency in question.depends_on() {
                if !dependencies.insert(*dependency) {
                    bail!(
                        "question {:?} declares duplicate dependency {:?}",
                        question.id(),
                        dependency
                    );
                }
                if let Some(dependency_index) = positions.get(dependency)
                    && *dependency_index >= index
                {
                    bail!(
                        "question {:?} depends on {:?}, which must appear earlier",
                        question.id(),
                        dependency
                    );
                }
                dependents
                    .entry(*dependency)
                    .or_default()
                    .push(question.id());
            }
            dependencies_by_question.insert(question.id(), question.depends_on().to_vec());
        }

        Ok(Self {
            dependencies: dependencies_by_question,
            dependents,
        })
    }

    pub(super) fn record_answer(
        &self,
        context: &mut InstallContext,
        id: QuestionId,
        answer: String,
    ) {
        let changed = context.answers.get(&id) != Some(&answer);
        context.answers.insert(id, answer);
        self.record_dependency_fingerprint(context, id);
        if changed {
            self.invalidate_dependents(context, id);
        }
    }

    pub(super) fn drop_answer(&self, context: &mut InstallContext, id: QuestionId) {
        context.answer_dependency_fingerprints.remove(&id);
        if context.answers.remove(&id).is_some() {
            self.invalidate_dependents(context, id);
        }
    }

    /// Whether a stored answer was recorded against the dependency values
    /// currently in the context.
    pub(super) fn answer_is_current(&self, context: &InstallContext, id: QuestionId) -> bool {
        let Some(dependencies) = self.dependencies.get(&id) else {
            return false;
        };
        if dependencies.is_empty() {
            return true;
        }
        context
            .answer_dependency_fingerprints
            .get(&id)
            .is_none_or(|stored| stored == &dependency_fingerprint(context, dependencies))
    }

    fn record_dependency_fingerprint(&self, context: &mut InstallContext, id: QuestionId) {
        let Some(dependencies) = self.dependencies.get(&id) else {
            return;
        };
        if dependencies.is_empty() {
            context.answer_dependency_fingerprints.remove(&id);
        } else {
            context
                .answer_dependency_fingerprints
                .insert(id, dependency_fingerprint(context, dependencies));
        }
    }

    fn invalidate_dependents(&self, context: &mut InstallContext, changed: QuestionId) {
        let mut queue = vec![changed];
        let mut visited = HashSet::from([changed]);

        while let Some(current) = queue.pop() {
            let Some(dependents) = self.dependents.get(&current) else {
                continue;
            };
            for dependent in dependents {
                if visited.insert(*dependent) {
                    context.answers.remove(dependent);
                    context.answer_dependency_fingerprints.remove(dependent);
                    queue.push(*dependent);
                }
            }
        }
    }
}

fn dependency_fingerprint(context: &InstallContext, dependencies: &[QuestionId]) -> String {
    let mut hasher = Sha256::new();
    for dependency in dependencies {
        let id = format!("{dependency:?}");
        hasher.update((id.len() as u64).to_le_bytes());
        hasher.update(id.as_bytes());
        match context.answers.get(dependency) {
            Some(answer) => {
                hasher.update([1]);
                hasher.update((answer.len() as u64).to_le_bytes());
                hasher.update(answer.as_bytes());
            }
            None => hasher.update([0]),
        }
    }
    hex::encode(hasher.finalize())
}
