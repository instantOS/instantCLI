use anyhow::Result;

pub fn detect_single_user() -> Option<String> {
    let home = std::path::Path::new("/home");
    if !home.exists() {
        return None;
    }

    let entries = match std::fs::read_dir(home) {
        Ok(e) => e,
        Err(_) => return None,
    };

    let mut users = Vec::new();
    for entry in entries.flatten() {
        if let Ok(file_type) = entry.file_type()
            && file_type.is_dir()
            && let Ok(name) = entry.file_name().into_string()
            && name != "lost+found"
        {
            users.push(name);
        }
    }

    if users.len() == 1 {
        Some(users[0].clone())
    } else {
        None
    }
}

pub fn ensure_root() -> Result<()> {
    if let sudo::RunningAs::User = sudo::check() {
        sudo::with_env(&["RUST_BACKTRACE", "RUST_LOG"])
            .map_err(|e| anyhow::anyhow!("Failed to escalate privileges: {}", e))?;
    }
    Ok(())
}
