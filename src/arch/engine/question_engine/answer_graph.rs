use std::collections::{HashMap, HashSet};

use anyhow::{Result, bail};

use super::super::{InstallContext, Question, QuestionId};

/// Validated dependency relationships between wizard questions.
///
/// Besides validating question order at construction time, this owns the
/// answer invalidation rules. Keeping mutations here makes it impossible for
/// the engine to update an answer without also invalidating derived answers.
pub(super) struct AnswerGraph {
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
        }

        Ok(Self { dependents })
    }

    pub(super) fn record_answer(
        &self,
        context: &mut InstallContext,
        id: QuestionId,
        answer: String,
    ) {
        if context.answers.get(&id) == Some(&answer) {
            return;
        }
        context.answers.insert(id, answer);
        self.invalidate_dependents(context, id);
    }

    pub(super) fn drop_answer(&self, context: &mut InstallContext, id: QuestionId) {
        if context.answers.remove(&id).is_some() {
            self.invalidate_dependents(context, id);
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
                    queue.push(*dependent);
                }
            }
        }
    }
}
