use crate::dot::config::DotfileConfig;
use crate::dot::db::{Database, DotFileType};
use crate::dot::dotfile::{Dotfile, SourceKind};
use crate::dot::dotfilerepo::DotfileRepo;
use crate::dot::utils::resolve_dotfile_path;
use crate::ui::prelude::*;
use anyhow::{Context, Result, anyhow};
use colored::Colorize;
use std::fs;

pub fn decrypt_dotfile(
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
    // disk" BEFORE the SourceKind check so we can recover from a previously
    // interrupted decrypt regardless of which file the resolver picked.
    let (plain_candidate, cipher_candidate): (std::path::PathBuf, std::path::PathBuf) =
        match dotfile.kind {
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
            SourceKind::Plain => {
                let cipher = crate::dot::encryption::append_age_suffix(&dotfile.source_path);
                (dotfile.source_path.clone(), cipher)
            }
        };

    if plain_candidate.exists() && cipher_candidate.exists() {
        match try_recover_decrypt_leftover(db, &plain_candidate, &cipher_candidate)? {
            DecryptRecovery::Recovered => {
                emit(
                    Level::Info,
                    "dot.decrypt.recovered_leftover",
                    &format!(
                        "{} Recovered from interrupted previous decrypt: {} already matches {}",
                        char::from(NerdFont::Info),
                        plain_candidate.display().to_string().cyan(),
                        cipher_candidate.display().to_string().cyan(),
                    ),
                    Some(serde_json::json!({
                        "decrypted_source": plain_candidate.display().to_string(),
                        "removed_ciphertext": cipher_candidate.display().to_string(),
                    })),
                );
                return Ok(());
            }
            DecryptRecovery::Diverged => {
                anyhow::bail!(
                    "plaintext source file already exists and does NOT match the encrypted source: {}\n\
This usually means a previous decrypt was interrupted, or the file was edited \
out-of-band. To recover, either:\n  \
- remove {} (drops new plaintext changes), or\n  \
- remove {} (drops the encrypted source) and re-run.",
                    plain_candidate.display(),
                    plain_candidate.display(),
                    cipher_candidate.display(),
                );
            }
            DecryptRecovery::Unknown => {
                anyhow::bail!(
                    "plaintext source file already exists: {}\n\
The previous decrypt may have been interrupted. If you are certain {} is the \
correct decryption of {}, delete the encrypted file and re-run; otherwise \
delete the plaintext file and re-run.",
                    plain_candidate.display(),
                    plain_candidate.display(),
                    cipher_candidate.display(),
                );
            }
        }
    }

    if dotfile.kind != SourceKind::Age {
        anyhow::bail!(
            "{} is already backed by a plaintext source: {}",
            display_target(&dotfile),
            dotfile.source_path.display()
        );
    }

    if !dotfile.source_path.exists() {
        anyhow::bail!(
            "encrypted source file does not exist for {}: {}",
            display_target(&dotfile),
            dotfile.source_path.display()
        );
    }

    let repo_name = crate::dot::git::get_repo_name_for_dotfile(&dotfile, config);
    let repo_config = config
        .repos
        .iter()
        .find(|r| r.name == repo_name.as_str())
        .ok_or_else(|| anyhow!("repository '{}' not found in config", repo_name))?;
    if repo_config.read_only {
        anyhow::bail!("repository '{}' is read-only", repo_name);
    }

    let dotfile_repo = DotfileRepo::new(config, repo_name.to_string())?;

    // Check if target exists and verify it doesn't have uncommitted modifications that would be lost
    if dotfile.target_path.exists() {
        let is_unmodified = dotfile
            .is_target_unmodified(db)
            .context("verifying target modification state")?;
        if !is_unmodified {
            anyhow::bail!(
                "Target file {} has local modifications. Fetch changes with `ins dot add` or reset before decrypting.",
                dotfile.target_path.display()
            );
        }
    }

    // Determine the plain source path by stripping the .age suffix.
    // Dual-existence recovery is handled at the top of this function.
    let plain_source_path = crate::dot::encryption::strip_age_suffix(&dotfile.source_path)
        .ok_or_else(|| {
            anyhow!(
                "encrypted source file path does not end in '.age': {}",
                dotfile.source_path.display()
            )
        })?;
    if plain_source_path.exists() {
        anyhow::bail!(
            "plaintext source file already exists: {}",
            plain_source_path.display()
        );
    }

    // Load identities for decryption
    let identities =
        crate::dot::encryption::load_identities().context("loading age decryption identities")?;

    // Decrypt source to memory
    let plaintext = crate::dot::encryption::decrypt_file_to_bytes(&dotfile.source_path, &identities)
        .with_context(|| {
            format!(
                "decrypting {} — please verify that your encryption key is correctly configured in ~/.config/instant/encryption/identity or $AGE_IDENTITY",
                dotfile.source_path.display()
            )
        })?;

    if dry_run {
        emit(
            Level::Info,
            "dot.decrypt.dry_run",
            &format!(
                "{} Would decrypt {} -> {}",
                char::from(NerdFont::Info),
                dotfile.source_path.display().to_string().cyan(),
                plain_source_path.display().to_string().cyan()
            ),
            Some(serde_json::json!({
                "target": display_target(&dotfile),
                "source": dotfile.source_path.display().to_string(),
                "decrypted_source": plain_source_path.display().to_string(),
                "repo": repo_name.as_str(),
                "dry_run": true
            })),
        );
        return Ok(());
    }

    // Compute cipher hash before deleting the file so we can clean up cache mapping
    let cipher_hash = Dotfile::compute_hash(&dotfile.source_path)?;

    // Persist plaintext source file
    crate::dot::utils::persist_file_safely(&plain_source_path, &plaintext, "plaintext source")?;

    // Remove old .age source file
    fs::remove_file(&dotfile.source_path).with_context(|| {
        format!(
            "removing encrypted source {}",
            dotfile.source_path.display()
        )
    })?;

    // Update SQLite database tracking
    db.remove_hashes_for_path(&dotfile.source_path)?;
    db.delete_encrypted_source(&cipher_hash)?;

    let plain_hash = Dotfile::hash_bytes(&plaintext);
    db.add_hash(&plain_hash, &plain_source_path, DotFileType::SourceFile)?;
    if dotfile.target_path.exists() {
        db.add_hash(&plain_hash, &dotfile.target_path, DotFileType::TargetFile)?;
    }

    // Stage changes in Git
    let repo_path = dotfile_repo.local_path(config)?;
    crate::dot::git::repo_ops::git_add(&repo_path, &dotfile.source_path, debug)
        .with_context(|| format!("staging deletion of {}", dotfile.source_path.display()))?;
    crate::dot::git::repo_ops::git_add(&repo_path, &plain_source_path, debug)
        .with_context(|| format!("staging {}", plain_source_path.display()))?;

    emit(
        Level::Success,
        "dot.decrypt.complete",
        &format!(
            "{} Decrypted {}",
            char::from(NerdFont::Check),
            display_target(&dotfile).green()
        ),
        Some(serde_json::json!({
            "target": display_target(&dotfile),
            "source_removed": dotfile.source_path.display().to_string(),
            "decrypted_source": plain_source_path.display().to_string(),
            "repo": repo_name.as_str()
        })),
    );

    print_decrypt_history_warning();

    Ok(())
}

fn print_decrypt_history_warning() {
    let warn_icon = NerdFont::ShieldLock.to_string();
    println!();
    println!(
        "  {}  {}",
        warn_icon.red().bold(),
        "SECURITY WARNING".red().bold()
    );
    println!(
        "{}",
        "  ────────────────────────────────────────────────────────".red()
    );
    println!(
        "{}",
        "  You have decrypted a tracked secret to PLAINTEXT.".red()
    );
    println!(
        "{}",
        "  If you commit and push this change to a remote, your".red()
    );
    println!(
        "{}",
        "  raw plaintext secrets will be publicly EXPOSED!".red()
    );
    println!();
    println!(
        "{}",
        "  Be extremely careful not to stage or commit this".red()
    );
    println!(
        "{}",
        "  plaintext file unless you explicitly intend to.".red()
    );
    println!(
        "{}",
        "  ────────────────────────────────────────────────────────".red()
    );
    println!();

    emit(
        Level::Warn,
        "dot.decrypt.plaintext_warning",
        "Decryption will expose plaintext secrets in repository git history",
        Some(serde_json::json!({
            "warning": "plaintext_exposed_in_git"
        })),
    );
}

/// Outcome of attempting to recover from a leftover plaintext file produced
/// by a previously interrupted decrypt run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DecryptRecovery {
    /// The on-disk plaintext is known (via the DB) to be the decryption of
    /// the ciphertext currently on disk. Ciphertext was removed and the
    /// database was re-synced; the caller can return success.
    Recovered,
    /// The on-disk plaintext differs from the recorded decryption of the
    /// ciphertext. Caller must bail with a divergence error.
    Diverged,
    /// We have no DB record for the on-disk ciphertext, so we can't safely
    /// decide without identities. Caller must bail with recovery
    /// instructions.
    Unknown,
}

fn try_recover_decrypt_leftover(
    db: &Database,
    plain_path: &std::path::Path,
    cipher_path: &std::path::Path,
) -> Result<DecryptRecovery> {
    let cipher_hash = Dotfile::compute_hash(cipher_path)
        .with_context(|| format!("hashing encrypted source {}", cipher_path.display()))?;
    let recorded_plain = db.get_plain_hash_for_cipher(&cipher_hash)?;
    let Some(recorded_plain) = recorded_plain else {
        return Ok(DecryptRecovery::Unknown);
    };

    let plain_bytes = fs::read(plain_path)
        .with_context(|| format!("reading leftover plaintext source {}", plain_path.display()))?;
    let current_plain = Dotfile::hash_bytes(&plain_bytes);

    if current_plain != recorded_plain {
        return Ok(DecryptRecovery::Diverged);
    }

    // Match: the leftover plaintext is the decryption of the ciphertext.
    // Finish the previously interrupted run by removing the ciphertext and
    // re-syncing the source-side bookkeeping.
    fs::remove_file(cipher_path).with_context(|| {
        format!(
            "removing leftover encrypted source {} during recovery",
            cipher_path.display()
        )
    })?;
    db.delete_encrypted_source(&cipher_hash)?;
    db.add_hash(&current_plain, plain_path, DotFileType::SourceFile)?;
    Ok(DecryptRecovery::Recovered)
}

fn display_target(dotfile: &Dotfile) -> String {
    crate::dot::display_path(&dotfile.target_path, dotfile.is_root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::TildePath;
    use crate::dot::config::Repo;
    use crate::dot::operations::encrypt::encrypt_dotfile;
    use crate::dot::test_util::{EnvGuard, setup_encrypt_test_env};
    use crate::dot::types::RepoMetaData;
    use age::secrecy::ExposeSecret;
    use serial_test::serial;
    use tempfile::tempdir;

    #[test]
    #[serial]
    fn test_decrypt_dotfile_converts_age_source_to_plain_source() {
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

        // 1. Encrypt first
        encrypt_dotfile(&config, &db, "secret.txt", None, None, false, false, false).unwrap();

        assert!(!dots_dir.join("secret.txt").exists());
        assert!(dots_dir.join("secret.txt.age").exists());

        // 2. Decrypt back
        decrypt_dotfile(&config, &db, "secret.txt", None, None, false, false, false).unwrap();

        assert!(dots_dir.join("secret.txt").exists());
        assert!(!dots_dir.join("secret.txt.age").exists());

        let plaintext_content = fs::read_to_string(dots_dir.join("secret.txt")).unwrap();
        assert_eq!(plaintext_content, "target secret");

        // Verify SQLite hashes
        assert!(
            db.source_hash_exists_anywhere(&Dotfile::hash_bytes(b"target secret"))
                .unwrap()
        );
        assert!(!db.source_hash_exists_anywhere("invalid").unwrap());
    }

    /// Issue #1 (TOCTOU recovery): if a previous decrypt run wrote the
    /// plaintext but crashed before removing the ciphertext, a retry must
    /// recover (delete the ciphertext) instead of bailing.
    #[test]
    #[serial]
    fn decrypt_dotfile_recovers_from_leftover_plaintext() {
        let env = setup_encrypt_test_env();
        let cipher_path = env.dots_dir.join("secret.txt.age");
        let plain_path = env.dots_dir.join("secret.txt");

        // Build the "crashed mid-decrypt" state: both .age ciphertext and
        // the matching plaintext on disk, with the DB pre-populated with
        // the cipher→plain mapping.
        let parsed = crate::dot::encryption::parse_recipients(&[env.recipient]).unwrap();
        let cipher_bytes =
            crate::dot::encryption::encrypt_bytes_to_armored(b"leftover plain", &parsed).unwrap();
        fs::write(&cipher_path, &cipher_bytes).unwrap();
        fs::write(&plain_path, "leftover plain").unwrap();
        fs::write(env.home.join("secret.txt"), "leftover plain").unwrap();

        let cipher_hash = Dotfile::compute_hash(&cipher_path).unwrap();
        let plain_hash = Dotfile::hash_bytes(b"leftover plain");
        env.db
            .record_encrypted_source(&cipher_hash, &plain_hash)
            .unwrap();

        decrypt_dotfile(
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

        // Recovery should have removed the ciphertext and kept the
        // matching plaintext untouched.
        assert!(plain_path.exists());
        assert!(!cipher_path.exists());
    }

    /// Divergent state: leftover plaintext does NOT match what the
    /// ciphertext decrypts to. Decrypt must bail without touching either
    /// file.
    #[test]
    #[serial]
    fn decrypt_dotfile_bails_when_leftover_plaintext_diverges() {
        let env = setup_encrypt_test_env();
        let cipher_path = env.dots_dir.join("secret.txt.age");
        let plain_path = env.dots_dir.join("secret.txt");

        let parsed = crate::dot::encryption::parse_recipients(&[env.recipient]).unwrap();
        let cipher_bytes =
            crate::dot::encryption::encrypt_bytes_to_armored(b"original plain", &parsed).unwrap();
        fs::write(&cipher_path, &cipher_bytes).unwrap();
        // Leftover plaintext doesn't match what cipher decrypts to.
        fs::write(&plain_path, "modified plain").unwrap();
        fs::write(env.home.join("secret.txt"), "modified plain").unwrap();

        let cipher_hash = Dotfile::compute_hash(&cipher_path).unwrap();
        env.db
            .record_encrypted_source(&cipher_hash, &Dotfile::hash_bytes(b"original plain"))
            .unwrap();

        let result = decrypt_dotfile(
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
        assert!(plain_path.exists());
        assert!(cipher_path.exists());
    }
}
