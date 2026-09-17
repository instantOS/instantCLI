use anyhow::{Result, bail};

use crate::restic::ResticWrapper;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RepositoryIntent {
    Create,
    Connect,
}

/// Keep storage probing separate from intent: a failed probe must never become
/// permission to initialize a repository.
trait RepositoryAccess {
    fn exists(&self) -> Result<bool>;
    fn create(&self) -> Result<()>;
    fn verify(&self) -> Result<()>;
}

impl RepositoryAccess for ResticWrapper {
    fn exists(&self) -> Result<bool> {
        Ok(self.repository_exists()?)
    }

    fn create(&self) -> Result<()> {
        Ok(self.init_repository()?)
    }

    fn verify(&self) -> Result<()> {
        self.list_snapshots_filtered(None)?;
        Ok(())
    }
}

pub(super) fn prepare_repository(
    repository: &str,
    password: &str,
    intent: RepositoryIntent,
) -> Result<()> {
    let restic = ResticWrapper::new(repository.to_string(), password.to_string())?;
    prepare(&restic, intent)
}

fn prepare(repository: &impl RepositoryAccess, intent: RepositoryIntent) -> Result<()> {
    match (intent, repository.exists()?) {
        (RepositoryIntent::Create, true) => {
            bail!(
                "Backups already exist at this location. Choose 'Connect existing backups' instead, or choose a different folder. Nothing was overwritten."
            )
        }
        (RepositoryIntent::Connect, false) => {
            bail!(
                "No backup repository was found at this location. Check the remote and folder, or choose 'Create new backups'."
            )
        }
        (RepositoryIntent::Create, false) => repository.create(),
        (RepositoryIntent::Connect, true) => repository.verify(),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    struct FakeRepository {
        exists: Result<bool>,
        creates: Cell<usize>,
        verifies: Cell<usize>,
    }

    impl FakeRepository {
        fn new(exists: Result<bool>) -> Self {
            Self {
                exists,
                creates: Cell::new(0),
                verifies: Cell::new(0),
            }
        }
    }

    impl RepositoryAccess for FakeRepository {
        fn exists(&self) -> Result<bool> {
            self.exists
                .as_ref()
                .copied()
                .map_err(|e| anyhow::anyhow!("{e}"))
        }
        fn create(&self) -> Result<()> {
            self.creates.set(self.creates.get() + 1);
            Ok(())
        }
        fn verify(&self) -> Result<()> {
            self.verifies.set(self.verifies.get() + 1);
            Ok(())
        }
    }

    #[test]
    fn probe_failure_never_creates_repository() {
        for intent in [RepositoryIntent::Create, RepositoryIntent::Connect] {
            let repo = FakeRepository::new(Err(anyhow::anyhow!("Invalid password")));
            assert!(prepare(&repo, intent).is_err());
            assert_eq!(repo.creates.get(), 0);
        }
    }

    #[test]
    fn creation_requires_missing_repository() {
        let repo = FakeRepository::new(Ok(false));
        prepare(&repo, RepositoryIntent::Create).unwrap();
        assert_eq!(repo.creates.get(), 1);
        let repo = FakeRepository::new(Ok(true));
        assert!(prepare(&repo, RepositoryIntent::Create).is_err());
        assert_eq!(repo.creates.get(), 0);
    }

    #[test]
    fn connecting_never_initializes() {
        let repo = FakeRepository::new(Ok(false));
        assert!(prepare(&repo, RepositoryIntent::Connect).is_err());
        assert_eq!(repo.creates.get(), 0);
        let repo = FakeRepository::new(Ok(true));
        prepare(&repo, RepositoryIntent::Connect).unwrap();
        assert_eq!(repo.verifies.get(), 1);
        assert_eq!(repo.creates.get(), 0);
    }
}
