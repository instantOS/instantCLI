use super::browser::{build_browser_menu_items, build_quick_access_items};
use super::operations::{otp_command_args, resolve_entry_by_name};
use super::types::PassEntry;
use super::types::{BrowserItemKind, BrowserMenuItem};
use super::utils::{first_secret_line, load_entries, normalize_otp_name, sanitize_entry_name};
use crate::menu::protocol::FzfPreview;
use crate::menu_utils::FzfSelectable;
use std::fs;

#[test]
fn normalizes_otp_names() {
    assert_eq!(normalize_otp_name("email/github"), "email/github.otp");
    assert_eq!(normalize_otp_name("email/github.otp"), "email/github.otp");
}

#[test]
fn first_secret_line_uses_first_nonempty_line_only() {
    let output = b"topsecret\nusername: demo\n";
    assert_eq!(first_secret_line(output).as_deref(), Some("topsecret"));
}

#[test]
fn otp_entry_reports_otp_primary_action() {
    let entry = PassEntry {
        display_name: "mail/work.otp".to_string(),
        secret_key: None,
        otp_key: Some("mail/work.otp".to_string()),
        secret_path: None,
        otp_path: None,
    };

    assert_eq!(entry.kind_label(), "otp");
    assert_eq!(entry.primary_action_label(), "Copy OTP code");
}

#[test]
fn sanitizes_bad_entry_names() {
    assert!(sanitize_entry_name("").is_err());
    assert!(sanitize_entry_name("../foo").is_err());
    assert!(sanitize_entry_name("foo\nbar").is_err());
    assert_eq!(sanitize_entry_name("/work/github/").unwrap(), "work/github");
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
        PassEntry {
            display_name: "mail/work".to_string(),
            secret_key: Some("mail/work".to_string()),
            otp_key: None,
            secret_path: None,
            otp_path: None,
        },
        PassEntry {
            display_name: "servers/prod/root".to_string(),
            secret_key: Some("servers/prod/root".to_string()),
            otp_key: None,
            secret_path: None,
            otp_path: None,
        },
    ];

    let items = build_quick_access_items(&entries);

    assert_eq!(items.len(), 3);
    assert_eq!(items[0].fzf_display_text(), "mail/work");
    assert_eq!(items[1].fzf_display_text(), "servers/prod/root");
    assert!(matches!(items[2].kind, BrowserItemKind::Menu));
}

#[test]
fn tree_browser_root_shows_folder_nodes() {
    let entries = vec![PassEntry {
        display_name: "mail/work".to_string(),
        secret_key: Some("mail/work".to_string()),
        otp_key: None,
        secret_path: None,
        otp_path: None,
    }];

    let items = build_browser_menu_items(&entries, &[], true).unwrap();

    assert!(
        items
            .iter()
            .any(|item| matches!(item.kind, BrowserItemKind::Folder(ref path) if path == "mail"))
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
        PassEntry {
            display_name: "paypal".to_string(),
            secret_key: Some("paypal".to_string()),
            otp_key: None,
            secret_path: None,
            otp_path: None,
        },
        PassEntry {
            display_name: "paypal.otp".to_string(),
            secret_key: None,
            otp_key: Some("paypal.otp".to_string()),
            secret_path: None,
            otp_path: None,
        },
    ];

    let resolved = resolve_entry_by_name(&entries, "paypal.otp", true).unwrap();

    assert_eq!(resolved.display_name, "paypal.otp");
    assert_eq!(resolved.kind_label(), "otp");
}

#[test]
fn otp_command_uses_plain_pass_otp_invocation() {
    assert_eq!(otp_command_args("paypal.otp"), ["otp", "paypal.otp"]);
}
