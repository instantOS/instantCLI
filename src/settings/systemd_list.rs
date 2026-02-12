use anyhow::Result;
use std::io::{self, Write};
use std::process::{Command, Stdio};

use crate::common::shell::current_exe_command;
use crate::common::systemd::ServiceScope;

pub fn list_command(scope: &str) -> String {
    let exe = current_exe_command();
    format!(
        "{} settings internal-generate-systemd-list --scope {}",
        exe, scope
    )
}

pub fn generate_and_print_list(scope: &str) -> Result<()> {
    let service_scope = match scope {
        "user" => ServiceScope::User,
        _ => ServiceScope::System,
    };

    stream_services(service_scope)
}

fn stream_services(scope: ServiceScope) -> Result<()> {
    let scope_args: Vec<&str> = match scope {
        ServiceScope::System => vec![],
        ServiceScope::User => vec!["--user"],
    };

    let mut child = Command::new("systemctl")
        .args([
            "list-units",
            "--type=service",
            "--all",
            "--no-pager",
            "--plain",
            "--no-legend",
        ])
        .args(&scope_args)
        .stdout(Stdio::piped())
        .spawn()?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("Failed to capture systemctl stdout"))?;

    let reader = io::BufReader::new(stdout);
    let mut enabled_cache = std::collections::HashMap::new();

    let mut lines: Vec<(String, String, String)> = Vec::new();

    use std::io::BufRead;
    for line in reader.lines() {
        let line = line?;
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }

        let name = parts[0].replace(".service", "");
        let active = parts[2].to_string();
        let description = if parts.len() > 4 {
            parts[4..].join(" ")
        } else {
            String::new()
        };

        lines.push((name, active, description));
    }

    child.wait()?;

    lines.sort_by(|a, b| a.0.cmp(&b.0));

    let scope_str = match scope {
        ServiceScope::System => "system",
        ServiceScope::User => "user",
    };

    let stdout = io::stdout();
    let mut handle = stdout.lock();

    for (name, active, description) in lines {
        let enabled = get_service_enabled_state_cached(&name, scope, &mut enabled_cache);
        let display = format_display(&name, &active, &enabled, &description);
        let key = format!("{}:{}", name, scope_str);

        writeln!(handle, "{}\t{}\t{}", key, display, description)?;
    }

    Ok(())
}

fn get_service_enabled_state_cached(
    name: &str,
    scope: ServiceScope,
    cache: &mut std::collections::HashMap<String, String>,
) -> String {
    if let Some(cached) = cache.get(name) {
        return cached.clone();
    }

    let state = get_service_enabled_state(name, scope);
    cache.insert(name.to_string(), state.clone());
    state
}

fn get_service_enabled_state(name: &str, scope: ServiceScope) -> String {
    let scope_args: Vec<&str> = match scope {
        ServiceScope::System => vec![],
        ServiceScope::User => vec!["--user"],
    };

    let output = Command::new("systemctl")
        .args(["is-enabled", name])
        .args(&scope_args)
        .output();

    match output {
        Ok(o) => {
            let state = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if o.status.success() {
                state
            } else {
                match state.as_str() {
                    "disabled" => "disabled".to_string(),
                    "not-found" => "transient".to_string(),
                    "static" => "static".to_string(),
                    "indirect" => "indirect".to_string(),
                    "masked" => "masked".to_string(),
                    "linked" => "linked".to_string(),
                    _ => state,
                }
            }
        }
        Err(_) => "unknown".to_string(),
    }
}

fn format_display(name: &str, active: &str, _enabled: &str, description: &str) -> String {
    use crate::ui::catppuccin::{colors, format_icon_colored};

    let (active_icon, active_color) = match active {
        "active" => (NerdFont::CheckCircle, colors::GREEN),
        "failed" => (NerdFont::CrossCircle, colors::RED),
        "inactive" => (NerdFont::Circle, colors::OVERLAY0),
        _ => (NerdFont::Question, colors::YELLOW),
    };

    use crate::ui::nerd_font::NerdFont;
    let icon_str = format_icon_colored(active_icon, active_color);
    let truncated = truncate(description, 40);

    format!("{}{} - {}", icon_str, name, truncated)
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}...", &s[..max_len - 3])
    } else {
        s.to_string()
    }
}
