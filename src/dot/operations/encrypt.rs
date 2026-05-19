use crate::dot::config::DotfileConfig;
use crate::dot::db::{Database, DotFileType};
use crate::dot::dotfile::{Dotfile, SourceKind};
use crate::dot::dotfilerepo::DotfileRepo;
use crate::dot::utils::resolve_dotfile_path;
use crate::ui::prelude::*;
use anyhow::{Context, Result, anyhow};
use colored::Colorize;
use std::fs;

pub fn encrypt_dotfile(
    config: &DotfileConfig,
    db: &Database,
    path: &str,
    repo: Option<&str>,
    subdir: Option<&str>,
    dry_run: bool,
    include_root: bool,
    debug: bool,
) -> Result<()> {
    let target_path = resolve_dotfile_path(path, include_root, false)?;
    let dotfile = crate::dot::utils::resolve_dotfile_to_source(
        config,
        db,
        &target_path,
        repo,
        subdir,
        include_root,
    )?;

    // Issue #1 (TOCTOU recovery): detect "both plaintext and ciphertext on
    // disk" BEFORE the SourceKind check below. This can happen after a
    // previous encrypt run wrote the .age file but crashed before unlinking
    // the plaintext. Depending on which file the resolver picked, kind may
    // be Plain or Age — we need to identify the matching plain/cipher pair
    // and attempt DB-confirmed recovery either way.
    let (plain_candidate, cipher_candidate): (std::path::PathBuf, std::path::PathBuf) =
        match dotfile.kind {
            SourceKind::Plain => {
                let cipher = crate::dot::encryption::append_age_suffix(&dotfile.source_path);
                (dotfile.source_path.clone(), cipher)
            }
            SourceKind::Age => {
                let plain = crate::dot::encryption::strip_age_suffix(&dotfile.source_path)
                    .ok_or_else(|| {
                        anyhow!(
                            "encrypted source path does not end in '.age': {}",
                            dotfile.source_path.display()
                        )
                    })?;
                (plain, dotfile.source_path.clone())
            }
        };

    if plain_candidate.exists() && cipher_candidate.exists() {
        match try_recover_encrypt_leftover(db, &plain_candidate, &cipher_candidate)? {
            EncryptRecovery::Recovered => {
                emit(
                    Level::Info,
                    "dot.encrypt.recovered_leftover",
                    &format!(
                        "{} Recovered from interrupted previous encrypt: {} already matches {}",
                        char::from(NerdFont::Info),
                        cipher_candidate.display().to_string().cyan(),
                        plain_candidate.display().to_string().cyan(),
                    ),
                    Some(serde_json::json!({
                        "encrypted_source": cipher_candidate.display().to_string(),
                        "removed_plaintext": plain_candidate.display().to_string(),
                    })),
                );
                return Ok(());
            }
            EncryptRecovery::Diverged => {
                anyhow::bail!(
                    "encrypted source already exists and does NOT match the current plaintext: {}\n\
This usually means a previous encrypt was interrupted, or the file was edited \
out-of-band. To recover, either:\n  \
- remove {} (drops new plaintext changes), or\n  \
- remove {} (drops previous ciphertext) and re-run.",
                    cipher_candidate.display(),
                    plain_candidate.display(),
                    cipher_candidate.display(),
                );
            }
            EncryptRecovery::Unknown => {
                anyhow::bail!(
                    "encrypted source already exists: {}\n\
The previous encrypt may have been interrupted. If you are certain the \
plaintext at {} is identical to the contents of the ciphertext, delete the \
plaintext file and re-run; otherwise delete the ciphertext file and re-run.",
                    cipher_candidate.display(),
                    plain_candidate.display(),
                );
            }
        }
    }

    if dotfile.kind == SourceKind::Age {
        anyhow::bail!(
            "{} is already backed by an encrypted source: {}",
            display_target(&dotfile),
            dotfile.source_path.display()
        );
    }

    if !dotfile.source_path.exists() {
        anyhow::bail!(
            "source file does not exist for {}: {}",
            display_target(&dotfile),
            dotfile.source_path.display()
        );
    }

    let repo_name = crate::dot::git::get_repo_name_for_dotfile(&dotfile, config);
    let repo_config = config
        .repos
        .iter()
        .find(|repo| repo.name == repo_name.as_str())
        .ok_or_else(|| anyhow!("repository '{}' not found in config", repo_name))?;
    if repo_config.read_only {
        anyhow::bail!("repository '{}' is read-only", repo_name);
    }

    let dotfile_repo = DotfileRepo::new(config, repo_name.to_string())?;
    let recipients = crate::dot::encryption::parse_recipients(
        &dotfile_repo.meta.encryption_recipients,
    )
    .with_context(|| {
        format!(
            "repository '{}' has no usable encryption_recipients configured in instantdots.toml.\n\
                 Please authorize decryption keys first using 'ins dot keys authorize'.",
            repo_name
        )
    })?;

    let encrypted_source_path = crate::dot::encryption::append_age_suffix(&dotfile.source_path);
    // Dual-existence is handled above as crash recovery; if we get here and
    // the ciphertext exists, the resolver picked the plaintext alone and
    // the prior recovery branch already bailed. Be defensive in case the
    // shape ever changes (e.g. test seeds the dir after resolve).
    if encrypted_source_path.exists() {
        anyhow::bail!(
            "encrypted source already exists: {}",
            encrypted_source_path.display()
        );
    }

    // If target exists, verify it doesn't have uncommitted modifications that would be lost
    if dotfile.target_path.exists() {
        let is_unmodified = dotfile
            .is_target_unmodified(db)
            .context("verifying target modification state")?;
        if !is_unmodified {
            anyhow::bail!(
                "Target file {} has local modifications. Fetch changes with `ins dot add` or reset before encrypting.",
                dotfile.target_path.display()
            );
        }
    }

    // Always encrypt the repository source file as the source of truth
    let plaintext = fs::read(&dotfile.source_path).with_context(|| {
        format!(
            "reading plaintext from source file {}",
            dotfile.source_path.display()
        )
    })?;
    let plain_hash = Dotfile::hash_bytes(&plaintext);

    if dry_run {
        emit(
            Level::Info,
            "dot.encrypt.dry_run",
            &format!(
                "{} Would encrypt {} -> {}",
                char::from(NerdFont::Info),
                dotfile.source_path.display().to_string().cyan(),
                encrypted_source_path.display().to_string().cyan()
            ),
            Some(serde_json::json!({
                "target": display_target(&dotfile),
                "source": dotfile.source_path.display().to_string(),
                "encrypted_source": encrypted_source_path.display().to_string(),
                "repo": repo_name.as_str(),
                "dry_run": true
            })),
        );
        print_history_warning();
        return Ok(());
    }

    let ciphertext = crate::dot::encryption::encrypt_bytes_to_armored(&plaintext, &recipients)
        .context("encrypting dotfile plaintext")?;
    crate::dot::utils::persist_file_safely(
        &encrypted_source_path,
        &ciphertext,
        "encrypted source",
    )?;
    fs::remove_file(&dotfile.source_path).with_context(|| {
        format!(
            "removing plaintext source {}",
            dotfile.source_path.display()
        )
    })?;

    let cipher_hash = Dotfile::compute_hash(&encrypted_source_path)?;
    db.record_encrypted_source(&cipher_hash, &plain_hash)?;
    db.add_hash(&plain_hash, &encrypted_source_path, DotFileType::SourceFile)?;
    if dotfile.target_path.exists() {
        db.add_hash(&plain_hash, &dotfile.target_path, DotFileType::TargetFile)?;
    }

    let repo_path = dotfile_repo.local_path(config)?;
    crate::dot::git::repo_ops::git_add(&repo_path, &dotfile.source_path, debug)
        .with_context(|| format!("staging deletion of {}", dotfile.source_path.display()))?;
    crate::dot::git::repo_ops::git_add(&repo_path, &encrypted_source_path, debug)
        .with_context(|| format!("staging {}", encrypted_source_path.display()))?;

    emit(
        Level::Success,
        "dot.encrypt.complete",
        &format!(
            "{} Encrypted {}",
            char::from(NerdFont::Check),
            display_target(&dotfile).green()
        ),
        Some(serde_json::json!({
            "target": display_target(&dotfile),
            "source_removed": dotfile.source_path.display().to_string(),
            "encrypted_source": encrypted_source_path.display().to_string(),
            "repo": repo_name.as_str()
        })),
    );
    print_history_warning();

    Ok(())
}

/// Outcome of attempting to recover from a leftover encrypted file produced
/// by a previously interrupted encrypt run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EncryptRecovery {
    /// The on-disk ciphertext is known (via the DB) to encrypt the exact
    /// plaintext currently on disk. Plaintext was removed and the database
    /// was re-synced; the caller can return success.
    Recovered,
    /// The on-disk ciphertext encrypts a *different* plaintext than what is
    /// currently on disk. The caller must bail with a divergence error.
    Diverged,
    /// We have no DB record for the on-disk ciphertext, so we can't safely
    /// decide what to do without identities. The caller must bail with
    /// recovery instructions.
    Unknown,
}

fn try_recover_encrypt_leftover(
    db: &Database,
    plain_path: &std::path::Path,
    cipher_path: &std::path::Path,
) -> Result<EncryptRecovery> {
    // The DB stores cipher_hash -> plain_hash for every previously encrypted
    // file. If both files are on disk and the DB has a record for the
    // current cipher_hash, we can decide divergence without identities.
    let cipher_hash = Dotfile::compute_hash(cipher_path).with_context(|| {
        format!(
            "hashing leftover encrypted source {}",
            cipher_path.display()
        )
    })?;
    let recorded_plain = db.get_plain_hash_for_cipher(&cipher_hash)?;
    let Some(recorded_plain) = recorded_plain else {
        return Ok(EncryptRecovery::Unknown);
    };

    let plaintext = fs::read(plain_path)
        .with_context(|| format!("reading plaintext source {}", plain_path.display()))?;
    let current_plain = Dotfile::hash_bytes(&plaintext);

    if current_plain != recorded_plain {
        return Ok(EncryptRecovery::Diverged);
    }

    // Match: the leftover ciphertext encrypts exactly the current
    // plaintext. Finish the previously interrupted run by removing the
    // plaintext and re-syncing the source-side bookkeeping.
    fs::remove_file(plain_path).with_context(|| {
        format!(
            "removing leftover plaintext source {} during recovery",
            plain_path.display()
        )
    })?;
    db.add_hash(&current_plain, cipher_path, DotFileType::SourceFile)?;
    Ok(EncryptRecovery::Recovered)
}

fn display_target(dotfile: &Dotfile) -> String {
    crate::dot::display_path(&dotfile.target_path, dotfile.is_root)
}

fn print_history_warning() {
    println!(
        "\n{}",
        "┌────────────────────────────────────────────────────────┐"
            .yellow()
            .bold()
    );
    println!(
        "{}",
        "│ SECURITY WARNING:                                      │"
            .yellow()
            .bold()
    );
    println!(
        "{}",
        "│                                                        │"
            .yellow()
            .bold()
    );
    println!(
        "{}",
        "│ Encrypting this file only protects future commits!     │"
            .yellow()
            .bold()
    );
    println!(
        "{}",
        "│ Plaintext secrets are STILL PRESENT in your git        │"
            .yellow()
            .bold()
    );
    println!(
        "{}",
        "│ repository history and can be exposed if pushed.       │"
            .yellow()
            .bold()
    );
    println!(
        "{}",
        "│                                                        │"
            .yellow()
            .bold()
    );
    println!(
        "{}",
        "│ To completely remove plaintext from history, you must  │"
            .yellow()
            .bold()
    );
    println!(
        "{}",
        "│ purge it using a tool like git-filter-repo or BFG.     │"
            .yellow()
            .bold()
    );
    println!(
        "{}\n",
        "└────────────────────────────────────────────────────────┘"
            .yellow()
            .bold()
    );

    emit(
        Level::Warn,
        "dot.encrypt.history_warning",
        "Encryption does not remove plaintext secrets from git history",
        Some(serde_json::json!({
            "warning": "git_history_not_rewritten"
        })),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::TildePath;
    use crate::dot::config::Repo;
    use crate::dot::test_util::{EnvGuard, setup_encrypt_test_env};
    use crate::dot::types::RepoMetaData;
    use age::secrecy::ExposeSecret;
    use serial_test::serial;
    use tempfile::tempdir;

    #[test]
    #[serial]
    fn encrypt_dotfile_converts_plain_source_to_age_source() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        let repos_dir = dir.path().join("repos");
        let repo_dir = repos_dir.join("test-repo");
        let dots_dir = repo_dir.join("dots");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&dots_dir).unwrap();
        fs::write(repo_dir.join("instantdots.toml"), "").unwrap();
        fs::write(dots_dir.join("secret.txt"), "target secret").unwrap();
        fs::write(home.join("secret.txt"), "target secret").unwrap();

        std::process::Command::new("git")
            .arg("init")
            .current_dir(&repo_dir)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["add", "dots/secret.txt"])
            .current_dir(&repo_dir)
            .output()
            .unwrap();

        let identity = age::x25519::Identity::generate();
        let recipient = identity.to_public().to_string();
        let identity_file = dir.path().join("identity.txt");
        fs::write(&identity_file, identity.to_string().expose_secret()).unwrap();

        let _home_guard = EnvGuard::set("HOME", &home);
        let _age_guard = EnvGuard::set("AGE_IDENTITY", &identity_file);

        let config = DotfileConfig {
            repos: vec![Repo {
                url: "local".to_string(),
                name: "test-repo".to_string(),
                branch: None,
                active_subdirectories: Some(vec!["dots".to_string()]),
                enabled: true,
                read_only: false,
                metadata: Some(RepoMetaData {
                    name: "test-repo".to_string(),
                    dots_dirs: vec!["dots".to_string()],
                    encryption_recipients: vec![recipient],
                    ..RepoMetaData::default()
                }),
            }],
            repos_dir: TildePath::new(repos_dir),
            database_dir: TildePath::new(dir.path().join("test.db")),
            ..DotfileConfig::default()
        };
        let db = Database::new(config.database_path().to_path_buf()).unwrap();

        encrypt_dotfile(&config, &db, "secret.txt", None, None, false, false, false).unwrap();

        assert!(!dots_dir.join("secret.txt").exists());
        let encrypted_source = dots_dir.join("secret.txt.age");
        assert!(encrypted_source.exists());

        let identities = crate::dot::encryption::load_identities().unwrap();
        let plaintext =
            crate::dot::encryption::decrypt_file_to_bytes(&encrypted_source, &identities).unwrap();
        assert_eq!(plaintext, b"target secret");
    }

    #[test]
    #[serial]
    fn encrypt_dotfile_fails_on_modified_target() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("home");
        let repos_dir = dir.path().join("repos");
        let repo_dir = repos_dir.join("test-repo");
        let dots_dir = repo_dir.join("dots");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&dots_dir).unwrap();
        fs::write(repo_dir.join("instantdots.toml"), "").unwrap();
        fs::write(dots_dir.join("secret.txt"), "old source").unwrap();
        fs::write(home.join("secret.txt"), "user modified target").unwrap();

        std::process::Command::new("git")
            .arg("init")
            .current_dir(&repo_dir)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["add", "dots/secret.txt"])
            .current_dir(&repo_dir)
            .output()
            .unwrap();

        let identity = age::x25519::Identity::generate();
        let recipient = identity.to_public().to_string();
        let identity_file = dir.path().join("identity.txt");
        fs::write(&identity_file, identity.to_string().expose_secret()).unwrap();

        let _home_guard = EnvGuard::set("HOME", &home);
        let _age_guard = EnvGuard::set("AGE_IDENTITY", &identity_file);

        let config = DotfileConfig {
            repos: vec![Repo {
                url: "local".to_string(),
                name: "test-repo".to_string(),
                branch: None,
                active_subdirectories: Some(vec!["dots".to_string()]),
                enabled: true,
                read_only: false,
                metadata: Some(RepoMetaData {
                    name: "test-repo".to_string(),
                    dots_dirs: vec!["dots".to_string()],
                    encryption_recipients: vec![recipient],
                    ..RepoMetaData::default()
                }),
            }],
            repos_dir: TildePath::new(repos_dir),
            database_dir: TildePath::new(dir.path().join("test.db")),
            ..DotfileConfig::default()
        };
        let db = Database::new(config.database_path().to_path_buf()).unwrap();

        // Seed the hash for the source file to establish status tracking
        let source_hash = Dotfile::compute_hash(&dots_dir.join("secret.txt")).unwrap();
        db.add_hash(
            &source_hash,
            &dots_dir.join("secret.txt"),
            DotFileType::SourceFile,
        )
        .unwrap();

        // Attempting to encrypt should fail because the target is modified relative to source
        let result = encrypt_dotfile(&config, &db, "secret.txt", None, None, false, false, false);
        assert!(result.is_err());
        let err_msg = result.err().unwrap().to_string();
        assert!(err_msg.contains("has local modifications"));
    }

    /// Issue #1 (TOCTOU recovery): if a previous encrypt run wrote the
    /// ciphertext but crashed before removing the plaintext, a retry must
    /// recover (delete the plaintext) instead of bailing out and leaving
    /// the user wedged.
    #[test]
    #[serial]
    fn encrypt_dotfile_recovers_from_leftover_ciphertext() {
        let env = setup_encrypt_test_env();
        let cipher_path = env.dots_dir.join("secret.txt.age");

        // Seed plaintext on disk first.
        fs::write(env.dots_dir.join("secret.txt"), "leftover plain").unwrap();
        fs::write(env.home.join("secret.txt"), "leftover plain").unwrap();

        // Simulate the "crashed mid-encrypt" state: both plaintext and
        // ciphertext on disk, with the DB already containing the cipher→plain
        // mapping that the happy path would have recorded.
        let parsed = crate::dot::encryption::parse_recipients(&[env.recipient]).unwrap();
        let cipher_bytes =
            crate::dot::encryption::encrypt_bytes_to_armored(b"leftover plain", &parsed).unwrap();
        fs::write(&cipher_path, &cipher_bytes).unwrap();
        let cipher_hash = Dotfile::compute_hash(&cipher_path).unwrap();
        let plain_hash = Dotfile::hash_bytes(b"leftover plain");
        env.db
            .record_encrypted_source(&cipher_hash, &plain_hash)
            .unwrap();

        encrypt_dotfile(
            &env.config,
            &env.db,
            "secret.txt",
            None,
            None,
            false,
            false,
            false,
        )
        .unwrap();

        // Recovery should have removed the plaintext and kept the existing
        // ciphertext on disk untouched.
        assert!(!env.dots_dir.join("secret.txt").exists());
        assert!(cipher_path.exists());
    }

    /// If the leftover ciphertext does NOT match the current plaintext we
    /// must NOT silently overwrite either file — the user has two divergent
    /// versions and needs to choose. The error message should explain how.
    #[test]
    #[serial]
    fn encrypt_dotfile_bails_when_leftover_ciphertext_diverges() {
        let env = setup_encrypt_test_env();
        let cipher_path = env.dots_dir.join("secret.txt.age");

        // Plaintext on disk now differs from what the ciphertext encrypts.
        fs::write(env.dots_dir.join("secret.txt"), "modified plain").unwrap();
        fs::write(env.home.join("secret.txt"), "modified plain").unwrap();

        let parsed = crate::dot::encryption::parse_recipients(&[env.recipient]).unwrap();
        let cipher_bytes =
            crate::dot::encryption::encrypt_bytes_to_armored(b"original plain", &parsed).unwrap();
        fs::write(&cipher_path, &cipher_bytes).unwrap();
        let cipher_hash = Dotfile::compute_hash(&cipher_path).unwrap();
        // DB recorded the cipher as encrypting the ORIGINAL plain.
        env.db
            .record_encrypted_source(&cipher_hash, &Dotfile::hash_bytes(b"original plain"))
            .unwrap();

        let result = encrypt_dotfile(
            &env.config,
            &env.db,
            "secret.txt",
            None,
            None,
            false,
            false,
            false,
        );
        let err = result.expect_err("divergent recovery must bail");
        let msg = format!("{err:#}");
        assert!(msg.contains("does NOT match"), "got: {msg}");
        // Both files should still be on disk untouched.
        assert!(env.dots_dir.join("secret.txt").exists());
        assert!(cipher_path.exists());
    }
}
