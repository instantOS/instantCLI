use anyhow::Result;

use crate::assist::utils::{copy_to_clipboard, get_clipboard_content};
use crate::common::display_server::DisplayServer;

/// Look for a complete, valid TOTP URI on the clipboard.
pub(super) fn clipboard_otp_uri() -> Option<String> {
    let display_server = DisplayServer::detect();
    if !display_server.is_desktop_session() {
        return None;
    }

    let content = get_clipboard_content(&display_server).ok()?;
    let uri = content
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("otpauth://") && super::totp::OtpAuth::parse(line).is_ok())?;
    Some(uri.to_string())
}

pub(super) fn copy_secret_to_clipboard(data: &[u8]) -> Result<()> {
    let display_server = DisplayServer::detect();
    copy_to_clipboard(data, &display_server)?;
    Ok(())
}
