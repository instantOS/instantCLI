use anyhow::Result;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

pub async fn setup_instant_repo(dry_run: bool) -> Result<()> {
    if dry_run {
        println!("[DRY RUN] Appending [instant] config to /etc/pacman.conf");
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
        b"\n[instant]\nSigLevel = Optional TrustAll\nServer = http://instantos.io/repo/$arch\n",
    )
    .await?;

    println!("Added InstantOS repository to /etc/pacman.conf");
    Ok(())
}
