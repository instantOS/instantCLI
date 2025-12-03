use anyhow::Result;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

pub const INSTANT_MIRRORLIST: &str = include_str!("instantmirrorlist");

pub async fn setup_instant_repo(dry_run: bool) -> Result<()> {
    if dry_run {
        println!("[DRY RUN] Appending [instant] config to /etc/pacman.conf");
        println!("[DRY RUN] Creating /etc/pacman.d/instantmirrorlist");
        return Ok(());
    }

    let pacman_conf = "/etc/pacman.conf";

    // Check if already exists to avoid duplication
    // Note: Doctor check does this check before calling fix, but good to have here too.
    match tokio::fs::read_to_string(pacman_conf).await {
        Ok(content) => {
            if content.contains("[instant]") {
                return Ok(());
            }
        }
        Err(_) => {
            // If file doesn't exist, we might be in trouble, but OpenOptions create(true) will handle it
        }
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(pacman_conf)
        .await?;

    file.write_all(
        b"\n[instant]\nSigLevel = Optional TrustAll\nInclude = /etc/pacman.d/instantmirrorlist\n",
    )
    .await?;

    // Create the mirrorlist file
    tokio::fs::write("/etc/pacman.d/instantmirrorlist", INSTANT_MIRRORLIST).await?;

    println!("Added InstantOS repository to /etc/pacman.conf");
    Ok(())
}

pub async fn enable_multilib(dry_run: bool) -> Result<()> {
    if dry_run {
        println!("[DRY RUN] Enabling [multilib] in /etc/pacman.conf");
        return Ok(());
    }

    let pacman_conf = "/etc/pacman.conf";
    let content = tokio::fs::read_to_string(pacman_conf).await?;

    if let Some(new_content) = enable_multilib_in_string(&content) {
        tokio::fs::write(pacman_conf, new_content).await?;
        println!("Enabled [multilib] repository in /etc/pacman.conf");
    } else {
        println!("[multilib] already enabled or not found in /etc/pacman.conf");
    }

    Ok(())
}

fn enable_multilib_in_string(content: &str) -> Option<String> {
    // We look for #[multilib] and the following #Include
    // Pattern:
    // #[multilib]
    // #Include = ...

    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    let mut changed = false;
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();
        if line == "#[multilib]" {
            // Found commented multilib section
            lines[i] = "[multilib]".to_string();
            changed = true;

            // Check next line for Include
            if i + 1 < lines.len() {
                let next_line = lines[i + 1].trim();
                if next_line.starts_with("#Include") {
                    lines[i + 1] = next_line.replacen("#", "", 1);
                }
            }
        }
        i += 1;
    }

    if changed {
        Some(lines.join("\n"))
    } else {
        None
    }
}

pub async fn configure_pacman_settings(path: Option<&str>, dry_run: bool) -> Result<()> {
    let config_path = path.unwrap_or("/etc/pacman.conf");
    if dry_run {
        println!(
            "[DRY RUN] Configuring pacman settings (Candy, Color, ParallelDownloads) in {}",
            config_path
        );
        return Ok(());
    }

    match tokio::fs::read_to_string(config_path).await {
        Ok(content) => {
            if let Some(new_content) = process_pacman_settings(&content) {
                tokio::fs::write(config_path, new_content).await?;
                println!("Configured pacman settings in {}", config_path);
            } else {
                println!("Pacman settings already configured in {}", config_path);
            }
        }
        Err(e) => {
            println!("Warning: Could not read {}: {}", config_path, e);
        }
    }
    Ok(())
}

fn process_pacman_settings(content: &str) -> Option<String> {
    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    let mut changed = false;
    let mut has_candy = false;
    let mut options_idx = None;
    let mut verbose_pkg_lists_idx = None;

    // First pass: analyze structure
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed == "[options]" {
            options_idx = Some(i);
        } else if trimmed == "ILoveCandy" {
            has_candy = true;
        } else if trimmed == "VerbosePkgLists" {
            verbose_pkg_lists_idx = Some(i);
        }
    }

    // Second pass: modifications
    for line in &mut lines {
        let trimmed = line.trim();
        if trimmed == "#Color" {
            *line = "Color".to_string();
            changed = true;
        } else if trimmed.starts_with("#ParallelDownloads") {
            *line = line.replacen('#', "", 1);
            changed = true;
        }
    }

    // Handle ILoveCandy insertion
    // We need to re-calculate indices or just insert if we haven't found it
    if !has_candy {
        if let Some(idx) = verbose_pkg_lists_idx {
            // Insert after VerbosePkgLists
            // Note: indices might have shifted if we modified lines? No, we only modified in place.
            // But we need to be careful if we iterate.
            // Vec::insert shifts elements.
            if idx + 1 <= lines.len() {
                lines.insert(idx + 1, "ILoveCandy".to_string());
                changed = true;
            }
        } else if let Some(idx) = options_idx {
            if idx + 1 <= lines.len() {
                lines.insert(idx + 1, "ILoveCandy".to_string());
                changed = true;
            }
        }
    }

    if changed {
        Some(lines.join("\n"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_enable_multilib() {
        let input = r#"
# Some comments
#[multilib]
#Include = /etc/pacman.d/mirrorlist

#[custom]
#Include = ...
"#;
        let expected = r#"
# Some comments
[multilib]
Include = /etc/pacman.d/mirrorlist

#[custom]
#Include = ...
"#;
        let processed = enable_multilib_in_string(input).unwrap();
        assert_eq!(processed.trim(), expected.trim());
    }

    #[test]
    fn test_already_enabled() {
        let input = r#"
[multilib]
Include = /etc/pacman.d/mirrorlist
"#;
        assert_eq!(enable_multilib_in_string(input), None);
    }

    #[test]
    fn test_process_pacman_settings() {
        let input = r#"
[options]
#VerbosePkgLists
#Color
#ParallelDownloads = 5
"#;
        let expected = r#"
[options]
ILoveCandy
#VerbosePkgLists
Color
ParallelDownloads = 5
"#;
        // Note: ILoveCandy inserted after [options] because VerbosePkgLists is commented out
        // Wait, in my logic: if VerbosePkgLists is commented, verbose_pkg_lists_idx is None.
        // So it falls back to options_idx.

        let processed = process_pacman_settings(input).unwrap();
        assert_eq!(processed.trim(), expected.trim());
    }

    #[test]
    fn test_process_pacman_settings_with_verbose() {
        let input = r#"
[options]
VerbosePkgLists
#Color
"#;
        let expected = r#"
[options]
VerbosePkgLists
ILoveCandy
Color
"#;
        let processed = process_pacman_settings(input).unwrap();
        assert_eq!(processed.trim(), expected.trim());
    }
}
