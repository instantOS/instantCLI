use super::types::PassEntry;
use super::types::{BrowserItemKind, BrowserMenuItem};
use super::utils::{first_secret_line, normalize_otp_name, sanitize_entry_name};
use crate::menu::protocol::FzfPreview;
use crate::menu_utils::FzfSelectable;

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
fn groups_password_and_otp_paths_under_one_display_name() {
    let mut entry = PassEntry {
        display_name: "mail/work".to_string(),
        secret_key: Some("mail/work".to_string()),
        otp_key: Some("mail/work.otp".to_string()),
        secret_path: None,
        otp_path: None,
    };

    assert!(entry.has_secret());
    assert!(entry.has_otp());
    assert_eq!(entry.kind_label(), "password + otp");
    assert_eq!(entry.primary_action_label(), "Copy password");

    entry.secret_key = None;
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
