use super::convert::{
    ConvertDirection, ConvertOptions, PrepareOutcome, display_target, ensure_destination_absent,
    ensure_target_unmodified, prepare_conversion,
};
use crate::dot::config::DotfileConfig;
use crate::dot::db::{Database, DotFileType};
use crate::dot::dotfile::Dotfile;
use crate::ui::prelude::*;
use anyhow::{Context, Result};
use colored::Colorize;
use std::fs;

pub fn encrypt_dotfile(
    config: &DotfileConfig,
    db: &Database,
    path: &str,
    options: &ConvertOptions,
    debug: bool,
) -> Result<()> {
    let PrepareOutcome::Ready(ctx) =
        prepare_conversion(config, db, path, options, ConvertDirection::Encrypt)?
    else {
        return Ok(());
    };

    let recipients = crate::dot::encryption::parse_recipients(
        &ctx.dotfile_repo.meta.encryption_recipients,
    )
    .with_context(|| {
        format!(
            "repository '{}' has no usable encryption_recipients configured in instantdots.toml.\n\
                 Please authorize decryption keys first using 'ins dot keys authorize'.",
            ctx.repo_name
        )
    })?;

    ensure_destination_absent(&ctx.cipher_path, "encrypted source")?;
    ensure_target_unmodified(db, &ctx.dotfile, "encrypting")?;

    // Always encrypt the repository source file as the source of truth
    let plaintext = fs::read(&ctx.plain_path).with_context(|| {
        format!(
            "reading plaintext from source file {}",
            ctx.plain_path.display()
        )
    })?;
    let plain_hash = Dotfile::hash_bytes(&plaintext);

    if options.dry_run {
        emit(
            Level::Info,
            "dot.encrypt.dry_run",
            &format!(
                "{} Would encrypt {} -> {}",
                char::from(NerdFont::Info),
                ctx.plain_path.display().to_string().cyan(),
                ctx.cipher_path.display().to_string().cyan()
            ),
            Some(serde_json::json!({
                "target": display_target(&ctx.dotfile),
                "source": ctx.plain_path.display().to_string(),
                "encrypted_source": ctx.cipher_path.display().to_string(),
                "repo": ctx.repo_name.as_str(),
                "dry_run": true
            })),
        );
        print_history_warning();
        return Ok(());
    }

    let ciphertext = crate::dot::encryption::encrypt_bytes_to_armored(&plaintext, &recipients)
        .context("encrypting dotfile plaintext")?;
    crate::dot::utils::persist_file_safely(&ctx.cipher_path, &ciphertext, "encrypted source")?;
    fs::remove_file(&ctx.plain_path)
        .with_context(|| format!("removing plaintext source {}", ctx.plain_path.display()))?;

    let cipher_hash = Dotfile::compute_hash(&ctx.cipher_path)?;
    db.record_encrypted_source(&cipher_hash, &plain_hash)?;
    db.add_hash(&plain_hash, &ctx.cipher_path, DotFileType::SourceFile)?;
    if ctx.dotfile.target_path.exists() {
        db.add_hash(
            &plain_hash,
            &ctx.dotfile.target_path,
            DotFileType::TargetFile,
        )?;
    }

    let repo_path = ctx.dotfile_repo.local_path(config)?;
    crate::dot::git::repo_ops::git_add(&repo_path, &ctx.plain_path, debug)
        .with_context(|| format!("staging deletion of {}", ctx.plain_path.display()))?;
    crate::dot::git::repo_ops::git_add(&repo_path, &ctx.cipher_path, debug)
        .with_context(|| format!("staging {}", ctx.cipher_path.display()))?;

    emit(
        Level::Success,
        "dot.encrypt.complete",
        &format!(
            "{} Encrypted {}",
            char::from(NerdFont::Check),
            display_target(&ctx.dotfile).green()
        ),
        Some(serde_json::json!({
            "target": display_target(&ctx.dotfile),
            "source_removed": ctx.plain_path.display().to_string(),
            "encrypted_source": ctx.cipher_path.display().to_string(),
            "repo": ctx.repo_name.as_str()
        })),
    );
    print_history_warning();

    Ok(())
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
        let identity_file = dir.path().join("identity.key");
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

        encrypt_dotfile(
            &config,
            &db,
            "secret.txt",
            &ConvertOptions {
                repo: None,
                subdir: None,
                dry_run: false,
                include_root: false,
            },
            false,
        )
        .unwrap();

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
        let identity_file = dir.path().join("identity.key");
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
        let result = encrypt_dotfile(
            &config,
            &db,
            "secret.txt",
            &ConvertOptions {
                repo: None,
                subdir: None,
                dry_run: false,
                include_root: false,
            },
            false,
        );
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
            &ConvertOptions {
                repo: None,
                subdir: None,
                dry_run: false,
                include_root: false,
            },
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
            &ConvertOptions {
                repo: None,
                subdir: None,
                dry_run: false,
                include_root: false,
            },
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
