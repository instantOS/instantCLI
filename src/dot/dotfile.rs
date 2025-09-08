use super::db::Database;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

pub struct Dotfile {
    pub source_path: PathBuf,
    pub target_path: PathBuf,
    pub hash: Option<String>,
    pub target_hash: Option<String>,
}

impl Dotfile {
    pub fn is_outdated(&self) -> bool {
        if !self.target_path.exists() {
            return true;
        }

        let source_metadata = fs::metadata(&self.source_path).ok();
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

        if let Ok(target_hash) = self.get_target_hash(db) {
            if let Ok(unmodified_hashes) = db.get_unmodified_hashes(&self.target_path) {
                return !unmodified_hashes.iter().any(|h| h.hash == target_hash);
            }
        }

        true
    }

    pub fn get_target_hash(&self, db: &Database) -> Result<String, anyhow::Error> {
        if !self.target_path.exists() {
            return Err(anyhow::anyhow!(
                "Target file does not exist: {}",
                self.target_path.display()
            ));
        }

        // Check if there's a hash in the database newer than the file's modification time
        let file_metadata = fs::metadata(&self.target_path)?;
        let file_modified = file_metadata.modified()?;

        if let Ok(Some(newest_hash)) = db.get_newest_hash(&self.target_path) {
            // Compare the database timestamp with file modification time
            let file_time = chrono::DateTime::<chrono::Utc>::from(file_modified);
            if newest_hash.created >= file_time {
                // Database has a hash newer than or equal to file modification time,
                // so we can return the newest unmodified hash for this file
                if newest_hash.unmodified {
                    return Ok(newest_hash.hash);
                }
            }
        }

        // No newer hash found, compute the hash
        let hash = Self::get_hash(&self.target_path)?;
        // Only add hash if it doesn't already exist in the database
        if !db.hash_exists(&hash, &self.target_path)? {
            db.add_hash(&hash, &self.target_path, false)?;
        }
        Ok(hash)
    }

    pub fn get_source_hash(&self, db: &Database) -> Result<String, anyhow::Error> {
        // Check if there's a hash in the database newer than the file's modification time
        let file_metadata = fs::metadata(&self.source_path)?;
        let file_modified = file_metadata.modified()?;

        if let Ok(Some(newest_hash)) = db.get_newest_hash(&self.target_path) {
            // Compare the database timestamp with file modification time
            let file_time = chrono::DateTime::<chrono::Utc>::from(file_modified);
            if newest_hash.created >= file_time {
                // Database has a hash newer than or equal to file modification time,
                // so we can return the newest unmodified hash for this file
                if newest_hash.unmodified {
                    return Ok(newest_hash.hash);
                }
            }
        }

        // No newer hash found, compute the hash
        let hash = Self::get_hash(&self.source_path)?;
        // Only add hash if it doesn't already exist in the database
        if !db.hash_exists(&hash, &self.target_path)? {
            db.add_hash(&hash, &self.target_path, true)?;
        }
        Ok(hash)
    }

    fn get_hash(path: &Path) -> Result<String, anyhow::Error> {
        let content = fs::read(path)?;
        let mut hasher = Sha256::new();
        hasher.update(content);
        let result = hasher.finalize();
        Ok(format!("{:x}", result))
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

        fs::copy(&self.source_path, &self.target_path)?;

        let _ = self.get_source_hash(db);

        Ok(())
    }

    pub fn fetch(&self, db: &Database) -> Result<(), std::io::Error> {
        if self.is_modified(db) {
            fs::copy(&self.target_path, &self.source_path)?;
            let _ = self.get_target_hash(db);
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
            source_path: repo_path.join("test.txt"),
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
