//! Own the kernel resources created by a bootstrap installation. Mounts are
//! namespace-local; swap, device-mapper mappings and LVM activation are global.
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::{RESOURCES, chroot, is_environment, run, write_json};

/// Kernel resources are global, unlike mounts. Record intent before creating
/// them so cleanup also handles a command interrupted just after success.
#[derive(Debug, Serialize, Deserialize)]
pub enum Resource {
    Swap(String),
    VolumeGroup(String),
    CryptMapping(String),
}

fn load(path: &Path) -> Result<Vec<Resource>> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).context("Invalid bootstrap resource journal"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error).context("Could not read bootstrap resource journal"),
    }
}

pub fn record(resource: Resource) -> Result<()> {
    if !is_environment() {
        return Ok(());
    }
    let mut resources = load(Path::new(RESOURCES))?;
    resources.push(resource);
    write_json(Path::new(RESOURCES), &resources)
}

/// Only these swap devices belong to this bootstrap installation. /proc/swaps
/// also contains the original host's swap, which must not enter the new fstab.
pub fn swap_devices() -> Result<Option<Vec<String>>> {
    if !is_environment() {
        return Ok(None);
    }
    Ok(Some(
        load(Path::new(RESOURCES))?
            .into_iter()
            .filter_map(|resource| match resource {
                Resource::Swap(path) => Some(path),
                _ => None,
            })
            .collect(),
    ))
}

pub fn ensure_encryption_names_available() -> Result<()> {
    if !is_environment() {
        return Ok(());
    }
    let groups = Command::new("vgs")
        .args(["--noheadings", "--options", "vg_name"])
        .output()?;
    if !groups.status.success() {
        bail!("Could not inspect host volume groups before encrypted installation");
    }
    if Path::new("/dev/mapper/cryptlvm").exists()
        || String::from_utf8(groups.stdout)?
            .lines()
            .any(|name| name.trim() == "instantOS")
    {
        bail!(
            "The host already has a cryptlvm mapping or instantOS volume group. Refusing to reuse host encryption resources."
        );
    }
    Ok(())
}

pub(super) fn cleanup(root: &Path) -> Result<()> {
    let target = root.join("mnt");
    let mounted = Command::new("mountpoint")
        .args(["-q", "--"])
        .arg(&target)
        .status()?;
    let mut failures = Vec::new();
    if mounted.success()
        && let Err(error) = run(Command::new("umount").arg("--recursive").arg(&target))
    {
        failures.push(error.to_string());
    }
    let journal = root.join(RESOURCES.trim_start_matches('/'));
    for resource in load(&journal)?.into_iter().rev() {
        let mut command = chroot(root);
        match resource {
            Resource::Swap(path) => {
                // An interrupted swapon might never have activated it.
                let swaps = std::fs::read_to_string("/proc/swaps")?;
                let device = std::fs::canonicalize(root.join(path.trim_start_matches('/'))).ok();
                if !swaps
                    .lines()
                    .filter_map(|line| line.split_whitespace().next())
                    .any(|swap| {
                        std::fs::canonicalize(swap)
                            .ok()
                            .is_some_and(|swap| Some(swap) == device)
                    })
                {
                    continue;
                }
                command.arg("swapoff").arg(path);
            }
            Resource::VolumeGroup(name) => {
                command.args(["vgchange", "-an"]).arg(name);
            }
            Resource::CryptMapping(name) => {
                if !root.join("dev/mapper").join(&name).exists() {
                    continue;
                }
                command.args(["cryptsetup", "close"]).arg(name);
            }
        }
        if let Err(error) = run(&mut command) {
            failures.push(error.to_string());
        }
    }
    if !failures.is_empty() {
        bail!("Bootstrap cleanup failed:\n{}", failures.join("\n"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_update_preserves_the_previous_cleanup_resources() -> Result<()> {
        use serde::ser::{Error, SerializeSeq};

        struct Interrupted<'a>(&'a Path);
        impl Serialize for Interrupted<'_> {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let mut sequence = serializer.serialize_seq(Some(2))?;
                sequence.serialize_element(&Resource::Swap("/dev/target-swap".into()))?;
                // A partially written replacement must never be visible at
                // the journal path, even before the writer reports failure.
                let resources = load(self.0).map_err(S::Error::custom)?;
                assert!(
                    matches!(&resources[..], [Resource::CryptMapping(name)] if name == "cryptlvm")
                );
                Err(S::Error::custom("interrupted registration"))
            }
        }

        let directory = tempfile::tempdir()?;
        let journal = directory.path().join("resources.json");
        assert!(load(&journal)?.is_empty());
        write_json(&journal, &vec![Resource::CryptMapping("cryptlvm".into())])?;
        assert!(write_json(&journal, &Interrupted(&journal)).is_err());
        assert!(
            matches!(&load(&journal)?[..], [Resource::CryptMapping(name)] if name == "cryptlvm")
        );

        let mut resources = load(&journal)?;
        resources.push(Resource::VolumeGroup("instantOS".into()));
        write_json(&journal, &resources)?;
        assert!(matches!(
            &load(&journal)?[..],
            [Resource::CryptMapping(_), Resource::VolumeGroup(_)]
        ));
        Ok(())
    }

    #[test]
    fn an_unreadable_or_invalid_journal_is_never_treated_as_empty() -> Result<()> {
        let directory = tempfile::tempdir()?;
        assert!(load(directory.path()).is_err());
        let journal = directory.path().join("resources.json");
        std::fs::write(&journal, b"[")?;
        assert!(load(&journal).is_err());
        Ok(())
    }
}
