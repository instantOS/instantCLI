use std::{
    collections::{HashSet, VecDeque},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};

use super::models::{default_shell, UserInfo};

const SUDOERS_PATH: &str = "/etc/sudoers";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WheelSudoStatus {
    Allowed,
    Denied,
    Unknown,
}

pub(super) fn wheel_sudo_status() -> WheelSudoStatus {
    match sudoers_allows_wheel() {
        Ok(true) => WheelSudoStatus::Allowed,
        Ok(false) => WheelSudoStatus::Denied,
        Err(_) => WheelSudoStatus::Unknown,
    }
}

/// Get information about a system user
pub(super) fn get_user_info(username: &str) -> Result<Option<UserInfo>> {
    let passwd = Command::new("getent")
        .arg("passwd")
        .arg(username)
        .output()
        .with_context(|| format!("querying passwd entry for {}", username))?;

    if !passwd.status.success() {
        return Ok(None);
    }

    let line = String::from_utf8(passwd.stdout).context("parsing passwd entry")?;
    let mut fields = line.trim().split(':');
    let _name = fields.next();
    let _pw = fields.next();
    let _uid = fields.next();
    let _gid = fields.next();
    let _gecos = fields.next();
    let _home = fields.next();
    let shell = fields
        .next()
        .map(str::to_string)
        .unwrap_or_else(default_shell);

    let primary_group = get_user_primary_group(username);
    let groups = get_user_groups(username);

    Ok(Some(UserInfo {
        shell,
        primary_group,
        groups,
    }))
}

/// Get the primary group name for a user
fn get_user_primary_group(username: &str) -> Option<String> {
    Command::new("id")
        .arg("-gn")
        .arg(username)
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
            } else {
                None
            }
        })
}

/// Get all groups for a user
fn get_user_groups(username: &str) -> Vec<String> {
    Command::new("id")
        .arg("-nG")
        .arg(username)
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                Some(
                    String::from_utf8_lossy(&output.stdout)
                        .split_whitespace()
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>(),
                )
            } else {
                None
            }
        })
        .unwrap_or_default()
}

/// Get all system users with home directories in /home/
pub(super) fn get_system_users_with_home() -> Result<Vec<String>> {
    let output = Command::new("getent")
        .arg("passwd")
        .output()
        .context("querying system users")?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let passwd_data = String::from_utf8_lossy(&output.stdout);
    let users: Vec<String> = passwd_data
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(':').collect();
            if fields.len() >= 7 {
                let username = fields[0];
                let home = fields[5];
                if home.starts_with("/home/") {
                    return Some(username.to_string());
                }
            }
            None
        })
        .collect();

    Ok(users)
}

/// Get all system groups
pub(super) fn get_all_system_groups() -> Result<Vec<String>> {
    let output = Command::new("getent")
        .arg("group")
        .output()
        .context("querying system groups")?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let group_data = String::from_utf8_lossy(&output.stdout);
    let groups: Vec<String> = group_data
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(':').collect();
            if !fields.is_empty() {
                Some(fields[0].to_string())
            } else {
                None
            }
        })
        .collect();

    Ok(groups)
}

fn sudoers_allows_wheel() -> Result<bool> {
    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();
    queue.push_back(PathBuf::from(SUDOERS_PATH));

    while let Some(path) = queue.pop_front() {
        if !visited.insert(path.clone()) {
            continue;
        }

        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(err) => {
                if path.as_path() == Path::new(SUDOERS_PATH) {
                    return Err(err.into());
                }
                continue;
            }
        };

        if scan_sudoers_contents(&path, &contents, &mut queue) {
            return Ok(true);
        }
    }

    Ok(false)
}

fn scan_sudoers_contents(
    current_path: &Path,
    contents: &str,
    queue: &mut VecDeque<PathBuf>,
) -> bool {
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let lower = trimmed.to_lowercase();
        if lower.starts_with("#includedir") {
            if let Some(path) = parse_include_path(trimmed, current_path) {
                enqueue_includedir(&path, queue);
            }
            continue;
        }

        if lower.starts_with("#include") {
            if let Some(path) = parse_include_path(trimmed, current_path) {
                queue.push_back(path);
            }
            continue;
        }

        let stripped = strip_inline_comment(trimmed);
        if stripped.is_empty() {
            continue;
        }

        if stripped.starts_with("%wheel") {
            return true;
        }
    }

    false
}

fn strip_inline_comment(line: &str) -> &str {
    match line.find('#') {
        Some(index) => line[..index].trim(),
        None => line.trim(),
    }
}

fn parse_include_path(line: &str, current_path: &Path) -> Option<PathBuf> {
    let mut parts = line.split_whitespace();
    let _directive = parts.next()?;
    let path = parts.next()?;
    let cleaned = trim_quotes(path);
    let include_path = PathBuf::from(cleaned);
    if include_path.is_absolute() {
        Some(include_path)
    } else {
        Some(
            current_path
                .parent()
                .unwrap_or_else(|| Path::new("/"))
                .join(include_path),
        )
    }
}

fn trim_quotes(value: &str) -> &str {
    let trimmed = value.trim();
    if trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
    {
        return &trimmed[1..trimmed.len() - 1];
    }
    trimmed
}

fn enqueue_includedir(path: &Path, queue: &mut VecDeque<PathBuf>) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && is_sudoers_included_file(&path) {
            queue.push_back(path);
        }
    }
}

fn is_sudoers_included_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };

    if name.starts_with('.') || name.ends_with('~') || name.contains('.') {
        return false;
    }

    true
}

/// Check if a group exists on the system
pub(super) fn group_exists(name: &str) -> Result<bool> {
    let status = Command::new("getent")
        .arg("group")
        .arg(name)
        .status()
        .with_context(|| format!("checking group {name}"))?;
    Ok(status.success())
}

/// Get available shells from /etc/shells
pub(super) fn get_available_shells() -> Result<Vec<String>> {
    let shells_path = std::path::Path::new("/etc/shells");
    if !shells_path.exists() {
        return Ok(vec![default_shell()]);
    }

    let contents = fs::read_to_string(shells_path).context("reading /etc/shells")?;

    let shells: Vec<String> = contents
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.to_string())
        .collect();

    if shells.is_empty() {
        Ok(vec![default_shell()])
    } else {
        Ok(shells)
    }
}

/// Partition groups into valid and missing groups
pub(super) fn partition_groups(groups: &[String]) -> Result<(Vec<String>, Vec<String>)> {
    let mut valid = Vec::new();
    let mut missing = Vec::new();

    for group in groups {
        if group_exists(group)? {
            valid.push(group.clone());
        } else {
            missing.push(group.clone());
        }
    }

    Ok((valid, missing))
}
