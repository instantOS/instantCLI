use super::db::{Database, DotFileType};
use super::encryption;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Whether a dotfile's source is stored plain on disk or as an age-encrypted blob.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    /// Source file is byte-identical to the desired target content.
    Plain,
    /// Source file ends in `.age` and must be decrypted before applying.
    Age,
}

impl SourceKind {
    /// Infer the source kind from a source path's extension.
    pub fn from_source_path(p: &Path) -> Self {
        if encryption::is_encrypted_source(p) {
            SourceKind::Age
        } else {
            SourceKind::Plain
        }
    }
}

// Simple in-memory cache for file hashes.
// This is distinct from the persistent Database. It is used to avoid re-computing
// SHA256 hashes for the same file path multiple times within a single process run.
// It does not persist across runs.
static HASH_CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

const HASH_CACHE_SIZE: usize = 1000; // Limit cache size to prevent memory bloat

fn get_hash_cache() -> &'static Mutex<HashMap<String, String>> {
    HASH_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Remove a path from the in-memory hash cache.
/// Should be called whenever a file is modified by the program.
fn invalidate_cache(path: &Path) {
    let path_str = path.to_string_lossy().to_string();
    if let Ok(mut cache) = get_hash_cache().lock() {
        cache.remove(&path_str);
    }
}

#[derive(Clone)]
pub struct Dotfile {
    pub source_path: PathBuf,
    pub target_path: PathBuf,
    pub is_root: bool,
    pub kind: SourceKind,
}

impl Dotfile {
    /// Construct a `Dotfile`, inferring `kind` from the source path's extension.
    /// Caller is responsible for having already stripped any `.age` suffix from
    /// `target_path` when applicable.
    pub fn new(source_path: PathBuf, target_path: PathBuf, is_root: bool) -> Self {
        let kind = SourceKind::from_source_path(&source_path);
        Self {
            source_path,
            target_path,
            is_root,
            kind,
        }
    }

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
        // This means the file matches a known source version and can be restored
        if db.source_hash_exists_anywhere(&target_hash)? {
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

        // Check if cached hash is newer than file modification time.
        // We add a buffer to account for filesystem timestamp granularity: some filesystems
        // (especially in CI containers) have coarse timestamp resolution (1-2 seconds).
        // Without this buffer, a file modified immediately after hashing might have a
        // modification time <= the recorded hash time, causing stale hash reuse.
        const TIMESTAMP_BUFFER: chrono::TimeDelta = chrono::TimeDelta::seconds(2);

        let file_metadata = fs::metadata(path)?;
        let file_modified = file_metadata.modified()?;
        let file_time = chrono::DateTime::<chrono::Utc>::from(file_modified);

        // Fast path (works for both plain and age sources): if the most recent
        // hash recorded against this path is newer than the file's mtime, the
        // stored hash — which for age sources is the *plaintext* hash — is
        // still valid.
        if let Ok(Some(newest_hash)) = db.get_newest_hash(path)
            && newest_hash.created >= file_time + TIMESTAMP_BUFFER
        {
            return Ok(newest_hash.hash);
        }

        // Slow path: file changed (or first time we've seen it).
        if is_source && self.kind == SourceKind::Age && path == self.source_path {
            return self.compute_and_store_age_source_hash(db);
        }

        // Plain path: hash the file's actual bytes and store.
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

    /// Resolve the plaintext sha256 of an age-encrypted source file.
    ///
    /// Strategy:
    /// 1. Hash the ciphertext (cheap, buffered).
    /// 2. Consult the `encrypted_sources` table — if we've decrypted this
    ///    exact ciphertext before, reuse the cached plaintext hash and skip
    ///    decryption entirely (this is the common case for `status` / `diff`
    ///    after `git pull` of unrelated files).
    /// 3. On miss: load identities, decrypt to memory, hash the plaintext,
    ///    persist the `cipher_hash → plain_hash` mapping.
    ///
    /// In every case the resulting plaintext hash is also written into
    /// `file_hashes` under the source path so subsequent calls hit the mtime
    /// fast path in [`Self::get_file_hash`].
    fn compute_and_store_age_source_hash(&self, db: &Database) -> Result<String, anyhow::Error> {
        let cipher_hash = Self::compute_hash(&self.source_path)?;

        let plain_hash = if let Some(cached) = db.get_plain_hash_for_cipher(&cipher_hash)? {
            cached
        } else {
            let identities = encryption::load_identities()?;
            let plaintext = encryption::decrypt_file_to_bytes(&self.source_path, &identities)?;
            let h = Self::hash_bytes(&plaintext);
            db.record_encrypted_source(&cipher_hash, &h)?;
            h
        };

        // Persist the plaintext hash against the source path so the mtime
        // fast path picks it up on subsequent calls.
        db.add_hash(&plain_hash, &self.source_path, DotFileType::SourceFile)?;
        Ok(plain_hash)
    }

    /// Sha256-hex of an in-memory byte slice. Used for plaintext of
    /// decrypted age sources (never goes through the on-disk hash cache).
    fn hash_bytes(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        hex::encode(hasher.finalize())
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

        let hash = hex::encode(hasher.finalize());

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

        match self.kind {
            SourceKind::Plain => {
                fs::copy(&self.source_path, &self.target_path)?;
                invalidate_cache(&self.target_path);

                // After applying, record the target hash with source_file=false since we just copied from source
                let source_hash = self.get_file_hash(&self.source_path, true, db)?;
                db.add_hash(&source_hash, &self.target_path, DotFileType::TargetFile)?;
            }
            SourceKind::Age => {
                let identities = encryption::load_identities()?;
                let plaintext = encryption::decrypt_file_to_bytes(&self.source_path, &identities)?;
                fs::write(&self.target_path, &plaintext)?;
                invalidate_cache(&self.target_path);

                // Record the plaintext hash on both sides. We just produced
                // the plaintext, so hash the in-memory buffer directly rather
                // than re-reading the target from disk (and also seed the
                // cipher → plain mapping if it was missing).
                let plain_hash = Self::hash_bytes(&plaintext);
                let cipher_hash = Self::compute_hash(&self.source_path)?;
                db.record_encrypted_source(&cipher_hash, &plain_hash)?;
                db.add_hash(&plain_hash, &self.source_path, DotFileType::SourceFile)?;
                db.add_hash(&plain_hash, &self.target_path, DotFileType::TargetFile)?;
            }
        }

        Ok(())
    }

    pub fn fetch(&self, db: &Database) -> Result<(), anyhow::Error> {
        if !self.target_path.exists() {
            return Ok(());
        }

        // v1: fetching back into an encrypted source is not yet implemented.
        // Doing so requires recipient configuration in instantdots.toml and
        // careful re-encryption to avoid spurious git diffs from new nonces;
        // see plans/encryption.md.
        if self.kind == SourceKind::Age {
            return Err(anyhow::anyhow!(
                "fetch is not yet supported for age-encrypted dotfiles ({}): re-encrypt manually with `age` for now",
                self.source_path.display()
            ));
        }

        let target_hash = self.get_file_hash(&self.target_path, false, db)?;

        let should_copy = if self.source_path.exists() {
            let source_hash = self.get_file_hash(&self.source_path, true, db)?;
            target_hash != source_hash
        } else {
            true
        };

        if should_copy {
            fs::copy(&self.target_path, &self.source_path)?;
            invalidate_cache(&self.source_path);
            // Update the source file's hash in the database after copying
            let _ = self.get_file_hash(&self.source_path, true, db);
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

        match self.kind {
            SourceKind::Plain => {
                // Force copy source -> target, overwriting any modifications
                fs::copy(&self.source_path, &self.target_path)?;
                invalidate_cache(&self.target_path);

                // After reset, record the target hash with source_file=false since we just copied from source
                let source_hash = self.get_file_hash(&self.source_path, true, db)?;
                db.add_hash(&source_hash, &self.target_path, DotFileType::TargetFile)?;
            }
            SourceKind::Age => {
                let identities = encryption::load_identities()?;
                let plaintext = encryption::decrypt_file_to_bytes(&self.source_path, &identities)?;
                fs::write(&self.target_path, &plaintext)?;
                invalidate_cache(&self.target_path);

                let plain_hash = Self::hash_bytes(&plaintext);
                let cipher_hash = Self::compute_hash(&self.source_path)?;
                db.record_encrypted_source(&cipher_hash, &plain_hash)?;
                db.add_hash(&plain_hash, &self.source_path, DotFileType::SourceFile)?;
                db.add_hash(&plain_hash, &self.target_path, DotFileType::TargetFile)?;
            }
        }

        Ok(())
    }

    /// Create the source file in the repository by copying from the target (home) file,
    /// and register its hash in the database as an unmodified source.
    pub fn create_source_from_target(&self, db: &Database) -> Result<(), anyhow::Error> {
        // v1: creating an encrypted source from a plaintext target is not
        // implemented. That belongs in `add --encrypt`, which requires
        // recipient configuration. See plans/encryption.md.
        if self.kind == SourceKind::Age {
            return Err(anyhow::anyhow!(
                "creating an encrypted source ({}) from a target is not yet supported; \
                 encrypt manually with `age` and place the resulting file in the repo",
                self.source_path.display()
            ));
        }

        // Ensure parent directories exist
        if let Some(parent) = self.source_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Copy target -> source
        fs::copy(&self.target_path, &self.source_path)?;
        invalidate_cache(&self.source_path);

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
    use serial_test::serial;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    #[serial]
    fn test_apply_and_fetch() {
        let dir = tempdir().unwrap();
        let repo_path = dir.path().join("repo");
        let target_path = dir.path().join("target");
        fs::create_dir_all(&repo_path).unwrap();
        fs::write(repo_path.join("test.txt"), "test").unwrap();

        let db_path = dir.path().join("test.db");
        let db = Database::new(db_path).unwrap();
        let dotfile = Dotfile::new(
            repo_path.join("test.txt"),
            target_path.join("test.txt"),
            false,
        );

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

    #[test]
    #[serial]
    fn test_apply_age_encrypted_source() {
        use std::io::Write as _;

        let dir = tempdir().unwrap();
        let repo_path = dir.path().join("repo");
        let target_dir = dir.path().join("target");
        fs::create_dir_all(&repo_path).unwrap();
        fs::create_dir_all(&target_dir).unwrap();

        // Generate a throwaway age identity, write it to a temp file, and
        // point AGE_IDENTITY at it for the duration of this test.
        let identity = age::x25519::Identity::generate();
        let recipient = identity.to_public();
        let identity_file = dir.path().join("identity.txt");
        fs::write(&identity_file, identity.to_string().expose_secret())
            .expect("write identity file");

        // Encrypt some plaintext into the source path.
        let plaintext = b"super secret token = abc123\n";
        let encryptor =
            age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
                .expect("build encryptor");
        let source_path = repo_path.join("secrets.txt.age");
        let cipher_file = fs::File::create(&source_path).unwrap();
        let mut writer = encryptor.wrap_output(cipher_file).unwrap();
        writer.write_all(plaintext).unwrap();
        writer.finish().unwrap();

        // Run apply with AGE_IDENTITY pointing at our identity file.
        let prev = std::env::var_os("AGE_IDENTITY");
        // SAFETY: tests are serialised via #[serial].
        unsafe {
            std::env::set_var("AGE_IDENTITY", &identity_file);
        }

        let db = Database::new(dir.path().join("test.db")).unwrap();
        let target_path = target_dir.join("secrets.txt");
        let dotfile = Dotfile::new(source_path.clone(), target_path.clone(), false);
        assert_eq!(dotfile.kind, super::SourceKind::Age);

        dotfile.apply(&db).expect("apply encrypted dotfile");

        // Restore env before asserting so a panic still cleans up.
        unsafe {
            match prev {
                Some(v) => std::env::set_var("AGE_IDENTITY", v),
                None => std::env::remove_var("AGE_IDENTITY"),
            }
        }

        assert!(target_path.exists(), "target should exist after apply");
        assert_eq!(
            fs::read(&target_path).unwrap(),
            plaintext,
            "target should contain the decrypted plaintext"
        );

        // A second apply with the same (unchanged) source should be a no-op
        // and must not require redoing the decryption — verify by deleting
        // the identity file and confirming apply still succeeds.
        fs::remove_file(&identity_file).unwrap();
        dotfile
            .apply(&db)
            .expect("second apply must reuse cached plaintext hash and not decrypt again");
    }

    #[test]
    #[serial]
    fn test_fetch_updates_hash_cache() {
        let dir = tempdir().unwrap();
        let repo_path = dir.path().join("repo");
        let target_path = dir.path().join("target");
        fs::create_dir_all(&repo_path).unwrap();
        fs::create_dir_all(&target_path).unwrap();

        fs::write(repo_path.join("test.txt"), "initial").unwrap();
        fs::write(target_path.join("test.txt"), "modified").unwrap();

        let db_path = dir.path().join("test.db");
        let db = Database::new(db_path).unwrap();
        let dotfile = Dotfile::new(
            repo_path.join("test.txt"),
            target_path.join("test.txt"),
            false,
        );

        // 1. Compute hash of source (populates cache with "initial")
        let initial_hash = Dotfile::compute_hash(&dotfile.source_path).unwrap();

        // 2. Fetch (updates source file to "modified")
        dotfile.fetch(&db).unwrap();

        // 3. Compute hash of source again (should return hash of "modified")
        let new_hash = Dotfile::compute_hash(&dotfile.source_path).unwrap();

        assert_ne!(initial_hash, new_hash, "Hash should change after fetch");
    }
}
