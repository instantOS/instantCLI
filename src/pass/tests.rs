use super::browser::{build_browser_menu_items, build_quick_access_items};
use super::operations::{
    otp_command_args, pass_edit_editor, pass_edit_was_unchanged, resolve_entry_by_name,
};
use super::types::PassEntry;
use super::types::{BrowserItemKind, BrowserMenuItem, location_color};
use super::utils::{
    first_secret_line, load_entries, normalize_file_entry_name, normalize_otp_name,
    sanitize_entry_name, suggested_export_filename, suggested_file_entry_name,
};
use super::{PassCommands, auto_setup_applies};
use crate::menu::protocol::FzfPreview;
use crate::menu_utils::FzfSelectable;
use crate::ui::nerd_font::NerdFont;
use crate::ui::text::strip_ansi;
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
fn file_entry_reports_export_primary_action() {
    let entry = PassEntry {
        display_name: "docs/contract.file".to_string(),
        secret_key: Some("docs/contract.file".to_string()),
        otp_key: None,
        secret_path: None,
        otp_path: None,
    };

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
    assert!(strip_ansi(&items[0].display).ends_with("mail/work"));
    assert!(strip_ansi(&items[1].display).ends_with("servers/prod/root"));
    assert!(matches!(items[2].kind, BrowserItemKind::Menu));
}

#[test]
fn quick_access_entries_show_kind_icons() {
    let entries = vec![
        PassEntry {
            display_name: "mail/work".to_string(),
            secret_key: Some("mail/work".to_string()),
            otp_key: None,
            secret_path: None,
            otp_path: None,
        },
        PassEntry {
            display_name: "docs/contract.file".to_string(),
            secret_key: Some("docs/contract.file".to_string()),
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

    let items = build_quick_access_items(&entries);

    assert!(items[0].display.contains(char::from(NerdFont::Key)));
    assert!(items[1].display.contains(char::from(NerdFont::File)));
    assert!(items[2].display.contains(char::from(NerdFont::Clock)));
}

#[test]
fn quick_access_colors_entry_locations() {
    let entries = vec![PassEntry {
        display_name: "mail/work/github".to_string(),
        secret_key: Some("mail/work/github".to_string()),
        otp_key: None,
        secret_path: None,
        otp_path: None,
    }];

    let items = build_quick_access_items(&entries);
    let mail = crate::ui::catppuccin::format_with_color("mail/", location_color("mail"));
    let work = crate::ui::catppuccin::format_with_color("work/", location_color("mail/work"));

    assert!(items[0].display.contains(&mail));
    assert!(items[0].display.contains(&work));
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

    let items = build_browser_menu_items(&entries, &[]).unwrap();

    assert!(
        items
            .iter()
            .any(|item| matches!(item.kind, BrowserItemKind::Folder(ref path) if path == "mail"))
    );
}

#[test]
fn tree_browser_colors_folder_locations() {
    let entries = vec![PassEntry {
        display_name: "mail/work".to_string(),
        secret_key: Some("mail/work".to_string()),
        otp_key: None,
        secret_path: None,
        otp_path: None,
    }];

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
    let entries = vec![PassEntry {
        display_name: "docs/contract.file".to_string(),
        secret_key: Some("docs/contract.file".to_string()),
        otp_key: None,
        secret_path: None,
        otp_path: None,
    }];

    let items = build_browser_menu_items(&entries, &["docs".to_string()]).unwrap();
    let file_item = items
        .iter()
        .find(|item| matches!(item.kind, BrowserItemKind::Entry(ref key) if key == "docs/contract.file"))
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

#[test]
fn password_edit_uses_clean_nvim() {
    assert_eq!(pass_edit_editor(), "nvim --clean");
}

#[test]
fn unchanged_pass_edit_exit_is_treated_as_noop() {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert!(pass_edit_was_unchanged(std::process::ExitStatus::from_raw(
            1 << 8
        )));
        assert!(!pass_edit_was_unchanged(
            std::process::ExitStatus::from_raw(0)
        ));
    }
}

#[test]
fn automatic_setup_runs_for_interactive_commands_only() {
    assert!(auto_setup_applies(&None, false));
    assert!(!auto_setup_applies(&None, true));
    assert!(auto_setup_applies(
        &Some(PassCommands::Menu { gui: false }),
        false
    ));
    assert!(!auto_setup_applies(
        &Some(PassCommands::Menu { gui: true }),
        false
    ));
    assert!(!auto_setup_applies(&Some(PassCommands::Setup), false));
}
