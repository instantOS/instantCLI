use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result};

use super::app_info::ApplicationInfoCache;

pub(crate) fn query_default_app(mime_type: &str) -> Result<Option<String>> {
    let output = Command::new("xdg-mime")
        .arg("query")
        .arg("default")
        .arg(mime_type)
        .output()
        .context("Failed to execute xdg-mime query")?;

    if !output.status.success() {
        anyhow::bail!(
            "xdg-mime query failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let default_app = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // xdg-utils #151: the generic backend rejects valid quoted Exec paths and
    // silently returns a fallback, even after successfully saving the default.
    // https://gitlab.freedesktop.org/xdg/xdg-utils/-/issues/151
    // Recover only this case from explicit XDG defaults. Leave other cases to
    // xdg-mime so this workaround can be removed once the upstream fix ships.
    let roots = super::data_dirs()
        .into_iter()
        .map(|root| root.join("applications"))
        .collect::<Vec<_>>();
    if let Some(id) = quoted_default(mime_type, &mimeapps_paths(), roots) {
        return Ok(Some(id));
    }
    if default_app.is_empty() {
        Ok(None)
    } else {
        Ok(Some(default_app))
    }
}

fn mimeapps_paths() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(config) = dirs::config_dir() {
        dirs.push(config);
    }
    let config_dirs = std::env::var_os("XDG_CONFIG_DIRS")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "/etc/xdg".into());
    dirs.extend(std::env::split_paths(&config_dirs).filter(|path| path.is_absolute()));
    dirs.extend(
        super::data_dirs()
            .into_iter()
            .map(|dir| dir.join("applications")),
    );
    let desktops = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    dirs.into_iter()
        .flat_map(|dir| {
            desktops
                .split(':')
                .filter(|desktop| !desktop.is_empty() && !desktop.contains(['/', '\\']))
                .map(|desktop| dir.join(format!("{}-mimeapps.list", desktop.to_lowercase())))
                .chain(std::iter::once(dir.join("mimeapps.list")))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn quoted_default(mime: &str, paths: &[PathBuf], roots: Vec<PathBuf>) -> Option<String> {
    let mut catalog = ApplicationInfoCache::new(roots);
    for path in paths {
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        let mut in_defaults = false;
        for line in content.lines().map(str::trim) {
            if line.starts_with('[') {
                in_defaults = line == "[Default Applications]";
                continue;
            }
            if !in_defaults || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            if key.trim() != mime {
                continue;
            }
            for id in value.split(';').map(str::trim).filter(|id| !id.is_empty()) {
                let app = catalog.get(id);
                if app.path.is_none() {
                    continue;
                }
                let exec = app.exec.as_deref()?;
                let Ok(tokens) = shell_words::split(exec) else {
                    continue;
                };
                if tokens
                    .first()
                    .is_none_or(|command| which::which(command).is_err())
                {
                    continue;
                }
                // Stop at the first usable default, including unquoted ones,
                // so a stale lower-priority entry cannot override it.
                return exec.trim_start().starts_with('"').then(|| id.to_owned());
            }
        }
    }
    None
}

pub(crate) fn set_default_app(mime_type: &str, desktop_file: &str) -> Result<()> {
    let result = set_default_app_without_invalidation(mime_type, desktop_file);
    invalidate_default_app_previews();
    result
}

pub(crate) fn set_default_apps(mime_types: &[&str], desktop_file: &str) -> Result<()> {
    let result = mime_types.iter().try_for_each(|mime_type| {
        set_default_app_without_invalidation(mime_type, desktop_file)
            .with_context(|| format!("Failed to set default for {mime_type}"))
    });
    invalidate_default_app_previews();
    result
}

fn set_default_app_without_invalidation(mime_type: &str, desktop_file: &str) -> Result<()> {
    let status = Command::new("xdg-mime")
        .arg("default")
        .arg(desktop_file)
        .arg(mime_type)
        .status()
        .context("Failed to execute xdg-mime default")?;

    if !status.success() {
        anyhow::bail!("xdg-mime default command failed");
    }

    Ok(())
}

fn invalidate_default_app_previews() {
    for preview in [
        crate::preview::PreviewId::MimeType,
        crate::preview::PreviewId::DefaultImageViewer,
        crate::preview::PreviewId::DefaultVideoPlayer,
        crate::preview::PreviewId::DefaultAudioPlayer,
        crate::preview::PreviewId::DefaultArchiveManager,
        crate::preview::PreviewId::DefaultBrowser,
        crate::preview::PreviewId::DefaultTextEditor,
        crate::preview::PreviewId::DefaultEmail,
        crate::preview::PreviewId::DefaultFileManager,
        crate::preview::PreviewId::DefaultPdfViewer,
    ] {
        let _ = crate::preview::cache::invalidate(preview);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_defaults_respect_precedence_and_skip_unavailable_entries() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let apps = temp.path().join("applications");
        std::fs::create_dir(&apps)?;
        let executable = which::which("true")?;
        for (id, exec, extra) in [
            ("quoted", format!("\"{}\" %F", executable.display()), ""),
            ("plain", executable.display().to_string(), ""),
            (
                "hidden",
                format!("\"{}\"", executable.display()),
                "Hidden=true\n",
            ),
            ("missing", "\"/nonexistent/ins-default-test\"".into(), ""),
        ] {
            std::fs::write(
                apps.join(format!("{id}.desktop")),
                format!("[Desktop Entry]\nType=Application\nName={id}\nExec={exec}\n{extra}"),
            )?;
        }
        let high = temp.path().join("high.list");
        let low = temp.path().join("low.list");
        let paths = [high.clone(), low.clone()];
        std::fs::write(&low, "[Default Applications]\ntext/plain=quoted.desktop;\n")?;
        let lookup = || quoted_default("text/plain", &paths, vec![apps.clone()]);
        assert_eq!(lookup().as_deref(), Some("quoted.desktop"));
        std::fs::write(
            &high,
            "[Default Applications]\ntext/plain=plain.desktop;quoted.desktop;\n",
        )?;
        assert_eq!(lookup(), None);
        std::fs::write(
            &high,
            "[Default Applications]\ntext/plain=absent.desktop;hidden.desktop;missing.desktop;quoted.desktop;\n",
        )?;
        assert_eq!(lookup().as_deref(), Some("quoted.desktop"));
        std::fs::write(
            &high,
            "[Added Associations]\ntext/plain=plain.desktop;\n[Default Applications]\nimage/png=plain.desktop;\n",
        )?;
        assert_eq!(lookup().as_deref(), Some("quoted.desktop"));
        Ok(())
    }
}
