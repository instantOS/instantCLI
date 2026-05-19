//! Age-encrypted dotfile support.
//!
//! A source file in a dotfile repository whose filename ends with `.age` is
//! treated as an age-encrypted blob. The corresponding target path drops the
//! `.age` suffix; `apply` decrypts the source into the target.
//!
//! For modified-tracking, both source and target hashes recorded in the
//! `file_hashes` table are *plaintext* sha256. A separate `encrypted_sources`
//! table maps `cipher_hash → plain_hash` so we only have to decrypt when the
//! ciphertext on disk actually changes (e.g. after `git pull`), and never
//! during plain `status` / `diff` operations.
//!
//! Identity discovery for v1:
//!   1. `$AGE_IDENTITY` env var (colon-separated paths, like `ssh-add` /
//!      the `age(1)` CLI).
//!   2. `<instant_config_dir>/dots.toml` `encryption_keys` list, if present.
//!   3. `<instant_config_dir>/encryption/identity` (single file) if it exists.
//!   4. `<instant_config_dir>/encryption/identities/*` (every file in the dir) if
//!      the directory exists.
//!
//! SSH agent and passphrase prompting are explicitly out of scope for v1 —
//! `apply` runs from the autostart path and must never block on user input.

use anyhow::{Context, Result, anyhow};
use std::ffi::OsStr;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::common::paths;

/// File extension that marks a source file as age-encrypted.
pub const AGE_EXTENSION: &str = "age";

/// Categorized reason for a failure while handling encrypted dotfiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptedFailureReason {
    IdentityNotConfigured,
    IdentityUnreadable,
    IdentityMismatch,
    CiphertextInvalid,
    IoFailure,
    Unknown,
}

impl EncryptedFailureReason {
    pub fn code(self) -> &'static str {
        match self {
            EncryptedFailureReason::IdentityNotConfigured => "identity_not_configured",
            EncryptedFailureReason::IdentityUnreadable => "identity_unreadable",
            EncryptedFailureReason::IdentityMismatch => "identity_mismatch",
            EncryptedFailureReason::CiphertextInvalid => "ciphertext_invalid",
            EncryptedFailureReason::IoFailure => "io_failure",
            EncryptedFailureReason::Unknown => "unknown_error",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            EncryptedFailureReason::IdentityNotConfigured => "identity required (not configured)",
            EncryptedFailureReason::IdentityUnreadable => "identity error (unreadable/invalid)",
            EncryptedFailureReason::IdentityMismatch => "identity required (no matching key)",
            EncryptedFailureReason::CiphertextInvalid => "encrypted source is invalid/corrupted",
            EncryptedFailureReason::IoFailure => "I/O error while processing encrypted source",
            EncryptedFailureReason::Unknown => "encrypted source processing failed",
        }
    }

    pub fn is_identity_related(self) -> bool {
        matches!(
            self,
            EncryptedFailureReason::IdentityNotConfigured
                | EncryptedFailureReason::IdentityUnreadable
                | EncryptedFailureReason::IdentityMismatch
        )
    }
}

/// Classify an encrypted dotfile processing error into a stable user-facing reason.
pub fn classify_encrypted_failure(err: &anyhow::Error) -> EncryptedFailureReason {
    let root_message = err.to_string().to_lowercase();
    if root_message.contains("no local encryption key found")
        || root_message.contains("no encryption keys were found")
        || root_message.contains("no age identities configured")
    {
        return EncryptedFailureReason::IdentityNotConfigured;
    }

    if root_message.contains("reading age identity file")
        || root_message.contains("parsing age identity file")
    {
        return EncryptedFailureReason::IdentityUnreadable;
    }

    for cause in err.chain() {
        if let Some(decrypt_err) = cause.downcast_ref::<age::DecryptError>() {
            return match decrypt_err {
                age::DecryptError::NoMatchingKeys => EncryptedFailureReason::IdentityMismatch,
                age::DecryptError::Io(_) => EncryptedFailureReason::IoFailure,
                age::DecryptError::DecryptionFailed
                | age::DecryptError::ExcessiveWork { .. }
                | age::DecryptError::InvalidHeader
                | age::DecryptError::InvalidMac
                | age::DecryptError::KeyDecryptionFailed
                | age::DecryptError::UnknownFormat => EncryptedFailureReason::CiphertextInvalid,
            };
        }
    }

    for cause in err.chain() {
        if cause.downcast_ref::<std::io::Error>().is_some() {
            return EncryptedFailureReason::IoFailure;
        }
    }

    if root_message.contains("no matching identity") || root_message.contains("no matching key") {
        return EncryptedFailureReason::IdentityMismatch;
    }

    if root_message.contains("parsing age header")
        || root_message.contains("invalid age")
        || root_message.contains("decrypting ")
    {
        return EncryptedFailureReason::CiphertextInvalid;
    }

    EncryptedFailureReason::Unknown
}

/// Returns `true` if the path's final extension is `.age`.
pub fn is_encrypted_source(path: &Path) -> bool {
    path.extension().and_then(OsStr::to_str) == Some(AGE_EXTENSION)
}

/// Given an encrypted source path like `foo.toml.age`, return the corresponding
/// target filename `foo.toml`. Returns `None` if the path is not `.age`-suffixed
/// or has no file name.
pub fn strip_age_suffix(path: &Path) -> Option<PathBuf> {
    if !is_encrypted_source(path) {
        return None;
    }
    let parent = path.parent();
    let file_name = path.file_name()?.to_str()?;
    // file_name ends in ".age" because is_encrypted_source returned true.
    let stripped = &file_name[..file_name.len() - (AGE_EXTENSION.len() + 1)];
    if stripped.is_empty() {
        return None;
    }
    Some(match parent {
        Some(p) if !p.as_os_str().is_empty() => p.join(stripped),
        _ => PathBuf::from(stripped),
    })
}

/// Given a plain target path like `foo.toml`, return the `.age`-suffixed
/// source candidate `foo.toml.age` (for searching alternative repos).
pub fn append_age_suffix(path: &Path) -> PathBuf {
    let mut bytes = path.as_os_str().to_os_string();
    bytes.push(".");
    bytes.push(AGE_EXTENSION);
    PathBuf::from(bytes)
}

/// Discover identity files in the order described at the top of the module.
///
/// Returns the absolute paths of every identity file that exists and is
/// readable; does not parse them. Empty list is a normal outcome — callers
/// must distinguish "no identities configured" from "identities failed to
/// decrypt" themselves.
///
/// Sources (in priority order, deduplicated):
///   1. `$AGE_IDENTITY` env var (colon-separated paths)
///   2. `dots.toml` `encryption_keys` list (loaded from the global config)
///   3. `<instant_config_dir>/encryption/identity`
///   4. `<instant_config_dir>/encryption/identities/*`
pub fn discover_identity_files() -> Vec<PathBuf> {
    let mut out = Vec::new();

    // 1. $AGE_IDENTITY env var
    if let Ok(val) = std::env::var("AGE_IDENTITY") {
        for raw in val.split(':') {
            let raw = raw.trim();
            if raw.is_empty() {
                continue;
            }
            let expanded = PathBuf::from(shellexpand::tilde(raw).into_owned());
            if expanded.is_file() {
                out.push(expanded);
            }
        }
    }

    // 2. dots.toml encryption_keys
    if let Ok(config) = crate::dot::config::DotfileConfig::load(None) {
        for raw in &config.encryption_keys {
            let expanded = PathBuf::from(shellexpand::tilde(raw).into_owned());
            if expanded.is_file() && !out.iter().any(|p| p == &expanded) {
                out.push(expanded);
            }
        }
    }

    // 3-4. Default paths under <instant_config_dir>/encryption/
    if let Ok(cfg_dir) = paths::instant_config_dir() {
        let single = cfg_dir.join("encryption").join("identity");
        if single.is_file() && !out.iter().any(|p| p == &single) {
            out.push(single);
        }
        let dir = cfg_dir.join("encryption").join("identities");
        if dir.is_dir()
            && let Ok(entries) = std::fs::read_dir(&dir)
        {
            let mut files: Vec<PathBuf> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_file())
                .collect();
            files.sort();
            for p in files {
                if !out.iter().any(|q| q == &p) {
                    out.push(p);
                }
            }
        }
    }

    out
}

/// Load and parse every discovered identity file into an in-memory list of
/// age identities. Returns `Ok(Vec::new())` if no identity files are present.
///
/// Note: this is intentionally not cached. Identity files are tiny and
/// parsing is microseconds; calling this once per `apply_all` invocation is
/// fine. Avoiding caching also sidesteps `Send`/`Sync` issues with the
/// `dyn age::Identity` trait object.
pub fn load_identities() -> Result<Vec<Box<dyn age::Identity>>> {
    let files = discover_identity_files();
    let mut all: Vec<Box<dyn age::Identity>> = Vec::new();
    for path in &files {
        let name = path.to_string_lossy().into_owned();
        let idf = age::IdentityFile::from_file(name.clone())
            .with_context(|| format!("reading age identity file {}", path.display()))?;
        let mut parsed = idf
            .into_identities()
            .with_context(|| format!("parsing age identity file {}", path.display()))?;
        all.append(&mut parsed);
    }
    Ok(all)
}

/// Parse public encryption recipients from repository metadata.
///
/// Supports native X25519 recipients (`age1...`) and SSH public keys
/// (`ssh-ed25519 ...`, `ssh-rsa ...`) through the `age` crate.
pub fn parse_recipients(raw_recipients: &[String]) -> Result<Vec<Box<dyn age::Recipient>>> {
    let mut recipients: Vec<Box<dyn age::Recipient>> = Vec::new();

    for raw in raw_recipients {
        let recipient = raw.trim();
        if recipient.is_empty() {
            continue;
        }

        if recipient.starts_with("age1") {
            let parsed = age::x25519::Recipient::from_str(recipient)
                .map_err(|err| anyhow!("invalid encryption recipient '{}': {}", recipient, err))?;
            recipients.push(Box::new(parsed));
            continue;
        }

        if recipient.starts_with("ssh-") {
            let parsed = age::ssh::Recipient::from_str(recipient).map_err(|err| {
                anyhow!(
                    "invalid SSH encryption recipient '{}': {:?}",
                    recipient,
                    err
                )
            })?;
            recipients.push(Box::new(parsed));
            continue;
        }

        return Err(anyhow!(
            "unsupported encryption recipient '{}': expected an age1... key or SSH public key",
            recipient
        ));
    }

    if recipients.is_empty() {
        return Err(anyhow!("no encryption recipients configured"));
    }

    Ok(recipients)
}

/// Encrypt plaintext bytes to ASCII-armored age ciphertext.
pub fn encrypt_bytes_to_armored(
    plaintext: &[u8],
    recipients: &[Box<dyn age::Recipient>],
) -> Result<Vec<u8>> {
    if recipients.is_empty() {
        return Err(anyhow!("no encryption recipients configured"));
    }

    let encryptor = age::Encryptor::with_recipients(
        recipients
            .iter()
            .map(|recipient| recipient.as_ref() as &dyn age::Recipient),
    )?;
    let mut ciphertext = Vec::new();
    let armored =
        age::armor::ArmoredWriter::wrap_output(&mut ciphertext, age::armor::Format::AsciiArmor)?;
    let mut writer = encryptor.wrap_output(armored)?;
    writer.write_all(plaintext)?;
    writer.finish()?.finish()?;

    Ok(ciphertext)
}

/// Decrypt an age-encrypted file at `cipher_path` to an owned byte buffer,
/// using the given identities. Returns an error if no identity matches the
/// file's recipients, or if the file is malformed.
pub fn decrypt_file_to_bytes(
    cipher_path: &Path,
    identities: &[Box<dyn age::Identity>],
) -> Result<Vec<u8>> {
    if identities.is_empty() {
        return Err(anyhow!(
            "No local encryption key found. Please run 'ins dot encrypt generate' first, or set $AGE_IDENTITY."
        ));
    }
    let file = File::open(cipher_path)
        .with_context(|| format!("opening encrypted file {}", cipher_path.display()))?;
    let decryptor = age::Decryptor::new_buffered(age::armor::ArmoredReader::new(file))
        .with_context(|| format!("parsing age header of {}", cipher_path.display()))?;
    let mut reader = decryptor
        .decrypt(identities.iter().map(|i| i.as_ref() as &dyn age::Identity))
        .with_context(|| {
            format!(
                "decrypting {} — no matching identity?",
                cipher_path.display()
            )
        })?;
    let mut out = Vec::new();
    reader
        .read_to_end(&mut out)
        .with_context(|| format!("reading decrypted stream from {}", cipher_path.display()))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_age_suffix_basic() {
        assert_eq!(
            strip_age_suffix(Path::new("/a/b/foo.toml.age")),
            Some(PathBuf::from("/a/b/foo.toml"))
        );
        assert_eq!(
            strip_age_suffix(Path::new("foo.toml.age")),
            Some(PathBuf::from("foo.toml"))
        );
        assert_eq!(
            strip_age_suffix(Path::new("foo.age")),
            Some(PathBuf::from("foo"))
        );
        assert_eq!(strip_age_suffix(Path::new("foo.toml")), None);
        assert_eq!(strip_age_suffix(Path::new(".age")), None);
    }

    #[test]
    fn append_age_suffix_basic() {
        assert_eq!(
            append_age_suffix(Path::new("/a/b/foo.toml")),
            PathBuf::from("/a/b/foo.toml.age")
        );
        assert_eq!(
            append_age_suffix(Path::new("foo")),
            PathBuf::from("foo.age")
        );
    }

    #[test]
    fn is_encrypted_source_basic() {
        assert!(is_encrypted_source(Path::new("foo.toml.age")));
        assert!(is_encrypted_source(Path::new("foo.age")));
        assert!(!is_encrypted_source(Path::new("foo.toml")));
        assert!(!is_encrypted_source(Path::new("foo")));
    }

    #[test]
    fn parse_recipients_accepts_x25519_recipient() {
        let identity = age::x25519::Identity::generate();
        let raw = vec![identity.to_public().to_string()];

        let recipients = parse_recipients(&raw).expect("parse generated recipient");

        assert_eq!(recipients.len(), 1);
    }

    #[test]
    fn parse_recipients_rejects_empty_list() {
        let err = match parse_recipients(&[]) {
            Ok(_) => panic!("empty recipients should fail"),
            Err(err) => err,
        };

        assert!(err.to_string().contains("no age recipients configured"));
    }

    #[test]
    fn encrypt_bytes_to_armored_round_trips() {
        let identity = age::x25519::Identity::generate();
        let raw_recipients = vec![identity.to_public().to_string()];
        let recipients = parse_recipients(&raw_recipients).expect("parse recipient");

        let ciphertext =
            encrypt_bytes_to_armored(b"secret payload", &recipients).expect("encrypt payload");
        assert!(ciphertext.starts_with(b"-----BEGIN AGE ENCRYPTED FILE-----"));

        let cipher_file = tempfile::NamedTempFile::new().expect("temp cipher file");
        std::fs::write(cipher_file.path(), ciphertext).expect("write cipher file");
        let identities: Vec<Box<dyn age::Identity>> = vec![Box::new(identity)];

        let plaintext =
            decrypt_file_to_bytes(cipher_file.path(), &identities).expect("decrypt payload");

        assert_eq!(plaintext, b"secret payload");
    }
}
