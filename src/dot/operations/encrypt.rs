use crate::common::home_dir;
use crate::dot::config::DotfileConfig;
use crate::dot::db::{Database, DotFileType};
use crate::dot::dotfile::{Dotfile, SourceKind};
use crate::dot::dotfilerepo::DotfileRepo;
use crate::dot::override_config::DotfileSource;
use crate::dot::utils::{get_all_dotfiles, resolve_dotfile_path};
use crate::ui::prelude::*;
use anyhow::{Context, Result, anyhow};
use colored::Colorize;
use std::fs;
use std::io::Write;
use std::path::Path;

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
    let target_path = resolve_dotfile_path(path, include_root)?;
    let dotfile = crate::dot::utils::resolve_dotfile_to_source(
        config,
        db,
        &target_path,
        repo,
        subdir,
        include_root,
    )?;

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
    let recipients = crate::dot::encryption::parse_recipients(&dotfile_repo.meta.age_recipients)
        .with_context(|| {
            format!(
                "repository '{}' has no usable age_recipients in instantdots.toml",
                repo_name
            )
        })?;

    let encrypted_source_path = crate::dot::encryption::append_age_suffix(&dotfile.source_path);
    if encrypted_source_path.exists() {
        anyhow::bail!(
            "encrypted source already exists: {}",
            encrypted_source_path.display()
        );
    }

    // Always encrypt the repository source file as the source of truth
    let plaintext = fs::read(&dotfile.source_path).with_context(|| {
        format!(
            "reading plaintext from source file {}",
            dotfile.source_path.display()
        )
    })?;
    let plain_hash = Dotfile::hash_bytes(&plaintext);

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

fn display_target(dotfile: &Dotfile) -> String {
    crate::dot::display_path(&dotfile.target_path, dotfile.is_root)
}

fn print_history_warning() {
    emit(
        Level::Warn,
        "dot.encrypt.history_warning",
        &format!(
            "{} Encryption does not remove plaintext secrets from git history",
            char::from(NerdFont::Warning)
        ),
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

        let prev_home = std::env::var_os("HOME");
        let prev_age = std::env::var_os("AGE_IDENTITY");
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("AGE_IDENTITY", &identity_file);
        }

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
                    age_recipients: vec![recipient],
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

        unsafe {
            match prev_home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
            match prev_age {
                Some(v) => std::env::set_var("AGE_IDENTITY", v),
                None => std::env::remove_var("AGE_IDENTITY"),
            }
        }
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

        let prev_home = std::env::var_os("HOME");
        let prev_age = std::env::var_os("AGE_IDENTITY");
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("AGE_IDENTITY", &identity_file);
        }

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
                    age_recipients: vec![recipient],
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

        unsafe {
            match prev_home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
            match prev_age {
                Some(v) => std::env::set_var("AGE_IDENTITY", v),
                None => std::env::remove_var("AGE_IDENTITY"),
            }
        }
    }
}
