//! Authenticate the bootstrap before extracting or executing any of it.
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use serde::Deserialize;
use tokio::io::AsyncWriteExt;

const RELEASES: &str = "https://archlinux.org/releng/releases/json/";
const SIGNER: &str = "3E80CA1A8B89F69CBA57D98A76A5EF9054449A5C";
const KEY: &str = include_str!("../../../resources/archlinux-release-key.asc");

#[derive(Deserialize)]
struct Releases {
    releases: Vec<Release>,
}

#[derive(Deserialize)]
struct Release {
    version: String,
    available: bool,
    pgp_fingerprint: Option<String>,
}

fn select_release(releases: Releases) -> Result<String> {
    let release = releases
        .releases
        .into_iter()
        .filter(|release| release.available)
        .max_by(|a, b| a.version.cmp(&b.version))
        .context("Arch publishes no available bootstrap release")?;
    // Versions become URL components. Do not accept arbitrary paths from metadata.
    let version = &release.version;
    if version.len() != 10
        || !version.bytes().enumerate().all(|(i, b)| {
            if i == 4 || i == 7 {
                b == b'.'
            } else {
                b.is_ascii_digit()
            }
        })
    {
        bail!("Invalid Arch release version: {version:?}");
    }
    if release.pgp_fingerprint.as_deref() != Some(SIGNER) {
        bail!(
            "Arch's release signing key has changed. Update ins before installing; expected {SIGNER}"
        );
    }
    Ok(release.version)
}

async fn download(client: &reqwest::Client, url: &str, path: &Path, limit: u64) -> Result<()> {
    let response = client.get(url).send().await?.error_for_status()?;
    let mut stream = response.bytes_stream();
    let mut file = tokio::fs::File::create(path).await?;
    let mut size = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        size += chunk.len() as u64;
        if size > limit {
            bail!("Download exceeds its size limit: {url}");
        }
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    Ok(())
}

pub async fn fetch(directory: &Path) -> Result<std::path::PathBuf> {
    let client = reqwest::Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(900))
        .build()?;
    let metadata = directory.join("releases.json");
    download(&client, RELEASES, &metadata, 1024 * 1024)
        .await
        .context("Could not discover the current Arch bootstrap release")?;
    let version = select_release(serde_json::from_slice(&std::fs::read(metadata)?)?)?;
    println!("Downloading and verifying Arch Linux {version} bootstrap...");
    let name = format!("archlinux-bootstrap-{version}-x86_64.tar.zst");
    let archive = directory.join(&name);
    let signature = directory.join(format!("{name}.sig"));
    download(
        &client,
        &format!("https://geo.mirror.pkgbuild.com/iso/{version}/{name}"),
        &archive,
        1024 * 1024 * 1024,
    )
    .await
    .context("Could not download the Arch bootstrap")?;
    // Fetch the detached signature from Arch itself, independently of the mirror.
    download(
        &client,
        &format!("https://archlinux.org/iso/{version}/{name}.sig"),
        &signature,
        1024 * 1024,
    )
    .await
    .context("Could not download the Arch bootstrap signature")?;
    verify(directory, &archive, &signature)?;
    Ok(archive)
}

fn valid_signer(status: &str) -> bool {
    status.lines().any(|line| {
        let fields: Vec<_> = line.split_whitespace().collect();
        // VALIDSIG fingerprint date timestamp expiry version reserved algo hash class [primary]
        fields.len() >= 11
            && fields[0] == "[GNUPG:]"
            && fields[1] == "VALIDSIG"
            && (fields[2] == SIGNER || fields.get(11) == Some(&SIGNER))
    })
}

pub(super) fn verify(directory: &Path, archive: &Path, signature: &Path) -> Result<()> {
    let home = directory.join("gnupg");
    std::fs::create_dir(&home)?;
    super::private_directory(&home)?;
    let key = directory.join("release-key.asc");
    std::fs::write(&key, KEY)?;
    let gpg = || {
        let mut command = Command::new("gpg");
        command
            .args([
                "--no-options",
                "--batch",
                "--no-auto-key-retrieve",
                "--homedir",
            ])
            .arg(&home);
        command
    };
    super::run(gpg().arg("--import").arg(key))?;
    let output = gpg()
        .args(["--status-fd=1", "--verify"])
        .arg(signature)
        .arg(archive)
        .output()?;
    if !output.status.success() || !valid_signer(&String::from_utf8_lossy(&output.stdout)) {
        bail!(
            "Arch bootstrap signature verification failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "downloads the official Arch bootstrap and requires gpg"]
    fn authenticates_official_bootstrap_and_rejects_tampering() {
        let directory = tempfile::tempdir().unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let archive = runtime.block_on(fetch(directory.path())).unwrap();
        let signature = archive.with_file_name(format!(
            "{}.sig",
            archive.file_name().unwrap().to_str().unwrap()
        ));
        use std::io::Write;
        std::fs::OpenOptions::new()
            .append(true)
            .open(&archive)
            .unwrap()
            .write_all(b"tampered")
            .unwrap();
        let second = tempfile::tempdir().unwrap();
        assert!(verify(second.path(), &archive, &signature).is_err());
    }

    #[test]
    fn release_metadata_cannot_change_the_trust_root_or_paths() {
        let releases = |version: &str, signer: &str| Releases {
            releases: vec![Release {
                version: version.into(),
                available: true,
                pgp_fingerprint: Some(signer.into()),
            }],
        };
        assert_eq!(
            select_release(releases("2026.11.01", SIGNER)).unwrap(),
            "2026.11.01"
        );
        assert!(select_release(releases("../../root", SIGNER)).is_err());
        assert!(select_release(releases("2026.11.01", "untrusted")).is_err());
        assert!(select_release(Releases { releases: vec![] }).is_err());
        let historical: Releases = serde_json::from_value(serde_json::json!({"releases": [
            {"version": "2010.05.01", "available": false, "pgp_fingerprint": null},
            {"version": "2026.11.01", "available": true, "pgp_fingerprint": SIGNER}
        ]}))
        .unwrap();
        assert_eq!(select_release(historical).unwrap(), "2026.11.01");
    }

    #[test]
    fn signature_status_requires_a_full_pinned_fingerprint() {
        assert!(valid_signer(&format!(
            "[GNUPG:] VALIDSIG {SIGNER} 2026-11-01 1 0 4 0 22 8 00 {SIGNER}"
        )));
        assert!(!valid_signer("[GNUPG:] GOODSIG 54449A5C Pierre Schmitz"));
        assert!(!valid_signer(
            "[GNUPG:] VALIDSIG BAD 2026-11-01 1 0 4 0 22 8 00 BAD"
        ));
    }
}
