use super::*;
use crate::dot::db::{Database, DotFileType};
use crate::dot::dotfile::Dotfile;
use crate::dot::units::UnitIndex;
use age::secrecy::ExposeSecret;
use serial_test::serial;
use std::fs;
use tempfile::tempdir;

#[test]
fn status_reports_known_unmodified_content_as_outdated_when_source_differs() {
    let dir = tempdir().unwrap();
    let source_path = dir.path().join("repo/dots/config.toml");
    let target_path = dir.path().join("home/config.toml");
    fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    fs::create_dir_all(target_path.parent().unwrap()).unwrap();
    fs::write(&source_path, "new source").unwrap();
    fs::write(&target_path, "previous source").unwrap();

    let source_file = fs::File::open(&source_path).unwrap();
    source_file
        .set_times(
            fs::FileTimes::new()
                .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(100)),
        )
        .unwrap();
    let target_file = fs::File::open(&target_path).unwrap();
    target_file
        .set_times(
            fs::FileTimes::new()
                .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(200)),
        )
        .unwrap();

    let db = Database::new(dir.path().join("test.db")).unwrap();
    let previous_hash = Dotfile::compute_hash(&target_path).unwrap();
    db.add_hash(
        &previous_hash,
        &dir.path().join("repo/dots/previous-config.toml"),
        DotFileType::SourceFile,
    )
    .unwrap();

    let dotfile = Dotfile::new(source_path, target_path, false);
    assert_eq!(
        get_dotfile_status(&dotfile, &db, &UnitIndex::default()),
        DotFileStatus::Outdated
    );
}

#[test]
#[serial]
fn summary_separates_modified_unit_from_unchanged_members() {
    let dir = tempdir().unwrap();
    let home = dir.path().join("home");
    let config_home = dir.path().join("config");
    let source_dir = dir.path().join("repo/dots/.config/editor");
    let target_dir = home.join(".config/editor");
    fs::create_dir_all(&source_dir).unwrap();
    fs::create_dir_all(&target_dir).unwrap();
    fs::create_dir_all(&config_home).unwrap();

    let prev_home = std::env::var_os("HOME");
    let prev_xdg = std::env::var_os("XDG_CONFIG_HOME");
    // SAFETY: this test is serialised and restores the process env below.
    unsafe {
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_CONFIG_HOME", &config_home);
    }

    let modified_source = source_dir.join("modified.lua");
    let modified_target = target_dir.join("modified.lua");
    fs::write(&modified_source, "repository version").unwrap();
    fs::write(&modified_target, "local version").unwrap();

    let clean_source = source_dir.join("clean.lua");
    let clean_target = target_dir.join("clean.lua");
    fs::write(&clean_source, "same version").unwrap();
    fs::write(&clean_target, "same version").unwrap();

    let db = Database::new(dir.path().join("test.db")).unwrap();
    let modified_dotfile = Dotfile::new(modified_source, modified_target.clone(), false);
    let clean_dotfile = Dotfile::new(clean_source, clean_target.clone(), false);
    let all_dotfiles = HashMap::from([
        (modified_target.clone(), modified_dotfile),
        (clean_target.clone(), clean_dotfile),
    ]);
    let unit_index =
        crate::dot::units::build_unit_index(&all_dotfiles, &[PathBuf::from(".config/editor")], &db)
            .unwrap();
    let report = collect_status(
        &all_dotfiles,
        &DotfileConfig::default(),
        &db,
        &UnitIndex::default(),
    );

    unsafe {
        match prev_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        match prev_xdg {
            Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }

    assert_eq!(report.summary.modified_count, 1);
    assert_eq!(report.summary.clean_count, 1);
    assert!(unit_index.is_target_in_modified_unit(&clean_target));
    let modified_units = unit_index.modified_unit_statuses();
    assert_eq!(modified_units.len(), 1);
    assert_eq!(modified_units[0].modified_files, vec![modified_target]);
}

#[test]
#[serial]
fn encrypted_status_reports_identity_required_on_decrypt_failure() {
    let dir = tempdir().unwrap();
    let home = dir.path().join("home");
    let config_home = dir.path().join("config");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&config_home).unwrap();

    let prev_home = std::env::var_os("HOME");
    let prev_xdg = std::env::var_os("XDG_CONFIG_HOME");
    let prev_age = std::env::var_os("AGE_IDENTITY");
    // SAFETY: this test is serialised and restores the process env below.
    unsafe {
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_CONFIG_HOME", &config_home);
        std::env::remove_var("AGE_IDENTITY");
    }

    let source_path = dir.path().join("repo/dots/secret.txt.age");
    let target_path = home.join("secret.txt");
    fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    fs::write(&source_path, "not an age file").unwrap();
    fs::write(&target_path, "plaintext").unwrap();

    let db = Database::new(dir.path().join("test.db")).unwrap();
    let dotfile = Dotfile::new(source_path, target_path, false);
    let status = get_dotfile_status(&dotfile, &db, &UnitIndex::default());

    unsafe {
        match prev_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        match prev_xdg {
            Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
        match prev_age {
            Some(v) => std::env::set_var("AGE_IDENTITY", v),
            None => std::env::remove_var("AGE_IDENTITY"),
        }
    }

    assert_eq!(status, DotFileStatus::IdentityRequired);
}

#[test]
#[serial]
fn encrypted_status_reports_encrypted_error_for_invalid_ciphertext_with_identity() {
    let dir = tempdir().unwrap();
    let home = dir.path().join("home");
    let config_home = dir.path().join("config");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&config_home).unwrap();

    let identity = age::x25519::Identity::generate();
    let identity_file = dir.path().join("identity.key");
    fs::write(&identity_file, identity.to_string().expose_secret()).unwrap();

    let prev_home = std::env::var_os("HOME");
    let prev_xdg = std::env::var_os("XDG_CONFIG_HOME");
    let prev_age = std::env::var_os("AGE_IDENTITY");
    // SAFETY: this test is serialised and restores the process env below.
    unsafe {
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_CONFIG_HOME", &config_home);
        std::env::set_var("AGE_IDENTITY", &identity_file);
    }

    let source_path = dir.path().join("repo/dots/secret.txt.age");
    let target_path = home.join("secret.txt");
    fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    fs::write(&source_path, "not an age file").unwrap();
    fs::write(&target_path, "plaintext").unwrap();

    let db = Database::new(dir.path().join("test.db")).unwrap();
    let dotfile = Dotfile::new(source_path, target_path, false);
    let status = get_dotfile_status(&dotfile, &db, &UnitIndex::default());

    unsafe {
        match prev_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        match prev_xdg {
            Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
        match prev_age {
            Some(v) => std::env::set_var("AGE_IDENTITY", v),
            None => std::env::remove_var("AGE_IDENTITY"),
        }
    }

    assert_eq!(status, DotFileStatus::EncryptedError);
}

#[test]
fn json_summary_formats_every_status_and_optional_source_metadata() -> Result<()> {
    use super::report::{FileInfo, StatusReport, StatusSummary};
    let config: DotfileConfig = toml::from_str("[[repos]]\nname = 'example'\nurl = '/repo'")?;
    let home = Path::new("/home/test");
    let context = RenderContext {
        home,
        config: &config,
    };
    let cases = [
        (DotFileStatus::Modified, "modified", None),
        (DotFileStatus::Outdated, "outdated", None),
        (
            DotFileStatus::IdentityRequired,
            "identity_required",
            Some("encrypted_source"),
        ),
        (
            DotFileStatus::EncryptedError,
            "encrypted_error",
            Some("encrypted_source_error"),
        ),
        (DotFileStatus::Clean, "clean", None),
    ];
    let report = StatusReport {
        files_by_status: cases
            .iter()
            .map(|(status, name, _)| {
                let target_path = home.join(name);
                (
                    *status,
                    vec![FileInfo {
                        dotfile: Dotfile::new(
                            PathBuf::from("/repo/dots/file"),
                            target_path.clone(),
                            false,
                        ),
                        target_path,
                        repo_name: crate::dot::RepoName::new("example".into()),
                        dotfile_dir: "dots".into(),
                        is_overridden: true,
                    }],
                )
            })
            .collect(),
        summary: StatusSummary {
            total_files: 5,
            clean_count: 1,
            modified_count: 1,
            outdated_count: 1,
            identity_required_count: 1,
            encrypted_error_count: 1,
        },
    };
    let units = UnitIndex::default();
    let full = json::document(&report, true, true, &context, &units);
    assert_eq!(full["total_files"], 5);
    for (_, name, reason) in cases {
        let file = &full[format!("{name}_files")][0];
        assert_eq!(file["path"], format!("~/{name}"));
        assert_eq!(file["status"], name);
        assert_eq!(file["repo"], "example");
        assert_eq!(file["dotfile_dir"], "dots");
        assert_eq!(file["priority"], 1);
        assert_eq!(file["override"], true);
        assert_eq!(file.get("reason").and_then(|value| value.as_str()), reason);
    }
    let brief = json::document(&report, false, false, &context, &units);
    assert_eq!(brief["clean_count"], 1);
    assert_eq!(brief["clean_files"], serde_json::json!([]));
    for (_, name, _) in &cases[..4] {
        let file = &brief[format!("{name}_files")][0];
        assert!(file.get("priority").is_none());
        assert!(file.get("override").is_none());
    }
    Ok(())
}

#[test]
fn status_paths_abbreviate_only_paths_inside_home() {
    let config = DotfileConfig::default();
    let context = RenderContext {
        home: Path::new("/home/test"),
        config: &config,
    };
    assert_eq!(
        context.path(Path::new("/home/test/.config/editor")),
        "~/.config/editor"
    );
    assert_eq!(context.path(Path::new("/etc/editor")), "/etc/editor");
    assert_eq!(
        context.path(Path::new("/home/test-other/file")),
        "/home/test-other/file"
    );
}
