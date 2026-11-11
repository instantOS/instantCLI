use crate::dot::config::DotfileConfig;
use crate::dot::db::{Database, DotFileType};
use crate::dot::dotfile::{Dotfile, SourceKind};
use crate::dot::dotfilerepo::DotfileRepo;
use crate::dot::utils::resolve_dotfile_path;
use crate::ui::prelude::*;
use anyhow::{Context, Result, anyhow};
use colored::Colorize;
use std::fs;
use std::path::{Path, PathBuf};

/// Options shared by the encrypt/decrypt (plaintext <-> `.age`) operations.
#[derive(Debug, Default, Clone)]
pub struct ConvertOptions {
    /// Convert the source from this repository instead of the default source
    pub repo: Option<String>,
    /// Convert the source from this subdirectory (requires `repo`)
    pub subdir: Option<String>,
    /// Show what would be converted without writing files
    pub dry_run: bool,
    /// Process root-owned dotfiles
    pub include_root: bool,
}

/// Direction of a plaintext <-> `.age` conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConvertDirection {
    Encrypt,
    Decrypt,
}

impl ConvertDirection {
    fn expected_kind(self) -> SourceKind {
        match self {
            Self::Encrypt => SourceKind::Plain,
            Self::Decrypt => SourceKind::Age,
        }
    }

    fn existing_source_label(self) -> &'static str {
        match self {
            Self::Encrypt => "an encrypted",
            Self::Decrypt => "a plaintext",
        }
    }

    fn source_description(self) -> &'static str {
        match self {
            Self::Encrypt => "source",
            Self::Decrypt => "encrypted source",
        }
    }
}

/// A dotfile that has passed the shared encrypt/decrypt preflight.
pub struct ConversionContext {
    pub dotfile: Dotfile,
    pub repo_name: String,
    pub dotfile_repo: DotfileRepo,
    pub plain_path: PathBuf,
    pub cipher_path: PathBuf,
}

/// Result of preparing a conversion.
pub enum PrepareOutcome {
    /// The conversion can proceed.
    Ready(Box<ConversionContext>),
    /// A previous interrupted conversion was recovered; the caller should
    /// return success without doing more work.
    Recovered,
}

/// Resolve and validate everything shared by encrypt and decrypt before the
/// direction-specific crypto and file-writing steps.
pub fn prepare_conversion(
    config: &DotfileConfig,
    db: &Database,
    path: &str,
    options: &ConvertOptions,
    direction: ConvertDirection,
) -> Result<PrepareOutcome> {
    let target_path = resolve_dotfile_path(path, options.include_root, false)?;
    let dotfile = crate::dot::utils::resolve_dotfile_to_source(
        config,
        db,
        &target_path,
        options.repo.as_deref(),
        options.subdir.as_deref(),
        options.include_root,
    )?;

    let (plain_path, cipher_path) = plain_and_cipher_paths(&dotfile)?;

    // Detect "both plaintext and ciphertext on disk" before the kind check.
    // This can happen after a previous conversion wrote one side but crashed
    // before removing the other. Depending on which file the resolver picked,
    // the source kind may be either Plain or Age, so recovery must run first.
    if plain_path.exists() && cipher_path.exists() {
        match try_recover_leftover(db, &plain_path, &cipher_path, direction)? {
            Recovery::Recovered => {
                emit_recovery(direction, &plain_path, &cipher_path);
                return Ok(PrepareOutcome::Recovered);
            }
            Recovery::Diverged => {
                return Err(diverged_error(direction, &plain_path, &cipher_path));
            }
            Recovery::Unknown => {
                return Err(unknown_error(direction, &plain_path, &cipher_path));
            }
        }
    }

    if dotfile.kind != direction.expected_kind() {
        anyhow::bail!(
            "{} is already backed by {} source: {}",
            display_target(&dotfile),
            direction.existing_source_label(),
            dotfile.source_path.display()
        );
    }

    if !dotfile.source_path.exists() {
        anyhow::bail!(
            "{} file does not exist for {}: {}",
            direction.source_description(),
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

    Ok(PrepareOutcome::Ready(Box::new(ConversionContext {
        dotfile,
        repo_name: repo_name.to_string(),
        dotfile_repo,
        plain_path,
        cipher_path,
    })))
}

/// Verify that an existing target has no local modifications that would be
/// lost by the conversion.
pub fn ensure_target_unmodified(db: &Database, dotfile: &Dotfile, action: &str) -> Result<()> {
    if dotfile.target_path.exists() {
        let is_unmodified = dotfile
            .is_target_unmodified(db)
            .context("verifying target modification state")?;
        if !is_unmodified {
            anyhow::bail!(
                "Target file {} has local modifications. Fetch changes with `ins dot add` or reset before {}.",
                dotfile.target_path.display(),
                action
            );
        }
    }

    Ok(())
}

/// Verify that the destination side of a conversion does not already exist.
pub fn ensure_destination_absent(path: &Path, description: &str) -> Result<()> {
    if path.exists() {
        anyhow::bail!("{} already exists: {}", description, path.display());
    }
    Ok(())
}

pub fn display_target(dotfile: &Dotfile) -> String {
    crate::dot::display_path(&dotfile.target_path, dotfile.is_root)
}

fn plain_and_cipher_paths(dotfile: &Dotfile) -> Result<(PathBuf, PathBuf)> {
    match dotfile.kind {
        SourceKind::Plain => Ok((
            dotfile.source_path.clone(),
            crate::dot::encryption::append_age_suffix(&dotfile.source_path),
        )),
        SourceKind::Age => {
            let plain = crate::dot::encryption::strip_age_suffix(&dotfile.source_path).ok_or_else(
                || {
                    anyhow!(
                        "encrypted source path does not end in '.age': {}",
                        dotfile.source_path.display()
                    )
                },
            )?;
            Ok((plain, dotfile.source_path.clone()))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Recovery {
    Recovered,
    Diverged,
    Unknown,
}

fn try_recover_leftover(
    db: &Database,
    plain_path: &Path,
    cipher_path: &Path,
    direction: ConvertDirection,
) -> Result<Recovery> {
    let cipher_hash = Dotfile::compute_hash(cipher_path)
        .with_context(|| format!("hashing encrypted source {}", cipher_path.display()))?;
    let recorded_plain = db.get_plain_hash_for_cipher(&cipher_hash)?;
    let Some(recorded_plain) = recorded_plain else {
        return Ok(Recovery::Unknown);
    };

    let plaintext = fs::read(plain_path)
        .with_context(|| format!("reading plaintext source {}", plain_path.display()))?;
    let current_plain = Dotfile::hash_bytes(&plaintext);

    if current_plain != recorded_plain {
        return Ok(Recovery::Diverged);
    }

    // Match: the leftover ciphertext encrypts exactly the current plaintext.
    // Finish the interrupted run by removing the side that should not remain
    // and re-syncing the source-side bookkeeping.
    match direction {
        ConvertDirection::Encrypt => {
            fs::remove_file(plain_path).with_context(|| {
                format!(
                    "removing leftover plaintext source {} during recovery",
                    plain_path.display()
                )
            })?;
            db.add_hash(&current_plain, cipher_path, DotFileType::SourceFile)?;
        }
        ConvertDirection::Decrypt => {
            fs::remove_file(cipher_path).with_context(|| {
                format!(
                    "removing leftover encrypted source {} during recovery",
                    cipher_path.display()
                )
            })?;
            db.delete_encrypted_source(&cipher_hash)?;
            db.add_hash(&current_plain, plain_path, DotFileType::SourceFile)?;
        }
    }

    Ok(Recovery::Recovered)
}

fn emit_recovery(direction: ConvertDirection, plain_path: &Path, cipher_path: &Path) {
    match direction {
        ConvertDirection::Encrypt => emit(
            Level::Info,
            "dot.encrypt.recovered_leftover",
            &format!(
                "{} Recovered from interrupted previous encrypt: {} already matches {}",
                char::from(NerdFont::Info),
                cipher_path.display().to_string().cyan(),
                plain_path.display().to_string().cyan(),
            ),
            Some(serde_json::json!({
                "encrypted_source": cipher_path.display().to_string(),
                "removed_plaintext": plain_path.display().to_string(),
            })),
        ),
        ConvertDirection::Decrypt => emit(
            Level::Info,
            "dot.decrypt.recovered_leftover",
            &format!(
                "{} Recovered from interrupted previous decrypt: {} already matches {}",
                char::from(NerdFont::Info),
                plain_path.display().to_string().cyan(),
                cipher_path.display().to_string().cyan(),
            ),
            Some(serde_json::json!({
                "decrypted_source": plain_path.display().to_string(),
                "removed_ciphertext": cipher_path.display().to_string(),
            })),
        ),
    }
}

fn diverged_error(
    direction: ConvertDirection,
    plain_path: &Path,
    cipher_path: &Path,
) -> anyhow::Error {
    match direction {
        ConvertDirection::Encrypt => anyhow!(
            "encrypted source already exists and does NOT match the current plaintext: {}\n\
This usually means a previous encrypt was interrupted, or the file was edited \
out-of-band. To recover, either:\n  \
- remove {} (drops new plaintext changes), or\n  \
- remove {} (drops previous ciphertext) and re-run.",
            cipher_path.display(),
            plain_path.display(),
            cipher_path.display(),
        ),
        ConvertDirection::Decrypt => anyhow!(
            "plaintext source file already exists and does NOT match the encrypted source: {}\n\
This usually means a previous decrypt was interrupted, or the file was edited \
out-of-band. To recover, either:\n  \
- remove {} (drops new plaintext changes), or\n  \
- remove {} (drops the encrypted source) and re-run.",
            plain_path.display(),
            plain_path.display(),
            cipher_path.display(),
        ),
    }
}

fn unknown_error(
    direction: ConvertDirection,
    plain_path: &Path,
    cipher_path: &Path,
) -> anyhow::Error {
    match direction {
        ConvertDirection::Encrypt => anyhow!(
            "encrypted source already exists: {}\n\
The previous encrypt may have been interrupted. If you are certain the \
plaintext at {} is identical to the contents of the ciphertext, delete the \
plaintext file and re-run; otherwise delete the ciphertext file and re-run.",
            cipher_path.display(),
            plain_path.display(),
        ),
        ConvertDirection::Decrypt => anyhow!(
            "plaintext source file already exists: {}\n\
The previous decrypt may have been interrupted. If you are certain {} is the \
correct decryption of {}, delete the encrypted file and re-run; otherwise \
delete the plaintext file and re-run.",
            plain_path.display(),
            plain_path.display(),
            cipher_path.display(),
        ),
    }
}
