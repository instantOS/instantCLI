use anyhow::{Context, Result};
use colored::Colorize;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use tempfile::NamedTempFile;

const SNIPS_KEY: &str = "-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
QyNTUxOQAAACDRzUC9CRt7es9BJmUrI+sDt6nG6CsSBvtfOeAvcR/J7gAAAJBM/19nTP9f
ZwAAAAtzc2gtZWQyNTUxOQAAACDRzUC9CRt7es9BJmUrI+sDt6nG6CsSBvtfOeAvcR/J7g
AAAEA+b6NfYeO8B3xNNqiixJPfcRrw2zQhmdA8uCFodPK4etHNQL0JG3t6z0EmZSsj6wO3
qcboKxIG+1854C9xH8nuAAAADWJlbmphbWluQHJ4cGM=
-----END OPENSSH PRIVATE KEY-----";

pub fn process_log_upload(context: &crate::arch::engine::InstallContext) {
    if context.get_answer_bool(crate::arch::engine::QuestionId::LogUpload) {
        println!("Uploading installation logs as requested...");
        let log_path = std::path::PathBuf::from(crate::arch::execution::paths::LOG_FILE);
        match upload_logs(&log_path) {
            Ok(url) => println!("Logs uploaded successfully: {}", url.green().bold()),
            Err(e) => eprintln!("Failed to upload logs: {}", e),
        }
    }
}

pub fn upload_logs(log_path: &Path) -> Result<String> {
    if !log_path.exists() {
        anyhow::bail!("Log file not found: {}", log_path.display());
    }

    // Create a temporary file for the key
    let mut key_file = NamedTempFile::new().context("Failed to create temporary key file")?;
    key_file
        .write_all(SNIPS_KEY.as_bytes())
        .context("Failed to write key to temporary file")?;

    // Ensure the key file has correct permissions (0600)
    // NamedTempFile is created with 0600 on Unix by default, but let's be explicit if needed or rely on tempfile crate guarantees.
    // The tempfile crate documentation says: "The file is created with mode 0600 on Unix-like systems."

    let key_path = key_file.path().to_path_buf();

    // Construct the SSH command
    // cat log_file | ssh -i key_file -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null instantos@snips.sh

    let output = Command::new("ssh")
        .arg("-i")
        .arg(&key_path)
        .arg("-o")
        .arg("StrictHostKeyChecking=no")
        .arg("-o")
        .arg("UserKnownHostsFile=/dev/null")
        .arg("instantos@snips.sh")
        .stdin(std::fs::File::open(log_path)?)
        .output()
        .context("Failed to execute ssh command")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to upload logs: {}", stderr);
    }

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // The key file is automatically deleted when key_file goes out of scope

    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_structure() {
        assert!(SNIPS_KEY.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----"));
        assert!(SNIPS_KEY.ends_with("-----END OPENSSH PRIVATE KEY-----"));
    }
}
