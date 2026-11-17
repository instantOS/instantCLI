use anyhow::{Context, Result};

use crate::applications::types::DesktopAppDetails;
use crate::common::desktop_entry::DesktopEntry;

/// Parse the selected file again immediately before launch. Discovery carries
/// its resolved path so nested IDs and XDG precedence remain exact.
pub fn load_desktop_details(file_path: &std::path::Path) -> Result<DesktopAppDetails> {
    let content = std::fs::read_to_string(file_path).context("Failed to read desktop file")?;
    let entry = DesktopEntry::parse(&content).context("Failed to parse desktop file")?;
    if entry.entry_type != Some("Application") {
        anyhow::bail!("Desktop file is not an application");
    }

    Ok(DesktopAppDetails {
        exec: entry.exec.unwrap_or_default().to_string(),
        name: entry.name.unwrap_or_default().to_string(),
        icon: entry.icon.map(ToOwned::to_owned),
        desktop_path: file_path.to_path_buf(),
        no_display: entry.no_display,
        terminal: entry.terminal,
    })
}

impl DesktopAppDetails {
    /// Execute a desktop application
    pub fn execute(&self) -> Result<()> {
        if self.no_display {
            anyhow::bail!("Application is marked as not displayable");
        }
        self.execute_with_file(None)
    }

    /// Open a local path using desktop field codes, preserving it as one argument.
    pub(crate) fn open_file(&self, path: &std::path::Path) -> Result<()> {
        self.execute_with_file(Some(path))
    }

    fn execute_with_file(&self, file: Option<&std::path::Path>) -> Result<()> {
        let parts = expand_exec_field_codes_for_file(
            &self.exec,
            &self.name,
            self.icon.as_deref(),
            &self.desktop_path,
            file,
        )?;
        if parts.is_empty() {
            return Err(anyhow::anyhow!("Empty Exec command"));
        }

        if self.terminal {
            instantos_terminal::Launcher::new(&parts[0])
                .args(&parts[1..])
                .spawn_detached()
                .context("Failed to launch terminal desktop app")?;
            return Ok(());
        }

        let mut cmd = std::process::Command::new(&parts[0]);
        cmd.args(&parts[1..]);
        // A picker launched directly by a terminal can be its session leader.
        // Keep the application out of that foreground process group so closing
        // the picker terminal does not send it SIGHUP before it starts.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }

        cmd.stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .stdin(std::process::Stdio::null())
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to launch desktop app: {}", e))?;

        Ok(())
    }
}

/// Expand field codes in Exec string
#[cfg(test)]
fn expand_exec_field_codes(
    exec: &str,
    name: &str,
    icon: Option<&str>,
    desktop_path: &std::path::Path,
) -> Result<Vec<std::ffi::OsString>> {
    expand_exec_field_codes_for_file(exec, name, icon, desktop_path, None)
}

fn expand_exec_field_codes_for_file(
    exec: &str,
    name: &str,
    icon: Option<&str>,
    desktop_path: &std::path::Path,
    file: Option<&std::path::Path>,
) -> Result<Vec<std::ffi::OsString>> {
    let tokens = shell_words::split(exec).context("Failed to parse desktop Exec command")?;
    if tokens
        .first()
        .is_none_or(|token| token.is_empty() || token.starts_with('%'))
    {
        anyhow::bail!("Desktop Exec command has no executable");
    }
    let mut expanded = Vec::with_capacity(tokens.len());
    let mut supplied_file = false;
    for token in tokens {
        match token.as_str() {
            "%f" | "%F" | "%u" | "%U" => {
                if let Some(path) = file {
                    supplied_file = true;
                    if matches!(token.as_str(), "%u" | "%U") {
                        let uri = reqwest::Url::from_file_path(path)
                            .map_err(|_| anyhow::anyhow!("Cannot convert path to file URI"))?;
                        expanded.push(std::ffi::OsString::from(uri.as_str()));
                    } else {
                        expanded.push(path.as_os_str().to_owned());
                    }
                }
            }
            "%c" => expanded.push(name.into()),
            "%k" => expanded.push(desktop_path.as_os_str().to_owned()),
            "%i" => {
                if let Some(icon) = icon {
                    expanded.push("--icon".into());
                    expanded.push(icon.into());
                }
            }
            _ => {
                expanded.push(
                    expand_embedded_codes(&token, name, desktop_path, file.is_some())?.into(),
                );
            }
        }
    }
    if !supplied_file && let Some(path) = file {
        expanded.push(path.as_os_str().to_owned());
    }
    expanded.retain(|argument| !argument.is_empty());
    Ok(expanded)
}

fn expand_embedded_codes(
    token: &str,
    name: &str,
    desktop_path: &std::path::Path,
    reject_file_codes: bool,
) -> Result<String> {
    let mut result = String::with_capacity(token.len());
    let mut characters = token.chars();
    while let Some(character) = characters.next() {
        if character != '%' {
            result.push(character);
            continue;
        }
        match characters.next() {
            Some('%') => result.push('%'),
            Some('c') => result.push_str(name),
            Some('k') => result.push_str(&desktop_path.to_string_lossy()),
            Some('f' | 'F' | 'u' | 'U') if reject_file_codes => {
                anyhow::bail!("File field codes must be separate arguments in desktop Exec");
            }
            Some('f' | 'F' | 'u' | 'U' | 'i') => {}
            Some(code) => anyhow::bail!("Unsupported desktop Exec field code %{code}"),
            None => anyhow::bail!("Trailing '%' in desktop Exec command"),
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_arguments_preserve_paths_and_encode_uris() -> Result<()> {
        let desktop = std::path::Path::new("/apps/example.desktop");
        let path = std::path::Path::new("/tmp/a file #1.txt");
        for placeholder in ["%f", "%F"] {
            let args = expand_exec_field_codes_for_file(
                &format!("app {placeholder}"),
                "App",
                None,
                desktop,
                Some(path),
            )?;
            assert_eq!(
                args,
                [std::ffi::OsString::from("app"), path.as_os_str().to_owned()]
            );
        }
        for placeholder in ["%u", "%U"] {
            let args = expand_exec_field_codes_for_file(
                &format!("app {placeholder}"),
                "App",
                None,
                desktop,
                Some(path),
            )?;
            assert_eq!(args[1], "file:///tmp/a%20file%20%231.txt");
        }
        let args =
            expand_exec_field_codes_for_file("app --flag", "App", None, desktop, Some(path))?;
        assert_eq!(args[2], path.as_os_str());
        assert!(expand_exec_field_codes_for_file("", "App", None, desktop, Some(path)).is_err());
        Ok(())
    }

    #[test]
    fn file_arguments_preserve_non_utf8_names() -> Result<()> {
        use std::os::unix::ffi::OsStringExt;
        let name = std::ffi::OsString::from_vec(b"/tmp/file\xff".to_vec());
        let args = expand_exec_field_codes_for_file(
            "app %f",
            "App",
            None,
            std::path::Path::new("/apps/app.desktop"),
            Some(std::path::Path::new(&name)),
        )?;
        assert_eq!(args[1], name);
        Ok(())
    }

    #[test]
    fn exec_expansion_preserves_quoted_arguments_and_literal_percent() {
        let args = expand_exec_field_codes(
            r#"app --title "Two words" --value=100%% %f"#,
            "Example",
            None,
            std::path::Path::new("/apps/example.desktop"),
        )
        .unwrap();
        assert_eq!(args, ["app", "--title", "Two words", "--value=100%"]);
    }

    #[test]
    fn exec_expansion_supplies_desktop_metadata() {
        let args = expand_exec_field_codes(
            "app %i %c %k",
            "Example App",
            Some("example"),
            std::path::Path::new("/apps/example.desktop"),
        )
        .unwrap();
        assert_eq!(
            args,
            [
                "app",
                "--icon",
                "example",
                "Example App",
                "/apps/example.desktop"
            ]
        );
    }
}
