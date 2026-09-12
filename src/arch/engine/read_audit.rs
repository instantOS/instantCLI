//! Dev-time audit for undeclared step-state reads.
//!
//! The [`WizardStep`] contract requires every step answer that `should_ask`,
//! `ask_policy`, `preselect_answer`, or `validate` reads to be declared in
//! [`WizardStep::depends_on`] — nothing else enforces that, and undeclared
//! reads silently break dependency invalidation. The engine therefore wraps
//! those synchronous hooks: while one runs, the answer accessors on
//! [`InstallContext`] record which [`StepId`]s they touched, and any read
//! outside the declared set is reported when debug mode is on.
//!
//! `run` is not audited: it is async and interactive.

use std::cell::RefCell;
use std::collections::BTreeSet;

use super::WizardStep;
use super::types::StepId;

thread_local! {
    static AUDIT_ACTIVE: RefCell<bool> = const { RefCell::new(false) };
    static READS: RefCell<BTreeSet<StepId>> = const { RefCell::new(BTreeSet::new()) };
}

/// Runs a synchronous step hook under the read audit and reports any reads of
/// answers the step did not declare in `depends_on` when debug mode is on.
pub(crate) fn hook<T>(step: &dyn WizardStep, phase: &str, read: impl FnOnce() -> T) -> T {
    let (result, undeclared) = audited(step, read);
    if !undeclared.is_empty() && crate::ui::is_debug_enabled() {
        eprintln!(
            "wizard audit: {:?} {phase}() reads {undeclared:?} without declaring them in depends_on()",
            step.id()
        );
    }
    result
}

/// Runs a synchronous step hook under the read audit and returns the answer
/// reads the step did not declare in `depends_on`.
pub(crate) fn audited<T>(step: &dyn WizardStep, read: impl FnOnce() -> T) -> (T, Vec<StepId>) {
    AUDIT_ACTIVE.with(|active| *active.borrow_mut() = true);
    READS.with(|reads| reads.borrow_mut().clear());
    let result = read();
    AUDIT_ACTIVE.with(|active| *active.borrow_mut() = false);
    (result, take_undeclared(step))
}

/// Records an answer read made while a hook audit is active. Called from the
/// answer accessors on [`InstallContext`].
pub(crate) fn record_read(id: StepId) {
    AUDIT_ACTIVE.with(|active| {
        if *active.borrow() {
            READS.with(|reads| reads.borrow_mut().insert(id));
        }
    });
}

fn take_undeclared(step: &dyn WizardStep) -> Vec<StepId> {
    let reads = READS.with(|reads| std::mem::take(&mut *reads.borrow_mut()));
    let declared = step.depends_on();
    reads
        .iter()
        .filter(|read| !declared.contains(read))
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arch::engine::{InstallContext, StepOutcome};

    struct ReadsTwoAnswers;

    #[async_trait::async_trait]
    impl WizardStep for ReadsTwoAnswers {
        fn id(&self) -> StepId {
            StepId::Locale
        }

        fn depends_on(&self) -> &[StepId] {
            &[StepId::Keymap]
        }

        async fn run(&self, _context: &InstallContext) -> anyhow::Result<StepOutcome> {
            unimplemented!("hooks are audited, never run")
        }
    }

    #[test]
    fn reads_outside_declared_dependencies_are_flagged() {
        let step = ReadsTwoAnswers;
        let returned = hook(&step, "preselect_answer", || {
            record_read(StepId::Keymap);
            record_read(StepId::Timezone);
            7
        });
        assert_eq!(returned, 7);

        AUDIT_ACTIVE.with(|active| *active.borrow_mut() = true);
        record_read(StepId::Keymap);
        record_read(StepId::Timezone);
        AUDIT_ACTIVE.with(|active| *active.borrow_mut() = false);
        assert_eq!(take_undeclared(&step), vec![StepId::Timezone]);
    }

    #[test]
    fn reads_outside_an_audit_are_not_recorded() {
        record_read(StepId::Timezone);
        let step = ReadsTwoAnswers;
        hook(&step, "should_ask", || {
            record_read(StepId::Keymap);
        });
        // The stray read was ignored; the audited one was cleared by the hook.
        AUDIT_ACTIVE.with(|active| *active.borrow_mut() = true);
        assert!(READS.with(|reads| reads.borrow().is_empty()));
        AUDIT_ACTIVE.with(|active| *active.borrow_mut() = false);
    }
}
