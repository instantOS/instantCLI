//! Bootstrap the instantOS trust root without network access.
//!
//! Resolve GPGDir from the configuration used by pacman. Inside installer
//! chroot re-entry this is the target's configuration and keyring.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use super::CommandRunner;

pub const MASTER_FINGERPRINT: &str = "E5C4740F883910B29694D6BE10DA645A82CF5206";
const SIGNING_FINGERPRINT: &str = "86D0589DC0EC55E4E5DFCE4436276C1C0A17CCD1";
const PUBLIC_KEY: &str = include_str!("../../../resources/instantos-signing-key.asc");

fn gpg_dir(conf: &Path) -> Result<PathBuf> {
    let output = Command::new("pacman-conf")
        .arg("--config")
        .arg(conf)
        .arg("GPGDir")
        .output()
        .context("Could not read pacman's GPGDir")?;
    if !output.status.success() {
        bail!(
            "Could not read pacman's GPGDir: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let path = PathBuf::from(String::from_utf8(output.stdout)?.trim());
    if !path.is_absolute() {
        bail!("Pacman's GPGDir must be an absolute path");
    }
    Ok(path)
}

fn bundle_matches_pin(listing: &str) -> bool {
    let mut primary = Vec::new();
    let mut signing = Vec::new();
    let mut record = "";
    for line in listing.lines() {
        let fields: Vec<_> = line.split(':').collect();
        match fields.first().copied() {
            Some("pub" | "sub" | "sec" | "ssb") => record = fields[0],
            Some("fpr") => match record {
                "pub" => primary.push(fields.get(9).copied().unwrap_or_default()),
                "sub" => signing.push(fields.get(9).copied().unwrap_or_default()),
                _ => return false,
            },
            _ => {}
        }
    }
    primary == [MASTER_FINGERPRINT] && signing.contains(&SIGNING_FINGERPRINT)
}

fn trusted_signing_key(listing: &str) -> bool {
    let mut primary_trusted = false;
    let mut primary_matches = false;
    let mut primary_record = false;
    let mut usable_signer = false;
    for line in listing.lines() {
        let fields: Vec<_> = line.split(':').collect();
        match fields.first().copied() {
            Some("pub") => {
                primary_record = true;
                primary_trusted = matches!(fields.get(1).copied(), Some("f" | "u"));
                primary_matches = false;
            }
            Some("fpr") if primary_record => {
                primary_matches = fields.get(9).copied() == Some(MASTER_FINGERPRINT);
                primary_record = false;
            }
            Some("sub") => {
                if primary_matches
                    && primary_trusted
                    && !matches!(fields.get(1).copied(), Some("r" | "e" | "d" | "i"))
                    && fields
                        .get(11)
                        .is_some_and(|capabilities| capabilities.contains('s'))
                {
                    usable_signer = true;
                }
                primary_record = false;
            }
            _ => {}
        }
    }
    usable_signer
}

/// Check the pinned master's trust and availability of a usable signing subkey.
/// Suppress trustdb writes so Doctor can inspect this as an ordinary user.
pub fn healthy(conf: &Path) -> Result<bool> {
    let homedir = gpg_dir(conf)?;
    if !homedir.is_dir() {
        return Ok(false);
    }
    let output = Command::new("gpg")
        .arg("--homedir")
        .arg(homedir)
        .args([
            "--batch",
            "--lock-never",
            "--no-auto-check-trustdb",
            "--with-colons",
            "--with-subkey-fingerprint",
            "--list-keys",
            MASTER_FINGERPRINT,
        ])
        .output()
        .context("Could not inspect the instantOS signing key")?;
    Ok(output.status.success() && trusted_signing_key(&String::from_utf8(output.stdout)?))
}

/// Import the bundled public key and locally certify its pinned master.
/// Import merges new subkeys without discarding revocations already installed.
pub fn bootstrap(conf: &Path, executor: &dyn CommandRunner) -> Result<()> {
    if executor.dry_run() {
        println!(
            "[DRY RUN] Import and trust bundled instantOS key {MASTER_FINGERPRINT} for {}",
            conf.display()
        );
        return Ok(());
    }
    let homedir = gpg_dir(conf)?;
    let temporary = tempfile::tempdir()?;
    let key_path = temporary.path().join("instantos.asc");
    std::fs::write(&key_path, PUBLIC_KEY)?;
    let output = Command::new("gpg")
        .arg("--homedir")
        .arg(temporary.path())
        .args([
            "--batch",
            "--with-colons",
            "--with-subkey-fingerprint",
            "--show-keys",
        ])
        .arg(&key_path)
        .output()?;
    if !output.status.success() || !bundle_matches_pin(&String::from_utf8(output.stdout)?) {
        bail!("Bundled instantOS public key does not match the pinned fingerprints");
    }
    println!("Preparing the instantOS package signing key...");
    for operation in [
        vec!["--init"],
        vec!["--add"],
        vec!["--lsign-key", MASTER_FINGERPRINT],
        vec!["--updatedb"],
    ] {
        let mut cmd = Command::new("pacman-key");
        cmd.arg("--gpgdir").arg(&homedir).args(&operation);
        if operation == ["--add"] {
            cmd.arg(&key_path);
        }
        executor
            .run(&mut cmd)
            .context("Failed to prepare the instantOS signing key")?;
    }
    if !healthy(conf)? {
        bail!("The instantOS signing key is missing, revoked, expired, or untrusted after import");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listing(trust: &str, sub_trust: &str, fingerprint: &str) -> String {
        format!(
            "pub:{trust}:255:22:master:::::::c:\nfpr:::::::::{fingerprint}:\nsub:{sub_trust}:255:22:signer:::::::s:\nfpr:::::::::{SIGNING_FINGERPRINT}:\n"
        )
    }

    #[test]
    fn doctor_requires_pinned_trusted_master_and_usable_signer() {
        assert!(trusted_signing_key(&listing("f", "f", MASTER_FINGERPRINT)));
        assert!(trusted_signing_key(&listing("u", "u", MASTER_FINGERPRINT)));
        for trust in ["-", "m", "r", "e", "d"] {
            assert!(!trusted_signing_key(&listing(
                trust,
                "f",
                MASTER_FINGERPRINT
            )));
        }
        for trust in ["r", "e", "d", "i"] {
            assert!(!trusted_signing_key(&listing(
                "f",
                trust,
                MASTER_FINGERPRINT
            )));
        }
        assert!(!trusted_signing_key(&listing("f", "f", "WRONG")));
        assert!(!trusted_signing_key(
            &listing("f", "f", MASTER_FINGERPRINT).replace("s:", "e:")
        ));
    }

    #[test]
    fn bundle_rejects_extra_master_or_missing_signing_subkey() {
        let valid = listing("-", "-", MASTER_FINGERPRINT);
        assert!(bundle_matches_pin(&valid));
        assert!(!bundle_matches_pin(
            &(valid.clone() + "pub::::::::::::\nfpr:::::::::OTHER:\n")
        ));
        assert!(!bundle_matches_pin(
            &valid.replace(SIGNING_FINGERPRINT, "OTHER")
        ));
    }

    #[test]
    fn doctor_checks_real_trust_in_the_configured_keyring() {
        let temp = tempfile::tempdir().unwrap();
        let homedir = temp.path().join("target-keyring");
        std::fs::create_dir(&homedir).unwrap();
        let conf = temp.path().join("pacman.conf");
        std::fs::write(
            &conf,
            format!("[options]\nGPGDir = {}\n", homedir.display()),
        )
        .unwrap();
        let key = temp.path().join("public.asc");
        std::fs::write(&key, PUBLIC_KEY).unwrap();
        assert!(!healthy(&conf).unwrap());
        let run_gpg = |args: &[&str]| {
            let result = Command::new("gpg")
                .arg("--homedir")
                .arg(&homedir)
                .args([
                    "--batch",
                    "--yes",
                    "--pinentry-mode",
                    "loopback",
                    "--passphrase",
                    "",
                ])
                .args(args)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        };
        run_gpg(&["--import", key.to_str().unwrap()]);
        assert!(!healthy(&conf).unwrap());
        run_gpg(&[
            "--quick-generate-key",
            "fixture@example.invalid",
            "ed25519",
            "cert",
            "0",
        ]);
        run_gpg(&["--quick-lsign-key", MASTER_FINGERPRINT]);
        run_gpg(&["--check-trustdb"]);
        assert!(healthy(&conf).unwrap());
    }

    #[test]
    fn bundled_public_key_matches_fingerprints() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("public.asc");
        std::fs::write(&file, PUBLIC_KEY).unwrap();
        let result = Command::new("gpg")
            .arg("--homedir")
            .arg(temp.path())
            .args([
                "--batch",
                "--with-colons",
                "--with-subkey-fingerprint",
                "--show-keys",
            ])
            .arg(file)
            .output()
            .unwrap();
        assert!(result.status.success());
        assert!(bundle_matches_pin(
            &String::from_utf8(result.stdout).unwrap()
        ));
    }
}
