use std::collections::{HashMap, HashSet};

use anyhow::{Result, bail};
use sha2::{Digest, Sha256};

use super::super::{InstallContext, StepId, WizardStep};

/// Validated dependency relationships between wizard steps.
///
/// Besides validating step order at construction time, this owns answer and
/// completion invalidation. Keeping mutations here makes it impossible for
/// the engine to update step state without invalidating derived state.
pub(super) struct StepGraph {
    dependencies: HashMap<StepId, Vec<StepId>>,
    dependents: HashMap<StepId, Vec<StepId>>,
}

impl StepGraph {
    pub(super) fn new(steps: &[Box<dyn WizardStep>]) -> Result<Self> {
        let mut positions = HashMap::new();
        for (index, step) in steps.iter().enumerate() {
            let id = step.id();
            if positions.insert(id, index).is_some() {
                bail!("step graph contains duplicate id {id:?}");
            }
        }

        let mut dependents: HashMap<StepId, Vec<StepId>> = HashMap::new();
        let mut dependencies_by_step = HashMap::new();
        for (index, step) in steps.iter().enumerate() {
            let mut dependencies = HashSet::new();
            for dependency in step.depends_on() {
                if !dependencies.insert(*dependency) {
                    bail!(
                        "step {:?} declares duplicate dependency {:?}",
                        step.id(),
                        dependency
                    );
                }
                if let Some(dependency_index) = positions.get(dependency)
                    && *dependency_index >= index
                {
                    bail!(
                        "step {:?} depends on {:?}, which must appear earlier",
                        step.id(),
                        dependency
                    );
                }
                dependents.entry(*dependency).or_default().push(step.id());
            }
            dependencies_by_step.insert(step.id(), step.depends_on().to_vec());
        }

        Ok(Self {
            dependencies: dependencies_by_step,
            dependents,
        })
    }

    pub(super) fn record_answer(&self, context: &mut InstallContext, id: StepId, answer: String) {
        let changed =
            context.answers.get(&id) != Some(&answer) || context.completed_steps.contains(&id);
        context.completed_steps.remove(&id);
        context.answers.insert(id, answer);
        self.record_dependency_fingerprint(context, id);
        if changed {
            self.invalidate_dependents(context, id);
        }
    }

    pub(super) fn record_completion(&self, context: &mut InstallContext, id: StepId) {
        let changed = context.answers.remove(&id).is_some() || context.completed_steps.insert(id);
        self.record_dependency_fingerprint(context, id);
        if changed {
            self.invalidate_dependents(context, id);
        }
    }

    pub(super) fn drop_step_state(&self, context: &mut InstallContext, id: StepId) {
        context.step_dependency_fingerprints.remove(&id);
        let changed = context.answers.remove(&id).is_some() || context.completed_steps.remove(&id);
        if changed {
            self.invalidate_dependents(context, id);
        }
    }

    /// Whether a stored answer was recorded against the dependency values
    /// currently in the context.
    pub(super) fn step_state_is_current(&self, context: &InstallContext, id: StepId) -> bool {
        let Some(dependencies) = self.dependencies.get(&id) else {
            return false;
        };
        if dependencies.is_empty() {
            return true;
        }
        context
            .step_dependency_fingerprints
            .get(&id)
            .is_some_and(|stored| stored == &dependency_fingerprint(context, dependencies))
    }

    fn record_dependency_fingerprint(&self, context: &mut InstallContext, id: StepId) {
        let Some(dependencies) = self.dependencies.get(&id) else {
            return;
        };
        if dependencies.is_empty() {
            context.step_dependency_fingerprints.remove(&id);
        } else {
            context
                .step_dependency_fingerprints
                .insert(id, dependency_fingerprint(context, dependencies));
        }
    }

    fn invalidate_dependents(&self, context: &mut InstallContext, changed: StepId) {
        let mut queue = vec![changed];
        let mut visited = HashSet::from([changed]);

        while let Some(current) = queue.pop() {
            let Some(dependents) = self.dependents.get(&current) else {
                continue;
            };
            for dependent in dependents {
                if visited.insert(*dependent) {
                    context.answers.remove(dependent);
                    context.completed_steps.remove(dependent);
                    context.step_dependency_fingerprints.remove(dependent);
                    queue.push(*dependent);
                }
            }
        }
    }
}

fn dependency_fingerprint(context: &InstallContext, dependencies: &[StepId]) -> String {
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
            None if context.completed_steps.contains(dependency) => hasher.update([2]),
            None => hasher.update([0]),
        }
    }
    hex::encode(hasher.finalize())
}
