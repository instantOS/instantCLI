use crate::common::network::{check_internet, get_local_ip, get_public_ip};
use crate::menu_utils::FzfWrapper;
use crate::ui::prelude::*;
use anyhow::Result;

use super::context::SettingsContext;

/// Show IP address information
// TODO: this should be moved to the definition
pub fn show_ip_info(ctx: &mut SettingsContext) -> Result<()> {
    ctx.emit_info(
        "settings.network.ip_info",
        "Gathering network information...",
    );

    // Get local IP address
    let local_ip = get_local_ip();

    // Check internet connectivity
    let has_internet = check_internet();

    // Get public IP address (only if internet is available)
    let public_ip = if has_internet {
        get_public_ip().ok()
    } else {
        None
    };

    // Build the message
    let mut message = String::new();
    message.push_str("═══════════════════════════════════════\n");
    message.push_str("         Network Information\n");
    message.push_str("═══════════════════════════════════════\n\n");

    // Internet status
    if has_internet {
        message.push_str(&format!(
            "{}  Internet:  Connected\n",
            char::from(NerdFont::CheckCircle)
        ));
    } else {
        message.push_str(&format!(
            "{}  Internet:  Not connected\n",
            char::from(NerdFont::CrossCircle)
        ));
    }

    message.push('\n');

    // Local IP
    if let Some(ref local) = local_ip {
        message.push_str(&format!(
            "{}  Local IP:  {}\n",
            char::from(NerdFont::Desktop),
            local
        ));
    } else {
        message.push_str(&format!(
            "{}  Local IP:  Not found\n",
            char::from(NerdFont::Warning)
        ));
    }

    // Public IP
    if let Some(ref public) = public_ip {
        message.push_str(&format!(
            "{}  Public IP: {}\n",
            char::from(NerdFont::Globe),
            public
        ));
    } else if has_internet {
        message.push_str(&format!(
            "{}  Public IP: Unable to retrieve\n",
            char::from(NerdFont::Warning)
        ));
    } else {
        message.push_str(&format!(
            "{}  Public IP: Not available (no internet)\n",
            char::from(NerdFont::Warning)
        ));
    }

    message.push('\n');

    // Additional status message
    if local_ip.is_none() && !has_internet {
        message.push_str(&format!(
            "{} No network connection detected\n",
            char::from(NerdFont::CrossCircle)
        ));
    } else if local_ip.is_some() && !has_internet {
        message.push_str(&format!(
            "{} Local network only (no internet access)\n",
            char::from(NerdFont::Info)
        ));
    }

    // Show the message
    FzfWrapper::builder()
        .title("Network Information")
        .message(message)
        .message_dialog()?;

    ctx.emit_success(
        "settings.network.ip_info.shown",
        "Network information displayed",
    );

    Ok(())
}
