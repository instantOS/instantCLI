//! Test utilities shared across `dot` test modules.
//!
//! Only compiled under `#[cfg(test)]`. Keeps helpers out of the production
//! build while letting any test in the `dot` tree reuse them.

#![cfg(test)]

use crate::common::TildePath;
use crate::dot::config::{DotfileConfig, Repo};
use crate::dot::db::Database;
use crate::dot::types::RepoMetaData;
use age::secrecy::ExposeSecret;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

/// RAII guard that overrides a process-wide environment variable for the
/// lifetime of the guard and restores the previous value on `Drop` — even
/// across panics.
///
/// Tests using this helper MUST be marked `#[serial]` because they mutate
/// process-global state. The guard removes the variable if it was previously
/// unset, or restores its old value otherwise.
///
/// This replaces the manual `match prev { Some(v) => set_var, None => remove_var }`
/// pattern, which leaks state into later tests if anything panics between
/// the `set_var` and the manual restore.
pub struct EnvGuard {
    key: OsString,
    prev: Option<OsString>,
}

impl EnvGuard {
    /// Set `key` to `value` for the lifetime of the returned guard.
    pub fn set<K, V>(key: K, value: V) -> Self
    where
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        let key_os = key.as_ref().to_os_string();
        let prev = std::env::var_os(&key_os);
        // SAFETY: setting environment variables is process-global. Tests using
        // this helper are required to be `#[serial]`.
        unsafe {
            std::env::set_var(&key_os, value.as_ref());
        }
        EnvGuard { key: key_os, prev }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        // SAFETY: see `EnvGuard::set`.
        unsafe {
            match &self.prev {
                Some(v) => std::env::set_var(&self.key, v),
                None => std::env::remove_var(&self.key),
            }
        }
    }
}

/// Shared test environment for encrypt/decrypt integration tests.
///
/// Creates a tempdir with a pre-initialized git repository, an age identity,
/// and the env vars + `DotfileConfig` + `Database` wired up. Individual
/// tests write their scenario-specific files under `dots_dir` / `home` after
/// construction.
pub struct EncryptTestEnv {
    pub _dir: TempDir,
    pub home: PathBuf,
    pub dots_dir: PathBuf,
    pub recipient: String,
    pub _home_guard: EnvGuard,
    pub _age_guard: EnvGuard,
    pub config: DotfileConfig,
    pub db: Database,
}

/// Build the shared test environment. The caller gets a ready-to-use config
/// and database; only scenario-specific file writes remain in each test.
pub fn setup_encrypt_test_env() -> EncryptTestEnv {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let repos_dir = dir.path().join("repos");
    let repo_dir = repos_dir.join("test-repo");
    let dots_dir = repo_dir.join("dots");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&dots_dir).unwrap();
    fs::write(repo_dir.join("instantdots.toml"), "").unwrap();

    std::process::Command::new("git")
        .arg("init")
        .current_dir(&repo_dir)
        .output()
        .unwrap();

    let identity = age::x25519::Identity::generate();
    let recipient = identity.to_public().to_string();
    let identity_file = dir.path().join("identity.key");
    fs::write(&identity_file, identity.to_string().expose_secret()).unwrap();

    let home_guard = EnvGuard::set("HOME", &home);
    let age_guard = EnvGuard::set("AGE_IDENTITY", &identity_file);

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
                encryption_recipients: vec![recipient.clone()],
                ..RepoMetaData::default()
            }),
        }],
        repos_dir: TildePath::new(repos_dir),
        database_dir: TildePath::new(dir.path().join("test.db")),
        ..DotfileConfig::default()
    };
    let db = Database::new(config.database_path().to_path_buf()).unwrap();

    EncryptTestEnv {
        _dir: dir,
        home,
        dots_dir,
        recipient,
        _home_guard: home_guard,
        _age_guard: age_guard,
        config,
        db,
    }
}
