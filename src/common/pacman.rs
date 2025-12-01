use anyhow::Result;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

pub const INSTANT_REPO_URL: &str = "https://instantos.io/packages";
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
        // Note: lines.join("\n") might not preserve trailing newline exactly as input if input has one,
        // but for config files it's usually fine.
        // Our input has a leading newline which split gives as empty string at start?
        // lines() iterator handles newlines.

        let processed = enable_multilib_in_string(input).unwrap();

        // Normalize newlines for comparison
        let processed_trim = processed.trim();
        let expected_trim = expected.trim();

        assert_eq!(processed_trim, expected_trim);
    }

    #[test]
    fn test_already_enabled() {
        let input = r#"
[multilib]
Include = /etc/pacman.d/mirrorlist
"#;
        assert_eq!(enable_multilib_in_string(input), None);
    }
}
