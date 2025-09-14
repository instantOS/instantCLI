use super::db::{Database, DotFileType};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

// Simple in-memory cache for file hashes
static HASH_CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

const HASH_CACHE_SIZE: usize = 1000; // Limit cache size to prevent memory bloat

fn get_hash_cache() -> &'static Mutex<HashMap<String, String>> {
    HASH_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub struct Dotfile {
    pub source_path: PathBuf,
    pub target_path: PathBuf,
}

impl Dotfile {
    pub fn is_outdated(&self, db: &Database) -> bool {
        if !self.target_path.exists() {
            return true;
        }

        // First, check if both files have the same hash in the database
        if let (Ok(source_hash), Ok(target_hash)) = (
            self.get_file_hash(&self.source_path, true, db),
            self.get_file_hash(&self.target_path, false, db),
        ) && source_hash == target_hash
        {
            // Files have the same content, not outdated
            return false;
        }

        // Fall back to modification time comparison
        let source_metadata = fs::metadata(&self.source_path).ok();
        let target_metadata = fs::metadata(&self.target_path).ok();

        if let (Some(source_meta), Some(target_meta)) = (source_metadata, target_metadata)
            && let (Ok(source_time), Ok(target_time)) =
                (source_meta.modified(), target_meta.modified())
        {
            return source_time > target_time;
        }

        false
    }

    /// Determines if the target file can be safely overwritten
    ///
    /// Returns true if the target file can be safely overwritten:
    /// - File doesn't exist (can be created safely)
    /// - File was created by instantCLI and hasn't been modified by user
    /// - File matches the current source content
    ///
    /// Returns false if the target file has been modified by the user and should not be overwritten
    pub fn is_target_unmodified(&self, db: &Database) -> Result<bool, anyhow::Error> {
        // Non-existent files can always be safely overwritten
        if !self.target_path.exists() {
            return Ok(true);
        }

        // Step 1: Get target hash
        let target_hash = self.get_file_hash(&self.target_path, false, db)?;

        // Step 2: Check if target hash matches any source hash in DB
        // This means the file was created by instantCLI and hasn't been modified
        if db.source_hash_exists(&target_hash, &self.target_path)? {
            return Ok(true);
        }

        // Step 3: Check if target matches current source content
        let source_hash = self.get_file_hash(&self.source_path, true, db)?;
        Ok(target_hash == source_hash)
    }

    pub fn get_file_hash(
        &self,
        path: &Path,
        is_source: bool,
        db: &Database,
    ) -> Result<String, anyhow::Error> {
        if !path.exists() {
            return Err(anyhow::anyhow!("File does not exist: {}", path.display()));
        }

        // Check if cached hash is newer than file modification time
        let file_metadata = fs::metadata(path)?;
        let file_modified = file_metadata.modified()?;
        let file_time = chrono::DateTime::<chrono::Utc>::from(file_modified);

        if let Ok(Some(newest_hash)) = db.get_newest_hash(path)
            && newest_hash.created >= file_time
        {
            return Ok(newest_hash.hash);
        }

        // Compute and store new hash
        let hash = Self::compute_hash(path)?;
        db.add_hash(
            &hash,
            path,
            if is_source {
                DotFileType::SourceFile
            } else {
                DotFileType::TargetFile
            },
        )?;
        Ok(hash)
    }

    pub fn compute_hash(path: &Path) -> Result<String, anyhow::Error> {
        // Check cache first
        let path_str = path.to_string_lossy().to_string();
        {
            let cache = get_hash_cache().lock().unwrap();
            if let Some(cached_hash) = cache.get(&path_str) {
                return Ok(cached_hash.clone());
            }
        }

        // Compute hash with buffered reading for large files
        let file = fs::File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0; 8192]; // 8KB buffer
        let mut file = std::io::BufReader::new(file);

        loop {
            let bytes_read = std::io::Read::read(&mut file, &mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }

        let result = hasher.finalize();
        let hash = format!("{result:x}");

        // Cache the result
        {
            let mut cache = get_hash_cache().lock().unwrap();
            if cache.len() >= HASH_CACHE_SIZE {
                // Simple eviction: clear half the cache
                let keys: Vec<String> = cache.keys().cloned().collect();
                for key in keys.iter().take(HASH_CACHE_SIZE / 2) {
                    cache.remove(key);
                }
            }
            cache.insert(path_str, hash.clone());
        }

        Ok(hash)
    }

    pub fn apply(&self, db: &Database) -> Result<(), anyhow::Error> {
        if !self.is_target_unmodified(db)? {
            // Skip modified files, as they could contain user modifications
            // This project is a dotfile manager which can be run in the background, and should not
            // override files touched by the user or other programs. If an unmodified hash exists,
            // this means the file was created by this program and has not not been touched by
            // anything else, so it can be overridden without concern
            return Ok(());
        }

        if !self.is_outdated(db) {
            let _ = self.get_file_hash(&self.source_path, true, db);
            return Ok(());
        }

        if !self.target_path.exists()
            && let Some(parent) = self.target_path.parent()
        {
            fs::create_dir_all(parent)?;
        }

        fs::copy(&self.source_path, &self.target_path)?;

        // After applying, record the target hash with source_file=false since we just copied from source
        let source_hash = self.get_file_hash(&self.source_path, true, db)?;
        db.add_hash(&source_hash, &self.target_path, DotFileType::TargetFile)?;

        Ok(())
    }

    pub fn fetch(&self, db: &Database) -> Result<(), anyhow::Error> {
        if !self.is_target_unmodified(db)? {
            fs::copy(&self.target_path, &self.source_path)?;
            let _ = self.get_file_hash(&self.target_path, false, db);
        }
        Ok(())
    }

    /// Reset the target file by forcefully copying from the source file,
    /// regardless of whether the target is currently modified.
    /// This updates the database to mark the target as unmodified.
    pub fn reset(&self, db: &Database) -> Result<(), anyhow::Error> {
        // Ensure parent directories exist
        if let Some(parent) = self.target_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Force copy source -> target, overwriting any modifications
        fs::copy(&self.source_path, &self.target_path)?;

        // After reset, record the target hash with source_file=false since we just copied from source
        let source_hash = self.get_file_hash(&self.source_path, true, db)?;
        db.add_hash(&source_hash, &self.target_path, DotFileType::TargetFile)?;

        Ok(())
    }

    /// Create the source file in the repository by copying from the target (home) file,
    /// and register its hash in the database as an unmodified source.
    pub fn create_source_from_target(&self, db: &Database) -> Result<(), anyhow::Error> {
        // Ensure parent directories exist
        if let Some(parent) = self.source_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Copy target -> source
        fs::copy(&self.target_path, &self.source_path)?;

        // Compute the hash of the copied content
        let hash = Self::compute_hash(&self.source_path)?;

        // Register the hash with correct source_file flags
        // This ensures that both files are considered in sync
        db.add_hash(&hash, &self.source_path, DotFileType::SourceFile)?; // source_file=true for source
        db.add_hash(&hash, &self.target_path, DotFileType::TargetFile)?; // source_file=false for target

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Dotfile;
    use crate::dot::db::{Database, DotFileType};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_apply_and_fetch() {
        let dir = tempdir().unwrap();
        let repo_path = dir.path().join("repo");
        let target_path = dir.path().join("target");
        fs::create_dir_all(&repo_path).unwrap();
        fs::write(repo_path.join("test.txt"), "test").unwrap();

        let db_path = dir.path().join("test.db");
        let db = Database::new(db_path).unwrap();
        let dotfile = Dotfile {
            source_path: repo_path.join("test.txt"),
            target_path: target_path.join("test.txt"),
        };

        // Register the source file hash first
        let source_hash = Dotfile::compute_hash(&dotfile.source_path).unwrap();
        db.add_hash(&source_hash, &dotfile.source_path, DotFileType::SourceFile)
            .unwrap();

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
