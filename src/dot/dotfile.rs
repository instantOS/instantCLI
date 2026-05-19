use super::config::DotfileConfig;
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

    pub fn set_source_path(&mut self, source_path: PathBuf) {
        self.kind = SourceKind::from_source_path(&source_path);
        self.source_path = source_path;
    }

    pub fn is_outdated(&self, db: &Database) -> Result<bool, anyhow::Error> {
        if !self.target_path.exists() {
            return Ok(true);
        }

        let source_hash = self.get_file_hash(&self.source_path, true, db)?;
        let target_hash = self.get_file_hash(&self.target_path, false, db)?;
        if source_hash == target_hash {
            // Files have the same content, not outdated
            return Ok(false);
        }

        // Fall back to modification time comparison
        let source_metadata = fs::metadata(&self.source_path).ok();
        let target_metadata = fs::metadata(&self.target_path).ok();

        if let (Some(source_meta), Some(target_meta)) = (source_metadata, target_metadata)
            && let (Ok(source_time), Ok(target_time)) =
                (source_meta.modified(), target_meta.modified())
        {
            return Ok(source_time > target_time);
        }

        Ok(false)
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
    pub(crate) fn hash_bytes(bytes: &[u8]) -> String {
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

    /// Decrypt the age source, write plaintext to the target, and record all
    /// hashes in the database. Shared by `apply` and `reset`.
    fn decrypt_source_to_target(&self, db: &Database) -> Result<(), anyhow::Error> {
        let identities = encryption::load_identities()?;
        let plaintext = encryption::decrypt_file_to_bytes(&self.source_path, &identities)?;
        crate::dot::utils::persist_file_safely(
            &self.target_path,
            &plaintext,
            "decrypted target file",
        )?;
        invalidate_cache(&self.target_path);

        let plain_hash = Self::hash_bytes(&plaintext);
        let cipher_hash = Self::compute_hash(&self.source_path)?;
        db.register_encrypted_hashes(
            &cipher_hash,
            &plain_hash,
            &self.source_path,
            &self.target_path,
        )?;
        Ok(())
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

        if !self.is_outdated(db)? {
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
                self.decrypt_source_to_target(db)?;
            }
        }

        Ok(())
    }

    pub fn fetch(&self, db: &Database, config: &DotfileConfig) -> Result<(), anyhow::Error> {
        use anyhow::Context as _;

        if !self.target_path.exists() {
            return Ok(());
        }

        match self.kind {
            SourceKind::Plain => {
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
            }
            SourceKind::Age => {
                let repo_name = crate::dot::git::get_repo_name_for_dotfile(self, config);
                let dotfile_repo =
                    crate::dot::dotfilerepo::DotfileRepo::new(config, repo_name.to_string())?;

                let recipients = crate::dot::encryption::parse_recipients(
                    &dotfile_repo.meta.encryption_recipients,
                )
                .context("loading repository public keys for re-encryption")?;

                let target_hash = self.get_file_hash(&self.target_path, false, db)?;
                let plain_hash = self.get_file_hash(&self.source_path, true, db)?;

                if target_hash != plain_hash {
                    let plaintext = fs::read(&self.target_path)?;
                    let ciphertext = encryption::encrypt_bytes_to_armored(&plaintext, &recipients)?;
                    crate::dot::utils::persist_file_safely(
                        &self.source_path,
                        &ciphertext,
                        "encrypted source file",
                    )?;
                    invalidate_cache(&self.source_path);

                    // Record the new plain hash and cipher hash in the database
                    let new_plain_hash = Self::hash_bytes(&plaintext);
                    let new_cipher_hash = Self::compute_hash(&self.source_path)?;
                    db.register_encrypted_hashes(
                        &new_cipher_hash,
                        &new_plain_hash,
                        &self.source_path,
                        &self.target_path,
                    )?;
                }
            }
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
                self.decrypt_source_to_target(db)?;
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

    /// Create an encrypted source file in the repository by encrypting the target (home) file,
    /// and register its hashes in the database.
    pub fn create_encrypted_source_from_target(
        &self,
        db: &Database,
        recipients: &[Box<dyn age::Recipient>],
    ) -> Result<(), anyhow::Error> {
        use anyhow::Context;

        if self.kind != SourceKind::Age {
            return Err(anyhow::anyhow!(
                "source path {} is not an encrypted (.age) source",
                self.source_path.display()
            ));
        }

        // Ensure parent directories exist
        if let Some(parent) = self.source_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Read plaintext from target
        let plaintext = fs::read(&self.target_path).with_context(|| {
            format!(
                "reading plaintext from target file {}",
                self.target_path.display()
            )
        })?;
        let plain_hash = Self::hash_bytes(&plaintext);

        // Encrypt to armored ciphertext
        let ciphertext = encryption::encrypt_bytes_to_armored(&plaintext, recipients)
            .context("encrypting plaintext for source file")?;

        // Write ciphertext safely/atomically using consolidated helper
        crate::dot::utils::persist_file_safely(&self.source_path, &ciphertext, "encrypted source")?;

        // Register hashes in SQLite database
        let cipher_hash = Self::compute_hash(&self.source_path)?;
        db.register_encrypted_hashes(
            &cipher_hash,
            &plain_hash,
            &self.source_path,
            &self.target_path,
        )?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Dotfile;
    use crate::dot::config::DotfileConfig;
    use crate::dot::db::{Database, DotFileType};
    use crate::dot::encryption;
    use age::secrecy::ExposeSecret;
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
        dotfile.fetch(&db, &DotfileConfig::default()).unwrap();
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
        dotfile.fetch(&db, &DotfileConfig::default()).unwrap();

        // 3. Compute hash of source again (should return hash of "modified")
        let new_hash = Dotfile::compute_hash(&dotfile.source_path).unwrap();

        assert_ne!(initial_hash, new_hash, "Hash should change after fetch");
    }

    #[test]
    #[serial]
    fn test_fetch_age_encrypted_source() {
        use crate::dot::config::{DotfileConfig, Repo};
        use crate::dot::encryption;
        use crate::dot::types::RepoMetaData;
        use std::io::Write as _;

        let dir = tempdir().unwrap();
        let config_path = dir.path().join("dots.toml");
        let repos_dir = dir.path().join("repos");
        let target_dir = dir.path().join("target");
        let repo_path = repos_dir.join("my-repo");
        let dots_dir = repo_path.join("dots");

        fs::create_dir_all(&dots_dir).unwrap();
        fs::create_dir_all(&target_dir).unwrap();

        // 1. Generate a throwaway age identity and key
        let identity = age::x25519::Identity::generate();
        let recipient = identity.to_public();
        let identity_file = dir.path().join("identity.txt");
        fs::write(&identity_file, identity.to_string().expose_secret())
            .expect("write identity file");

        // 2. Set up age recipients string
        let recipient_str = recipient.to_string();

        // 3. Write instantdots.toml inside the repository
        let metadata = RepoMetaData {
            name: "my-repo".to_string(),
            description: Some("My personal secrets".to_string()),
            dots_dirs: vec!["dots".to_string()],
            encryption_recipients: vec![recipient_str.clone()],
            ..Default::default()
        };
        let meta_toml = toml::to_string(&metadata).unwrap();
        fs::write(repo_path.join("instantdots.toml"), meta_toml).unwrap();

        // 4. Create the global dots.toml config
        let repo_config = Repo {
            url: "https://example.com/dots.git".to_string(),
            name: "my-repo".to_string(),
            branch: None,
            enabled: true,
            read_only: false,
            active_subdirectories: Some(vec!["dots".to_string()]),
            metadata: None,
        };
        let config = DotfileConfig {
            repos: vec![repo_config],
            repos_dir: crate::common::tilde_path::TildePath::new(repos_dir.clone()),
            database_dir: crate::common::tilde_path::TildePath::new(dir.path().join("test.db")),
            clone_depth: 1,
            hash_cleanup_days: 30,
            ignored_paths: vec![],
            units: vec![],
            encryption_keys: vec![],
        };
        // Save the config to disk so that DotfileConfig::load(None) reads it
        let config_toml = toml::to_string(&config).unwrap();
        fs::write(&config_path, config_toml).unwrap();

        // 5. Encrypt initial plaintext "initial secret" into repository source
        let initial_plaintext = b"initial secret";
        let encryptor =
            age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
                .expect("build encryptor");
        let source_path = dots_dir.join("secrets.txt.age");
        let cipher_file = fs::File::create(&source_path).unwrap();
        let mut writer = encryptor.wrap_output(cipher_file).unwrap();
        writer.write_all(initial_plaintext).unwrap();
        writer.finish().unwrap();

        // 6. Set up the local target file with modified content "modified secret"
        let target_path = target_dir.join("secrets.txt");
        let modified_plaintext = b"modified secret";
        fs::write(&target_path, modified_plaintext).unwrap();

        // 7. Point AGE_IDENTITY at our identity file for decryption during status checks
        let prev = std::env::var_os("AGE_IDENTITY");
        let prev_config = std::env::var_os("XDG_CONFIG_HOME");
        // SAFETY: serialized via #[serial]
        unsafe {
            std::env::set_var("AGE_IDENTITY", &identity_file);
            // Point XDG_CONFIG_HOME to our temp dir so load(None) finds dots.toml
            let config_home = dir.path().join(".config");
            fs::create_dir_all(config_home.join("instant")).unwrap();
            fs::copy(&config_path, config_home.join("instant").join("dots.toml")).unwrap();
            std::env::set_var("XDG_CONFIG_HOME", &config_home);
        }

        let db = Database::new(dir.path().join("test.db")).unwrap();
        let dotfile = Dotfile::new(source_path.clone(), target_path.clone(), false);
        assert_eq!(dotfile.kind, super::SourceKind::Age);

        // Pre-register initial plain/cipher hashes so target is detected as modified from source
        let initial_cipher_hash = Dotfile::compute_hash(&source_path).unwrap();
        let initial_plain_hash = Dotfile::hash_bytes(initial_plaintext);
        db.record_encrypted_source(&initial_cipher_hash, &initial_plain_hash)
            .unwrap();
        db.add_hash(&initial_plain_hash, &source_path, DotFileType::SourceFile)
            .unwrap();
        db.add_hash(&initial_plain_hash, &target_path, DotFileType::TargetFile)
            .unwrap();

        // Run fetch (sync target -> source)
        dotfile
            .fetch(&db, &config)
            .expect("fetch encrypted dotfile");

        // Verify the source ciphertext has changed and can be decrypted to the modified plaintext
        let new_cipher_hash = Dotfile::compute_hash(&source_path).unwrap();
        assert_ne!(
            initial_cipher_hash, new_cipher_hash,
            "Ciphertext hash must have updated"
        );

        // Decrypt the newly generated source file to verify its contents
        let identities = encryption::load_identities().unwrap();
        let decrypted_bytes = encryption::decrypt_file_to_bytes(&source_path, &identities).unwrap();
        assert_eq!(
            decrypted_bytes, modified_plaintext,
            "Decrypted bytes must match modified plaintext"
        );

        // Restore env
        unsafe {
            match prev {
                Some(v) => std::env::set_var("AGE_IDENTITY", v),
                None => std::env::remove_var("AGE_IDENTITY"),
            }
            match prev_config {
                Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
                None => std::env::remove_var("XDG_CONFIG_HOME"),
            }
        }
    }

    #[test]
    #[serial]
    fn test_create_encrypted_source_from_target() {
        let dir = tempdir().unwrap();
        let target_path = dir.path().join("secrets.txt");
        let source_path = dir.path().join("secrets.txt.age");
        fs::write(&target_path, "super_secret_plaintext").unwrap();

        let db_path = dir.path().join("test.db");
        let db = Database::new(db_path).unwrap();

        // Generate recipient and identity
        let identity = age::x25519::Identity::generate();
        let recipient = identity.to_public();

        let dotfile = Dotfile::new(source_path.clone(), target_path.clone(), false);
        assert_eq!(dotfile.kind, super::SourceKind::Age);

        // Perform encryption copy to source repo path
        dotfile
            .create_encrypted_source_from_target(&db, &[Box::new(recipient)])
            .unwrap();

        // Verify source exists
        assert!(source_path.exists());

        // Verify plaintext doesn't leak into source directory directly
        let source_content = fs::read_to_string(&source_path).unwrap();
        assert!(!source_content.contains("super_secret_plaintext"));

        // Verify we can decrypt the source file back to original plaintext
        let decrypted =
            encryption::decrypt_file_to_bytes(&source_path, &[Box::new(identity)]).unwrap();
        assert_eq!(decrypted, b"super_secret_plaintext");

        // Verify DB records
        let plain_hash = Dotfile::compute_hash(&target_path).unwrap();
        let cipher_hash = Dotfile::compute_hash(&source_path).unwrap();

        let recorded_plain = db.get_plain_hash_for_cipher(&cipher_hash).unwrap();
        assert_eq!(recorded_plain, Some(plain_hash));
    }
}
