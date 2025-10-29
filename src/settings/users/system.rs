use std::{fs, process::Command};

use anyhow::{Context, Result};

use super::models::{default_shell, UserInfo};

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

