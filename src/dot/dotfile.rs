use super::db::Database;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

pub struct Dotfile {
    pub repo_path: PathBuf,
    pub target_path: PathBuf,
    pub hash: Option<String>,
    pub target_hash: Option<String>,
}

impl Dotfile {
    pub fn is_outdated(&self) -> bool {
        if !self.target_path.exists() {
            return true;
        }

        let source_metadata = fs::metadata(&self.repo_path).ok();
        let target_metadata = fs::metadata(&self.target_path).ok();

        if let (Some(source_meta), Some(target_meta)) = (source_metadata, target_metadata) {
            if let (Ok(source_time), Ok(target_time)) =
                (source_meta.modified(), target_meta.modified())
            {
                return source_time > target_time;
            }
        }

        false
    }

    pub fn is_modified(&self, db: &Database) -> bool {
        if !self.target_path.exists() {
            return false;
        }

        if let Some(target_hash) = self.get_target_hash(db) {
            if let Ok(valid_hashes) = db.get_valid_hashes(&self.target_path) {
                return !valid_hashes.contains(&target_hash);
            }
        }

        true
    }

    pub fn get_target_hash(&self, db: &Database) -> Option<String> {
        if !self.target_path.exists() {
            return None;
        }
        let hash = Self::get_hash(&self.target_path).unwrap();
        //TODO: do not add hash if the current hash is one already in the DB
        //TODO: check if this behavior is already present
        db.add_hash(&hash, &self.target_path, false).ok();
        Some(hash)
    }

    pub fn get_source_hash(&self, db: &Database) -> Option<String> {
        let hash = Self::get_hash(&self.repo_path).unwrap();
        db.add_hash(&hash, &self.target_path, true).ok();
        Some(hash)
    }

    fn get_hash(path: &Path) -> Option<String> {
        if let Ok(content) = fs::read(path) {
            let mut hasher = Sha256::new();
            hasher.update(content);
            let result = hasher.finalize();
            return Some(format!("{:x}", result));
        }
        None
    }

    pub fn apply(&self, db: &Database) -> Result<(), std::io::Error> {
        if self.is_modified(db) {
            return Ok(());
        }

        if !self.is_outdated() {
            return Ok(());
        }

        if let Some(parent) = self.target_path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::copy(&self.repo_path, &self.target_path)?;

        self.get_source_hash(db);

        Ok(())
    }

    pub fn fetch(&self, db: &Database) -> Result<(), std::io::Error> {
        if self.is_modified(db) {
            fs::copy(&self.target_path, &self.repo_path)?;
            self.get_target_hash(db);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Dotfile;
    use crate::dot::db::Database;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_apply_and_fetch() {
        let dir = tempdir().unwrap();
        let repo_path = dir.path().join("repo");
        let target_path = dir.path().join("target");
        fs::create_dir_all(&repo_path).unwrap();
        fs::write(repo_path.join("test.txt"), "test").unwrap();

        let db = Database::new().unwrap();
        let dotfile = Dotfile {
            repo_path: repo_path.join("test.txt"),
            target_path: target_path.join("test.txt"),
            hash: None,
            target_hash: None,
        };

        dotfile.apply(&db).unwrap();
        assert!(target_path.join("test.txt").exists());

        fs::write(target_path.join("test.txt"), "modified").unwrap();
        dotfile.fetch(&db).unwrap();
        assert_eq!(
            fs::read_to_string(repo_path.join("test.txt")).unwrap(),
            "modified"
        );
    }
}
