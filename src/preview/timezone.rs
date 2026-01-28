use std::process::Command;

use anyhow::Result;

use crate::preview::PreviewContext;
use crate::preview::helpers::command_output_optional;
use crate::ui::catppuccin::colors;
use crate::ui::prelude::NerdFont;
use crate::ui::preview::PreviewBuilder;

pub(crate) fn render_timezone_preview(ctx: &PreviewContext) -> Result<String> {
    let Some(tz) = ctx.key() else {
        return Ok(String::new());
    };

    let current_local =
        date_in_tz(tz, "+%Y-%m-%d %H:%M:%S %Z").unwrap_or_else(|| "Unavailable".to_string());
    let day_line = date_in_tz(tz, "+%A, %d %B %Y").unwrap_or_else(|| "Unavailable".to_string());
    let twelve_hour = date_in_tz(tz, "+%I:%M %p").unwrap_or_else(|| "Unavailable".to_string());
    let twenty_four = date_in_tz(tz, "+%H:%M").unwrap_or_else(|| "Unavailable".to_string());
    let offset_raw = date_in_tz(tz, "+%z").unwrap_or_else(|| "Unknown".to_string());
    let local_system =
        date_local("+%Y-%m-%d %H:%M:%S %Z").unwrap_or_else(|| "Unavailable".to_string());

    let offset = format_utc_offset(&offset_raw);

    let builder = PreviewBuilder::new()
        .header(NerdFont::Clock, "Timezone")
        .subtext("Preview of the selected timezone.")
        .blank()
        .field("Timezone", tz)
        .blank()
        .line(colors::TEAL, None, "Current Time")
        .raw(&format!("  {current_local}"))
        .raw(&format!("  {day_line}"))
        .blank()
        .line(colors::TEAL, None, "UTC Offset")
        .raw(&format!("  {offset}"))
        .blank()
        .line(colors::TEAL, None, "Formats")
        .raw(&format!("  12-hour: {twelve_hour}"))
        .raw(&format!("  24-hour: {twenty_four}"))
        .blank()
        .line(colors::TEAL, None, "Local System Time")
        .raw(&format!("  {local_system}"));

    Ok(builder.build_string())
}

fn date_in_tz(tz: &str, format: &str) -> Option<String> {
    let mut cmd = Command::new("date");
    cmd.arg(format).env("TZ", tz);
    command_output_optional(cmd)
}

fn date_local(format: &str) -> Option<String> {
    let mut cmd = Command::new("date");
    cmd.arg(format);
    command_output_optional(cmd)
}

fn format_utc_offset(raw: &str) -> String {
    if raw.len() == 5 {
        format!("UTC{}:{}", &raw[..3], &raw[3..])
    } else {
        format!("UTC{raw}")
    }
}
