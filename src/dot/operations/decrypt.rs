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

pub fn decrypt_dotfile(
    config: &DotfileConfig,
    db: &Database,
    path: &str,
    options: &ConvertOptions,
    debug: bool,
) -> Result<()> {
    let PrepareOutcome::Ready(ctx) =
        prepare_conversion(config, db, path, options, ConvertDirection::Decrypt)?
    else {
        return Ok(());
    };

    ensure_target_unmodified(db, &ctx.dotfile, "decrypting")?;
    ensure_destination_absent(&ctx.plain_path, "plaintext source file")?;

    // Load identities for decryption
    let identities =
        crate::dot::encryption::load_identities().context("loading age decryption identities")?;

    // Decrypt source to memory
    let plaintext = crate::dot::encryption::decrypt_file_to_bytes(&ctx.cipher_path, &identities)
        .with_context(|| {
            format!(
                "decrypting {} — please verify that your encryption key is in ~/.config/instant/encryption/identities/ or set $AGE_IDENTITY",
                ctx.cipher_path.display()
            )
        })?;

    if options.dry_run {
        emit(
            Level::Info,
            "dot.decrypt.dry_run",
            &format!(
                "{} Would decrypt {} -> {}",
                char::from(NerdFont::Info),
                ctx.cipher_path.display().to_string().cyan(),
                ctx.plain_path.display().to_string().cyan()
            ),
            Some(serde_json::json!({
                "target": display_target(&ctx.dotfile),
                "source": ctx.cipher_path.display().to_string(),
                "decrypted_source": ctx.plain_path.display().to_string(),
                "repo": ctx.repo_name.as_str(),
                "dry_run": true
            })),
        );
        return Ok(());
    }

    // Compute cipher hash before deleting the file so we can clean up cache mapping
    let cipher_hash = Dotfile::compute_hash(&ctx.cipher_path)?;

    // Persist plaintext source file
    crate::dot::utils::persist_file_safely(&ctx.plain_path, &plaintext, "plaintext source")?;

    // Remove old .age source file
    fs::remove_file(&ctx.cipher_path)
        .with_context(|| format!("removing encrypted source {}", ctx.cipher_path.display()))?;

    // Update SQLite database tracking
    db.remove_hashes_for_path(&ctx.cipher_path)?;
    db.delete_encrypted_source(&cipher_hash)?;

    let plain_hash = Dotfile::hash_bytes(&plaintext);
    db.add_hash(&plain_hash, &ctx.plain_path, DotFileType::SourceFile)?;
    if ctx.dotfile.target_path.exists() {
        db.add_hash(
            &plain_hash,
            &ctx.dotfile.target_path,
            DotFileType::TargetFile,
        )?;
    }

    // Stage changes in Git
    let repo_path = ctx.dotfile_repo.local_path(config)?;
    crate::dot::git::repo_ops::git_add(&repo_path, &ctx.cipher_path, debug)
        .with_context(|| format!("staging deletion of {}", ctx.cipher_path.display()))?;
    crate::dot::git::repo_ops::git_add(&repo_path, &ctx.plain_path, debug)
        .with_context(|| format!("staging {}", ctx.plain_path.display()))?;

    emit(
        Level::Success,
        "dot.decrypt.complete",
        &format!(
            "{} Decrypted {}",
            char::from(NerdFont::Check),
            display_target(&ctx.dotfile).green()
        ),
        Some(serde_json::json!({
            "target": display_target(&ctx.dotfile),
            "source_removed": ctx.cipher_path.display().to_string(),
            "decrypted_source": ctx.plain_path.display().to_string(),
            "repo": ctx.repo_name.as_str()
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

        // 1. Encrypt first
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
        assert!(dots_dir.join("secret.txt.age").exists());

        // 2. Decrypt back
        decrypt_dotfile(
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
            &ConvertOptions {
                repo: None,
                subdir: None,
                dry_run: false,
                include_root: false,
            },
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
        assert!(plain_path.exists());
        assert!(cipher_path.exists());
    }
}
