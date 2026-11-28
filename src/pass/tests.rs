use super::browser::{build_browser_menu_items, build_quick_access_items};
use super::content::{find_otp_uri_line, first_secret_line, write_export_file};
use super::names::{
    entry_name_prefill, normalize_file_entry_name, normalize_otp_name, sanitize_entry_name,
    suggested_export_filename, suggested_file_entry_name, suggested_otp_entry_name,
};
use super::operations::{
    otp_command_args, pass_edit_editor, pass_edit_was_unchanged, read_pass_edit_stderr,
    rename_entry, rename_target, resolve_entry_by_name,
};
use super::pass_cli::run_status_with_input;
use super::prompts::prompt_otp_uri;
use super::store::load_entries;
use super::types::{BrowserItemKind, BrowserMenuItem, EntryKind, PassEntry, location_color};
use super::{PassCommands, auto_setup_applies};
use crate::menu::protocol::FzfPreview;
use crate::menu_utils::FzfSelectable;
use crate::ui::nerd_font::NerdFont;
use crate::ui::text::strip_ansi;
use std::fs;

fn pass_entry(name: &str, kind: EntryKind) -> PassEntry {
    PassEntry::test_entry(name, kind)
}

#[test]
fn normalizes_otp_names() {
    assert_eq!(normalize_otp_name("email/github"), "email/github.otp");
    assert_eq!(normalize_otp_name("email/github.otp"), "email/github.otp");
}

#[test]
fn new_entry_names_start_inside_the_current_folder() {
    assert_eq!(
        entry_name_prefill(Some("example"), None).as_deref(),
        Some("example/")
    );
    assert_eq!(
        entry_name_prefill(Some("example"), Some("github")).as_deref(),
        Some("example/github")
    );
    assert_eq!(
        entry_name_prefill(None, Some("github")).as_deref(),
        Some("github")
    );
    assert!(entry_name_prefill(Some(""), None).is_none());
}

#[test]
fn suggests_otp_entry_names_from_the_issuer() {
    let secret = "JBSWY3DPEHPK3PXP";
    assert_eq!(
        suggested_otp_entry_name(&format!("otpauth://totp/GitHub:alice?secret={secret}"))
            .as_deref(),
        Some("GitHub")
    );
    assert_eq!(
        suggested_otp_entry_name(&format!(
            "otpauth://totp/alice?secret={secret}&issuer=instantos%20tester"
        ))
        .as_deref(),
        Some("instantos-tester")
    );
    assert!(suggested_otp_entry_name("https://example.com").is_none());
}

#[cfg(unix)]
#[test]
fn pass_input_stays_out_of_arguments_and_errors() {
    use std::env;
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;

    struct PathGuard(Option<OsString>);

    impl Drop for PathGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.0 {
                    Some(path) => env::set_var("PATH", path),
                    None => env::remove_var("PATH"),
                }
            }
        }
    }

    let temp = tempfile::tempdir().unwrap();
    let pass = temp.path().join("pass");
    let args_path = temp.path().join("args");
    let stdin_path = temp.path().join("stdin");
    let script = format!(
        "#!/bin/sh\nif [ \"$1\" = reject ]; then cat >&2; exit 1; fi\nprintf '%s\\n' \"$@\" > '{}'\ncat > '{}'\n",
        args_path.display(),
        stdin_path.display()
    );
    fs::write(&pass, script).unwrap();
    let mut permissions = fs::metadata(&pass).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&pass, permissions).unwrap();

    let old_path = env::var_os("PATH");
    let mut paths = vec![temp.path().to_path_buf()];
    let current_path = old_path.clone().unwrap_or_default();
    paths.extend(env::split_paths(&current_path));
    let new_path = env::join_paths(paths).unwrap();
    unsafe { env::set_var("PATH", &new_path) };
    let _guard = PathGuard(old_path);

    let uri = "otpauth://totp/demo?secret=JBSWY3DPEHPK3PXP";
    run_status_with_input(["otp", "insert", "-f", "demo.otp"], uri).unwrap();

    assert_eq!(
        fs::read_to_string(&args_path).unwrap(),
        "otp\ninsert\n-f\ndemo.otp\n"
    );
    assert_eq!(fs::read_to_string(&stdin_path).unwrap(), format!("{uri}\n"));

    let error = run_status_with_input(["reject"], uri).unwrap_err();
    assert!(!error.to_string().contains(uri));
}

#[test]
fn manual_otp_prompt_uses_masked_input() -> anyhow::Result<()> {
    use crate::menu_utils::MockQueue;

    let uri = "otpauth://totp/test?secret=JBSWY3DPEHPK3PXP";
    let _mock = MockQueue::new()
        .password(format!("  {uri}  "))
        .password_cancelled()
        .guard();
    assert_eq!(prompt_otp_uri("New OTP URI")?.as_deref(), Some(uri));
    assert_eq!(prompt_otp_uri("Edit OTP URI")?, None);
    Ok(())
}

#[test]
fn manual_otp_prompt_rejects_invalid_secret() {
    use crate::menu_utils::MockQueue;

    let _mock = MockQueue::new()
        .password("otpauth://totp/instantos%20test?secret=263636&issuer=instantos")
        .guard();
    let error = prompt_otp_uri("New OTP URI").unwrap_err();
    assert!(error.to_string().contains("not a valid base32 length"));
}

#[test]
fn plaintext_exports_are_private_on_create_and_overwrite() -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    use crate::menu_utils::MockQueue;

    let directory = tempfile::tempdir()?;
    let destination = directory.path().join("exports/password");
    write_export_file(&destination, b"first")?;
    assert_eq!(
        fs::metadata(&destination)?.permissions().mode() & 0o777,
        0o600
    );
    fs::set_permissions(&destination, fs::Permissions::from_mode(0o644))?;

    let _mock = MockQueue::new().confirm_yes().guard();
    write_export_file(&destination, b"second")?;
    assert_eq!(
        fs::metadata(&destination)?.permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(fs::read(&destination)?, b"second");
    Ok(())
}

#[test]
fn plaintext_export_rejects_a_symlink_destination() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let target = directory.path().join("existing");
    let destination = directory.path().join("export");
    fs::write(&target, b"original")?;
    std::os::unix::fs::symlink(&target, &destination)?;

    let _mock = crate::menu_utils::MockQueue::new().confirm_yes().guard();
    assert!(write_export_file(&destination, b"replacement").is_err());
    assert_eq!(fs::read(&target)?, b"original");
    Ok(())
}

#[test]
fn first_secret_line_uses_first_nonempty_line_only() {
    let output = b"topsecret\nusername: demo\n";
    assert_eq!(first_secret_line(output).as_deref(), Some("topsecret"));
}

#[test]
fn finds_otp_uri_on_any_entry_line() {
    let output = b"topsecret\nusername: demo\n  otpauth://totp/test?secret=JBSWY3DPEHPK3PXP  \n";
    assert_eq!(
        find_otp_uri_line(output),
        Some("otpauth://totp/test?secret=JBSWY3DPEHPK3PXP")
    );
}

#[test]
fn otp_entry_reports_otp_primary_action() {
    let entry = pass_entry("mail/work.otp", EntryKind::Otp);

    assert_eq!(entry.kind_label(), "otp");
    assert_eq!(entry.primary_action_label(), "Copy OTP code");
}

#[test]
fn file_entry_reports_export_primary_action() {
    let entry = pass_entry("docs/contract.file", EntryKind::File);

    assert!(entry.is_file_entry());
    assert_eq!(entry.primary_action_label(), "Export file");
}

#[test]
fn sanitizes_bad_entry_names() {
    assert!(sanitize_entry_name("").is_err());
    assert!(sanitize_entry_name("../foo").is_err());
    assert!(sanitize_entry_name("foo\nbar").is_err());
    assert_eq!(sanitize_entry_name("/work/github/").unwrap(), "work/github");
}

#[test]
fn file_entry_names_get_the_file_suffix() {
    assert_eq!(normalize_file_entry_name("contract"), "contract.file");
    assert_eq!(normalize_file_entry_name("contract.file"), "contract.file");
}

#[test]
fn rename_preserves_the_entry_kind_suffix() {
    let file = pass_entry("docs/contract.file", EntryKind::File);
    assert_eq!(
        rename_target(&file, "archive/contract").unwrap(),
        "archive/contract.file"
    );
    assert_eq!(
        rename_target(&file, "archive/contract.file").unwrap(),
        "archive/contract.file"
    );

    let otp = pass_entry("mail/work.otp", EntryKind::Otp);
    assert_eq!(
        rename_target(&otp, "mail/personal").unwrap(),
        "mail/personal.otp"
    );

    let password = pass_entry("mail/work", EntryKind::Password);
    assert_eq!(
        rename_target(&password, "mail/personal").unwrap(),
        "mail/personal"
    );
}

#[cfg(unix)]
#[test]
fn rename_refuses_to_overwrite_an_existing_entry() {
    use std::env;
    use std::ffi::OsString;

    use crate::menu_utils::MockQueue;

    struct EnvGuard(Option<OsString>);

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.0 {
                    Some(value) => env::set_var("PASSWORD_STORE_DIR", value),
                    None => env::remove_var("PASSWORD_STORE_DIR"),
                }
            }
        }
    }

    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source.gpg");
    let destination = temp.path().join("destination.gpg");
    fs::write(&source, b"source").unwrap();
    fs::write(&destination, b"existing").unwrap();

    let old_store = env::var_os("PASSWORD_STORE_DIR");
    unsafe { env::set_var("PASSWORD_STORE_DIR", temp.path()) };
    let _guard = EnvGuard(old_store);
    let _mock = MockQueue::new().confirm_no().guard();

    let entry = PassEntry::new("source", &source, EntryKind::Password);
    assert!(rename_entry(&entry, "destination").is_err());
    assert_eq!(fs::read(&source).unwrap(), b"source");
    assert_eq!(fs::read(&destination).unwrap(), b"existing");
}

#[test]
fn file_entry_suggestion_uses_the_source_name() {
    assert_eq!(
        suggested_file_entry_name(std::path::Path::new("/tmp/contract.pdf")),
        "contract.pdf.file"
    );
}

#[test]
fn file_export_suggestion_restores_the_source_name() {
    assert_eq!(
        suggested_export_filename("docs/contract.pdf.file"),
        "contract.pdf"
    );
    assert_eq!(
        suggested_export_filename("docs/contract.file"),
        "contract.file"
    );
}

#[test]
fn browser_items_use_plain_selection_keys() {
    let item = BrowserMenuItem {
        key: "folder:mail".to_string(),
        display: "\u{1b}[35m\u{1b}[0m mail".to_string(),
        preview: FzfPreview::None,
        kind: BrowserItemKind::Folder("mail".to_string()),
    };

    assert_eq!(item.fzf_key(), "folder:mail");
}

#[test]
fn quick_access_items_are_flat_and_include_tree_menu_entry() {
    let entries = vec![
        pass_entry("mail/work", EntryKind::Password),
        pass_entry("servers/prod/root", EntryKind::Password),
    ];

    let items = build_quick_access_items(&entries);

    assert_eq!(items.len(), 3);
    assert!(strip_ansi(&items[0].display).ends_with("mail/work"));
    assert!(strip_ansi(&items[1].display).ends_with("servers/prod/root"));
    assert!(matches!(items[2].kind, BrowserItemKind::Menu));
}

#[test]
fn quick_access_entries_show_kind_icons() {
    let entries = vec![
        pass_entry("mail/work", EntryKind::Password),
        pass_entry("docs/contract.file", EntryKind::File),
        pass_entry("paypal.otp", EntryKind::Otp),
    ];

    let items = build_quick_access_items(&entries);

    assert!(items[0].display.contains(char::from(NerdFont::Key)));
    assert!(items[1].display.contains(char::from(NerdFont::File)));
    assert!(items[2].display.contains(char::from(NerdFont::Clock)));
}

#[test]
fn quick_access_colors_entry_locations() {
    let entries = vec![pass_entry("mail/work/github", EntryKind::Password)];

    let items = build_quick_access_items(&entries);
    let mail = crate::ui::catppuccin::format_with_color("mail/", location_color("mail"));
    let work = crate::ui::catppuccin::format_with_color("work/", location_color("mail/work"));

    assert!(items[0].display.contains(&mail));
    assert!(items[0].display.contains(&work));
}

#[test]
fn tree_browser_root_shows_folder_nodes() {
    let entries = vec![pass_entry("mail/work", EntryKind::Password)];

    let items = build_browser_menu_items(&entries, &[]).unwrap();

    assert!(
        items
            .iter()
            .any(|item| matches!(item.kind, BrowserItemKind::Folder(ref path) if path == "mail"))
    );
}

#[test]
fn tree_browser_colors_folder_locations() {
    let entries = vec![pass_entry("mail/work", EntryKind::Password)];

    let items = build_browser_menu_items(&entries, &[]).unwrap();
    let folder_item = items
        .iter()
        .find(|item| matches!(item.kind, BrowserItemKind::Folder(ref path) if path == "mail"))
        .unwrap();
    let color = crate::ui::catppuccin::hex_to_ansi_fg(location_color("mail"));

    assert!(folder_item.display.contains(&color));
}

#[test]
fn tree_browser_uses_distinct_icon_for_file_entries() {
    let entries = vec![pass_entry("docs/contract.file", EntryKind::File)];

    let items = build_browser_menu_items(&entries, &["docs".to_string()]).unwrap();
    let file_item = items
        .iter()
        .find(|item| matches!(item.kind, BrowserItemKind::Entry { ref key, .. } if key == "docs/contract.file"))
        .unwrap();

    assert!(
        file_item
            .display
            .contains(char::from(crate::ui::nerd_font::NerdFont::File))
    );
}

#[test]
fn load_entries_keeps_standalone_otp_suffix_visible() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("paypal.otp.gpg"), b"otp").unwrap();

    let entries = load_entries(temp.path()).unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].display_name, "paypal.otp");
    assert!(!entries[0].has_secret());
    assert!(entries[0].has_otp());
    assert_eq!(entries[0].kind_label(), "otp");
}

#[test]
fn load_entries_classifies_file_entries() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("docs")).unwrap();
    fs::write(temp.path().join("docs/contract.file.gpg"), b"file").unwrap();

    let entries = load_entries(temp.path()).unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].display_name, "docs/contract.file");
    assert_eq!(entries[0].kind, EntryKind::File);
    assert_eq!(entries[0].kind_label(), "file");
}

#[test]
fn load_entries_keeps_password_and_otp_as_separate_entries() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("paypal.gpg"), b"secret").unwrap();
    fs::write(temp.path().join("paypal.otp.gpg"), b"otp").unwrap();

    let entries = load_entries(temp.path()).unwrap();

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].display_name, "paypal");
    assert_eq!(entries[0].kind_label(), "password");
    assert_eq!(entries[1].display_name, "paypal.otp");
    assert_eq!(entries[1].kind_label(), "otp");
}

#[test]
fn resolve_entry_by_name_prefers_exact_otp_match() {
    let entries = vec![
        pass_entry("paypal", EntryKind::Password),
        pass_entry("paypal.otp", EntryKind::Otp),
    ];

    let resolved = resolve_entry_by_name(&entries, "paypal.otp", true).unwrap();

    assert_eq!(resolved.display_name, "paypal.otp");
    assert_eq!(resolved.kind_label(), "otp");
}

#[test]
fn otp_command_uses_plain_pass_otp_invocation() {
    assert_eq!(otp_command_args("paypal.otp"), ["otp", "paypal.otp"]);
}

#[test]
fn password_edit_uses_clean_nvim_without_swap() {
    assert_eq!(pass_edit_editor(), "nvim --clean -n");
}

#[test]
fn unchanged_pass_edit_exit_is_treated_as_noop() {
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    let status = ExitStatus::from_raw(1 << 8);
    assert!(pass_edit_was_unchanged(status, "Password unchanged.\n"));
    assert!(!pass_edit_was_unchanged(status, ""));
    assert!(!pass_edit_was_unchanged(status, "gpg: decryption failed\n"));
    assert!(!pass_edit_was_unchanged(
        status,
        "nvim: command not found\nPassword unchanged.\n"
    ));
    assert!(!pass_edit_was_unchanged(
        ExitStatus::from_raw(2 << 8),
        "Password unchanged.\n"
    ));
}

#[test]
fn pass_edit_shows_retry_prompt_and_suppresses_unchanged_message() -> anyhow::Result<()> {
    let mut terminal = Vec::new();
    let unchanged = b"Password unchanged.\n";
    assert_eq!(
        read_pass_edit_stderr(&unchanged[..], &mut terminal)?,
        unchanged
    );
    assert!(terminal.is_empty());

    let prompt = b"GPG encryption failed. Would you like to try again? [y/N] ";
    assert_eq!(read_pass_edit_stderr(&prompt[..], &mut terminal)?, prompt);
    assert_eq!(terminal, prompt);
    Ok(())
}

#[test]
fn automatic_setup_runs_for_pickers_and_interactive_writes() {
    assert!(auto_setup_applies(None, false, false));
    assert!(!auto_setup_applies(None, true, true));
    assert!(auto_setup_applies(
        Some(&PassCommands::Menu { gui: false }),
        false,
        true,
    ));
    assert!(!auto_setup_applies(
        Some(&PassCommands::Menu { gui: true }),
        false,
        false,
    ));
    assert!(!auto_setup_applies(Some(&PassCommands::Setup), false, true));
    let generate = PassCommands::Generate {
        name: Some("example".into()),
        length: 20,
    };
    assert!(auto_setup_applies(Some(&generate), false, true));
    assert!(!auto_setup_applies(Some(&generate), false, false));

    let name = Some("example".into());
    for command in [
        PassCommands::Otp { name: name.clone() },
        PassCommands::Delete { name: name.clone() },
        PassCommands::Export {
            name,
            path: Some("output".into()),
        },
    ] {
        assert!(!auto_setup_applies(Some(&command), false, true));
        assert!(!auto_setup_applies(Some(&command), false, false));
    }
}
