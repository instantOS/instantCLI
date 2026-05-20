//! Encryption key and recipient management operations.
//!
//! Sub-modules:
//! - `generate` — key creation (`handle_init`)
//! - `discover` — identity discovery (`discover_all_keys_info`, `get_local_public_keys`)
//! - `authorize` — authorize / de-authorize recipients in repos
//! - `rotate`   — re-encrypt repos with a new recipient set
//! - `manage`   — rename, remove, lookup repos using a key
//! - `status`   — display commands (list, status, show)
//! - `types`    — shared types (`KeyInfo`, `KeyType`)
//! - `util`     — internal helpers (`identities_dir`, `find_age_files`)

pub(crate) mod util;

pub(crate) mod authorize;
pub(crate) mod discover;
pub(crate) mod generate;
pub(crate) mod manage;
pub(crate) mod rotate;
pub(crate) mod status;
pub(crate) mod types;

pub use discover::{discover_all_keys_info, get_local_public_keys};
pub use generate::handle_init;
pub use manage::{find_repos_using_key, handle_rename};
pub use status::{handle_identity, handle_list, handle_status};
pub use types::KeyType;

pub(crate) use authorize::{handle_authorize, handle_deauthorize};
pub(crate) use manage::handle_remove;
pub(crate) use rotate::handle_rotate;

use crate::dot::commands::EncryptCommands;
use crate::dot::config::DotfileConfig;
use crate::dot::db::Database;
use anyhow::Result;

pub fn handle_encrypt_command(
    config: &DotfileConfig,
    db: &Database,
    command: &EncryptCommands,
    debug: bool,
) -> Result<()> {
    match command {
        EncryptCommands::Generate { name, force } => handle_init(name.as_deref(), *force),
        EncryptCommands::List => handle_list(config),
        EncryptCommands::Rename { old_name, new_name } => handle_rename(old_name, new_name),
        EncryptCommands::Remove { name } => handle_remove(config, name),
        EncryptCommands::Authorize {
            recipient,
            repo,
            dry_run,
            ..
        } => handle_authorize(
            config,
            db,
            recipient.as_deref(),
            repo.as_deref(),
            *dry_run,
            debug,
        ),
        EncryptCommands::Deauthorize {
            recipient,
            repo,
            dry_run,
            ..
        } => handle_deauthorize(config, db, recipient, repo.as_deref(), *dry_run, debug),
        EncryptCommands::Rotate {
            recipients,
            repo,
            dry_run,
            ..
        } => handle_rotate(config, db, recipients, repo.as_deref(), *dry_run, debug),
        EncryptCommands::Status { repo, .. } => handle_status(config, repo.as_deref()),
        EncryptCommands::Show => handle_identity(),
    }
}

#[cfg(test)]
mod tests {
    use super::util::find_age_files;
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_find_age_files_recursively() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        let age_file1 = path.join("test.txt.age");
        let plain_file = path.join("test.txt");
        let sub = path.join("sub");
        std::fs::create_dir(&sub).unwrap();
        let age_file2 = sub.join("another.toml.age");

        std::fs::write(&age_file1, "").unwrap();
        std::fs::write(&plain_file, "").unwrap();
        std::fs::write(&age_file2, "").unwrap();

        let mut discovered = Vec::new();
        find_age_files(path, &mut discovered).unwrap();

        assert_eq!(discovered.len(), 2);
        assert!(discovered.contains(&age_file1));
        assert!(discovered.contains(&age_file2));
    }

    #[test]
    fn test_authorize_and_rotate_operation() {
        let temp = tempdir().unwrap();
        let repo_dir = temp.path().join("my-repo");
        std::fs::create_dir_all(repo_dir.join("dots")).unwrap();

        // 1. Generate encryption keys
        let id1 = age::x25519::Identity::generate();
        let pub1 = id1.to_public().to_string();

        let id2 = age::x25519::Identity::generate();
        let pub2 = id2.to_public().to_string();

        // 2. Initialize repo metadata and git
        crate::common::git::init_repo(&repo_dir).unwrap();
        std::fs::write(repo_dir.join("instantdots.toml"), "").unwrap();

        let meta = crate::dot::types::RepoMetaData {
            name: "my-repo".to_string(),
            dots_dirs: vec!["dots".to_string()],
            encryption_recipients: vec![pub1.clone()],
            ..Default::default()
        };
        crate::dot::meta::update_meta(&repo_dir, &meta).unwrap();

        // 3. Encrypt an initial file for id1
        let plain_bytes = b"super secret password";
        let parsed_recipients = crate::dot::encryption::parse_recipients(&[pub1.clone()]).unwrap();
        let cipher_bytes =
            crate::dot::encryption::encrypt_bytes_to_armored(plain_bytes, &parsed_recipients)
                .unwrap();
        let encrypted_file_path = repo_dir.join("dots/secrets.txt.age");
        std::fs::write(&encrypted_file_path, &cipher_bytes).unwrap();

        // 4. Setup mock DotfileConfig and DB
        let config_file = temp.path().join("dots.toml");
        std::fs::write(
            &config_file,
            format!(
                r#"
            clone_depth = 1
            [[repos]]
            url = "{}"
            name = "my-repo"
            enabled = true
            "#,
                repo_dir.to_string_lossy()
            ),
        )
        .unwrap();

        let mut config = DotfileConfig::load(Some(config_file.to_str().unwrap())).unwrap();
        config.repos_dir = crate::common::TildePath::new(temp.path().to_path_buf());
        let db_file = temp.path().join("instant.db");
        let db = Database::new(db_file).unwrap();

        // 5. Mock discover identities by setting env var
        let identity_file = temp.path().join("my_identity");
        use age::secrecy::ExposeSecret;
        let id1_string = id1.to_string();
        std::fs::write(&identity_file, id1_string.expose_secret()).unwrap();
        let age_guard = crate::dot::test_util::EnvGuard::set("AGE_IDENTITY", &identity_file);

        // 6. Test Authorize Operation
        handle_authorize(&config, &db, Some(&pub2), Some("my-repo"), false, false).unwrap();

        // Check metadata updated
        let updated_meta = crate::dot::meta::read_meta(&repo_dir).unwrap();
        assert!(updated_meta.encryption_recipients.contains(&pub1));
        assert!(updated_meta.encryption_recipients.contains(&pub2));

        // 7. Verify we can decrypt with the new key (id2)
        let newly_encrypted_bytes = std::fs::read(&encrypted_file_path).unwrap();
        let decryptor = age::Decryptor::new_buffered(age::armor::ArmoredReader::new(
            newly_encrypted_bytes.as_slice(),
        ))
        .unwrap();
        let mut reader = decryptor
            .decrypt(
                vec![Box::new(id2.clone()) as Box<dyn age::Identity>]
                    .iter()
                    .map(|i| i.as_ref() as &dyn age::Identity),
            )
            .unwrap();
        let mut decrypted_payload = Vec::new();
        std::io::Read::read_to_end(&mut reader, &mut decrypted_payload).unwrap();
        assert_eq!(decrypted_payload, plain_bytes);

        // 8. Setup id2 as the local key to test rotating out id1.
        let identity_file2 = temp.path().join("my_identity2");
        let id2_string = id2.to_string();
        std::fs::write(&identity_file2, id2_string.expose_secret()).unwrap();
        drop(age_guard);
        let _age_guard2 = crate::dot::test_util::EnvGuard::set("AGE_IDENTITY", &identity_file2);

        // 9. Test Rotate Operation (only allow id2)
        handle_rotate(&config, &db, &[pub2.clone()], Some("my-repo"), false, false).unwrap();

        let rotated_meta = crate::dot::meta::read_meta(&repo_dir).unwrap();
        assert_eq!(rotated_meta.encryption_recipients, vec![pub2.clone()]);
    }
}
