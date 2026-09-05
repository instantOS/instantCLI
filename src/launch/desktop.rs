use crate::launch::types::DesktopAppDetails;
use anyhow::{Context, Result};
use freedesktop_file_parser::{EntryType, parse};

/// Parse the selected file again immediately before launch. Discovery carries
/// its resolved path so nested IDs and XDG precedence remain exact.
pub fn load_desktop_details(file_path: &std::path::Path) -> Result<DesktopAppDetails> {
    let content = std::fs::read_to_string(file_path).context("Failed to read desktop file")?;
    let desktop_file = parse(&content).context("Failed to parse desktop file")?;

    let (exec, terminal) = match &desktop_file.entry.entry_type {
        EntryType::Application(app) => {
            let exec = app.exec.clone().unwrap_or_default();
            let terminal = app.terminal.unwrap_or(false);
            (exec, terminal)
        }
        _ => (String::new(), false), // Fallback for non-application types
    };

    Ok(DesktopAppDetails {
        exec,
        name: desktop_file.entry.name.default,
        icon: desktop_file.entry.icon.map(|icon| icon.content),
        desktop_path: file_path.to_path_buf(),
        no_display: desktop_file.entry.no_display.unwrap_or(false),
        terminal,
    })
}

impl DesktopAppDetails {
    /// Execute a desktop application
    pub fn execute(&self) -> Result<()> {
        if self.no_display {
            return Err(anyhow::anyhow!("Application is marked as not displayable"));
        }

        let parts = expand_exec_field_codes(
            &self.exec,
            &self.name,
            self.icon.as_deref(),
            &self.desktop_path,
        )?;
        if parts.is_empty() {
            return Err(anyhow::anyhow!("Empty Exec command"));
        }

        let mut cmd = std::process::Command::new(&parts[0]);

        for arg in &parts[1..] {
            cmd.arg(arg);
        }

        if self.terminal {
            crate::common::terminal::wrap_with_terminal(&mut cmd)?;
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
fn expand_exec_field_codes(
    exec: &str,
    name: &str,
    icon: Option<&str>,
    desktop_path: &std::path::Path,
) -> Result<Vec<String>> {
    let tokens = shell_words::split(exec).context("Failed to parse desktop Exec command")?;
    let mut expanded = Vec::with_capacity(tokens.len());
    for token in tokens {
        match token.as_str() {
            "%f" | "%F" | "%u" | "%U" => {}
            "%c" => expanded.push(name.to_string()),
            "%k" => expanded.push(desktop_path.to_string_lossy().into_owned()),
            "%i" => {
                if let Some(icon) = icon {
                    expanded.push("--icon".to_string());
                    expanded.push(icon.to_string());
                }
            }
            _ => expanded.push(expand_embedded_codes(&token, name, desktop_path)?),
        }
    }
    expanded.retain(|argument| !argument.is_empty());
    Ok(expanded)
}

fn expand_embedded_codes(
    token: &str,
    name: &str,
    desktop_path: &std::path::Path,
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
