use anyhow::Result;

use crate::settings::defaultapps::{get_application_info, query_default_app};
use crate::ui::catppuccin::colors;
use crate::ui::prelude::NerdFont;
use crate::ui::preview::PreviewBuilder;

pub(crate) fn render_default_app_preview(
    title: &str,
    icon: NerdFont,
    summary: &str,
    mime_types: &[&str],
) -> Result<String> {
    let mut builder = PreviewBuilder::new()
        .header(icon, title)
        .subtext(summary)
        .blank()
        .line(colors::TEAL, Some(NerdFont::ChevronRight), "MIME Types")
        .bullets(mime_types.iter().copied())
        .blank()
        .subtext("Only apps supporting ALL formats are shown.")
        .blank()
        .line(
            colors::TEAL,
            Some(NerdFont::ChevronRight),
            "Current Defaults",
        );

    for mime in mime_types {
        let label = query_default_app(mime)
            .ok()
            .flatten()
            .map(|desktop_id| display_app_name(&desktop_id))
            .unwrap_or_else(|| "(not set)".to_string());
        builder = builder.field_indented(mime, &label);
    }

    Ok(builder.build_string())
}

pub(crate) fn display_app_name(desktop_id: &str) -> String {
    let info = get_application_info(desktop_id);
    info.name.unwrap_or_else(|| desktop_id.to_string())
}
