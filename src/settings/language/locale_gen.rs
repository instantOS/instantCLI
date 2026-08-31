use std::collections::HashSet;
use std::fs;
use std::io::Write;

use anyhow::{Context, Result};
use tempfile::NamedTempFile;

use super::SettingsContext;
use crate::common::locale_gen::{apply_enable_disable, available_locales};

pub(super) const LOCALE_GEN_PATH: &str = "/etc/locale.gen";

pub(super) fn enabled_locales() -> Result<HashSet<String>> {
    let contents = fs::read_to_string(LOCALE_GEN_PATH)
        .with_context(|| format!("reading {LOCALE_GEN_PATH}"))?;

    Ok(crate::common::locale_gen::enabled_locales(&contents))
}

pub(super) fn all_locale_gen_entries() -> Result<Vec<String>> {
    let contents = fs::read_to_string(LOCALE_GEN_PATH)
        .with_context(|| format!("reading {LOCALE_GEN_PATH}"))?;

    Ok(available_locales(&contents))
}

pub(super) fn apply_locale_gen_updates(
    ctx: &mut SettingsContext,
    enable: &[String],
    disable: &[String],
) -> Result<()> {
    let original = fs::read_to_string(LOCALE_GEN_PATH)
        .with_context(|| format!("reading {LOCALE_GEN_PATH}"))?;

    let Some(updated) = apply_enable_disable(&original, enable, disable) else {
        return Ok(());
    };

    write_locale_gen(ctx, &updated)?;
    Ok(())
}

fn write_locale_gen(ctx: &mut SettingsContext, contents: &str) -> Result<()> {
    let mut temp = NamedTempFile::new().context("creating temporary locale.gen")?;
    temp.write_all(contents.as_bytes())
        .context("writing temporary locale.gen")?;
    temp.flush().context("flushing temporary locale.gen")?;
    let temp_path = temp.into_temp_path();

    ctx.run_command_as_root(
        "install",
        [
            std::ffi::OsStr::new("-m"),
            std::ffi::OsStr::new("644"),
            temp_path.as_os_str(),
            std::ffi::OsStr::new(LOCALE_GEN_PATH),
        ],
    )?;

    temp_path.close().context("removing temporary locale.gen")?;
    Ok(())
}
