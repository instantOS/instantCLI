use anyhow::{Result, bail};

use crate::menu_utils::{FzfWrapper, HeaderBuilder};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::super::clipboard::clipboard_otp_uri;
use super::super::dependencies::ensure_otp_dependency;
use super::super::operations::{
    generate_password_entry_with_prefix, insert_file_entry_with_prefix,
    insert_otp_entry_from_clipboard_with_prefix, insert_otp_entry_with_prefix,
    insert_password_entry_with_prefix,
};
use super::super::types::{AddMenuAction, AddMenuItem, DEFAULT_PASSWORD_LENGTH};

pub(crate) fn run_add_menu(current_prefix: Option<&str>) -> Result<()> {
    let clipboard_otp = clipboard_otp_uri();
    let items = build_items(clipboard_otp.is_some());

    let header = HeaderBuilder::new(NerdFont::Plus, "Add Password Store Entry")
        .subtitle("Create a new password, generated secret, OTP key, or file")
        .field("Destination", destination(current_prefix))
        .build();

    if let crate::menu_utils::DialogOutcome::Submitted(item) = FzfWrapper::builder()
        .header(header)
        .prompt("Create")
        .responsive_layout()
        .items(items)
        .padded()
        .select_one()?
    {
        match item.action {
            AddMenuAction::AddPassword => {
                insert_password_entry_with_prefix(None, current_prefix)?;
            }
            AddMenuAction::GeneratePassword => {
                generate_password_entry_with_prefix(None, current_prefix, DEFAULT_PASSWORD_LENGTH)?;
            }
            AddMenuAction::AddOtp => {
                ensure_otp_dependency()?;
                insert_otp_entry_with_prefix(None, current_prefix)?;
            }
            AddMenuAction::AddOtpFromClipboard => {
                let Some(otp_uri) = clipboard_otp.as_deref() else {
                    bail!("The clipboard no longer contains a valid OTP URI");
                };
                ensure_otp_dependency()?;
                insert_otp_entry_from_clipboard_with_prefix(current_prefix, otp_uri)?;
            }
            AddMenuAction::AddFile => {
                insert_file_entry_with_prefix(current_prefix)?;
            }
            AddMenuAction::Back => {}
        }
    }

    Ok(())
}

fn destination(current_prefix: Option<&str>) -> String {
    current_prefix
        .map(|prefix| format!("/{prefix}"))
        .unwrap_or_else(|| "/".to_string())
}

fn build_items(clipboard_otp_available: bool) -> Vec<AddMenuItem> {
    let mut items = vec![
        AddMenuItem {
            key: "add-password",
            display: format!(
                "{} Add Password",
                format_icon_colored(NerdFont::Plus, colors::GREEN)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Plus, "Enter Password")
                .text("Create a password entry in the pass store.")
                .blank()
                .bullet("Prompts for a name")
                .bullet("Prompts for password + confirmation")
                .build(),
            action: AddMenuAction::AddPassword,
        },
        AddMenuItem {
            key: "generate-password",
            display: format!(
                "{} Generate Password",
                format_icon_colored(NerdFont::Refresh, colors::TEAL)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Refresh, "Generate Password")
                .text("Generate a 20-character password into a pass entry.")
                .blank()
                .bullet("Prompts for a name")
                .bullet("Uses `pass generate -f`")
                .build(),
            action: AddMenuAction::GeneratePassword,
        },
        AddMenuItem {
            key: "add-otp",
            display: format!(
                "{} Add OTP",
                format_icon_colored(NerdFont::Clock, colors::SAPPHIRE)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Clock, "Add OTP")
                .text("Create a standalone OTP entry using pass-otp.")
                .blank()
                .bullet("Prompts for a name")
                .bullet("Accepts only `otpauth://` URIs")
                .build(),
            action: AddMenuAction::AddOtp,
        },
    ];

    if clipboard_otp_available {
        items.push(AddMenuItem {
            key: "add-otp-from-clipboard",
            display: format!(
                "{} Add OTP from Clipboard",
                format_icon_colored(NerdFont::Clipboard, colors::MAUVE)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::Clipboard, "Add OTP from Clipboard")
                .text("Use the OTP URI currently stored on your clipboard.")
                .blank()
                .bullet("The entry name is prefilled from the issuer")
                .bullet("The clipboard URI is not shown in this preview")
                .build(),
            action: AddMenuAction::AddOtpFromClipboard,
        });
    }

    items.extend([
        AddMenuItem {
            key: "add-file",
            display: format!(
                "{} Add File",
                format_icon_colored(NerdFont::File, colors::LAVENDER)
            ),
            preview: PreviewBuilder::new()
                .header(NerdFont::File, "Add File")
                .text("Store a file in the pass store.")
                .blank()
                .bullet("Opens the Yazi file picker")
                .bullet("Prompts for a name and suggests the source filename")
                .bullet("Adds a `.file` suffix so the entry exports instead of copying")
                .build(),
            action: AddMenuAction::AddFile,
        },
        AddMenuItem {
            key: "back",
            display: format!("{} Back", format_back_icon()),
            preview: PreviewBuilder::new()
                .header(NerdFont::ArrowLeft, "Back")
                .text("Return to the pass browser.")
                .build(),
            action: AddMenuAction::Back,
        },
    ]);

    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destination_defaults_to_the_store_root() {
        assert_eq!(destination(None), "/");
    }

    #[test]
    fn destination_shows_the_current_folder() {
        assert_eq!(destination(Some("mail/work")), "/mail/work");
    }

    #[test]
    fn add_menu_offers_file_entries() {
        let items = build_items(false);

        assert!(
            items
                .iter()
                .any(|item| matches!(item.action, AddMenuAction::AddFile))
        );
    }

    #[test]
    fn add_menu_only_offers_clipboard_otp_when_available() {
        let without_clipboard = build_items(false);
        assert!(
            without_clipboard
                .iter()
                .all(|item| !matches!(item.action, AddMenuAction::AddOtpFromClipboard))
        );

        let with_clipboard = build_items(true);
        assert!(
            with_clipboard
                .iter()
                .any(|item| matches!(item.action, AddMenuAction::AddOtpFromClipboard))
        );
    }
}
