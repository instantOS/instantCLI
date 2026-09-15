use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::arch::engine::{InstallContext, InstallPlan, StepId, StoragePlan};
use crate::common::blockdev::{BlockDevice, load_lsblk};
use crate::common::commands::run_status;

const IDENTITY_SCHEMA_VERSION: u32 = 1;
pub const INSTALLATION_MARKER: &str = "/etc/instant/installation.toml";

#[derive(Debug, Serialize)]
struct InstallationIntent {
    schema_version: u32,
    answers: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InstallationMarker {
    pub schema_version: u32,
    pub intent_sha256: String,
    pub installed_at: DateTime<Utc>,
    pub installer_version: String,
}

#[derive(Debug)]
pub struct MatchingInstallation {
    pub root_device: String,
    pub installed_at: DateTime<Utc>,
}

/// Produce a stable identity for the requested installed system.
///
/// Secrets are deliberately excluded: persisting a digest of the complete
/// questions file would provide an offline password verifier. Wizard control
/// answers and warnings are excluded because they do not affect the target.
pub fn fingerprint(context: &InstallContext) -> Result<String> {
    let answers = context
        .answers()
        .filter(|(id, _)| answer_affects_installation(**id))
        .map(|(id, answer)| (format!("{id:?}"), answer.clone()))
        .collect();

    let intent = InstallationIntent {
        schema_version: IDENTITY_SCHEMA_VERSION,
        answers,
    };
    let bytes = toml::to_string(&intent).context("Failed to serialize installation identity")?;
    Ok(hex::encode(Sha256::digest(bytes.as_bytes())))
}

/// Bind resumable execution state to the exact configuration, including
/// secrets. This digest never belongs in the completed target marker.
pub fn configuration_fingerprint(configuration: &str) -> String {
    hex::encode(Sha256::digest(configuration.as_bytes()))
}

fn answer_affects_installation(id: StepId) -> bool {
    !matches!(
        id,
        StepId::Password
            | StepId::EncryptionPassword
            | StepId::PrepareDisk
            | StepId::RunCfdisk
            | StepId::ConfirmInstall
            | StepId::LogUpload
            | StepId::VirtualBoxWarning
            | StepId::WeakPasswordWarning
            | StepId::LowRamWarning
            | StepId::DualBootEspWarning
    )
}

pub fn find_matching_installation(
    plan: &InstallPlan,
    intent_sha256: &str,
) -> Result<Option<MatchingInstallation>> {
    let disk_path = plan.storage.disk().as_str();
    let tree = load_lsblk(&[disk_path])?;

    for device in tree.blockdevices.iter().flat_map(linux_filesystems) {
        if let Some(found) = probe_marker(&device.path(), device.fstype.as_deref(), intent_sha256)?
        {
            return Ok(Some(found));
        }
    }

    if let StoragePlan::Automatic {
        encryption: Some(encryption),
        ..
    } = &plan.storage
    {
        return probe_encrypted_installation(plan, encryption.password.expose(), intent_sha256);
    }

    Ok(None)
}

fn linux_filesystems(device: &BlockDevice) -> Vec<&BlockDevice> {
    fn collect<'a>(device: &'a BlockDevice, found: &mut Vec<&'a BlockDevice>) {
        if device.is_linux_root_fs() {
            found.push(device);
        }
        for child in &device.children {
            collect(child, found);
        }
    }

    let mut found = Vec::new();
    collect(device, &mut found);
    found
}

fn probe_marker(
    device: &str,
    fs_type: Option<&str>,
    intent_sha256: &str,
) -> Result<Option<MatchingInstallation>> {
    if !Path::new(device).exists() {
        return Ok(None);
    }

    let tempdir = tempfile::Builder::new()
        .prefix("ins-install-check-")
        .tempdir()
        .context("Failed to create installation probe mountpoint")?;

    let mount_options = match fs_type.map(str::to_ascii_lowercase).as_deref() {
        Some("btrfs") => vec![
            format!("ro,subvol={}", crate::arch::config::BTRFS_ROOT_SUBVOLUME),
            "ro".to_string(),
        ],
        Some("ext3" | "ext4") => vec!["ro,noload".to_string()],
        Some("xfs") => vec!["ro,norecovery".to_string()],
        _ => vec!["ro".to_string()],
    };

    for options in mount_options {
        let mut mount = Command::new("mount");
        mount.args(["-o", &options]).arg(device).arg(tempdir.path());
        if run_status(&mut mount).is_err() {
            continue;
        }

        let marker_path = tempdir
            .path()
            .join(INSTALLATION_MARKER.trim_start_matches('/'));
        let marker = read_marker(&marker_path);
        run_status(Command::new("umount").arg(tempdir.path()))
            .with_context(|| format!("Failed to unmount installation probe for {device}"))?;

        if let Some(marker) = marker? {
            if marker.schema_version == IDENTITY_SCHEMA_VERSION
                && marker.intent_sha256 == intent_sha256
            {
                return Ok(Some(MatchingInstallation {
                    root_device: device.to_string(),
                    installed_at: marker.installed_at,
                }));
            }
            return Ok(None);
        }
    }

    Ok(None)
}

fn read_marker(path: &Path) -> Result<Option<InstallationMarker>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read installation marker {}", path.display()))?;
    let marker = toml::from_str(&content)
        .with_context(|| format!("Invalid installation marker {}", path.display()))?;
    Ok(Some(marker))
}

fn probe_encrypted_installation(
    plan: &InstallPlan,
    password: &str,
    intent_sha256: &str,
) -> Result<Option<MatchingInstallation>> {
    let disk = plan.storage.disk().as_str();
    let luks_partition = crate::arch::execution::disk::get_part_path(disk, 2);
    if !Path::new(&luks_partition).exists() {
        return Ok(None);
    }

    let mapper_name = format!("ins-install-check-{}", std::process::id());
    let mapper_path = format!("/dev/mapper/{mapper_name}");
    if Path::new(&mapper_path).exists() {
        bail!("Temporary encryption mapper already exists: {mapper_path}");
    }

    if !open_luks_read_only(&luks_partition, &mapper_name, password)? {
        return Ok(None);
    }

    let root_was_active = Path::new("/dev/instantOS/root").exists();
    let activation = if root_was_active {
        Ok(())
    } else {
        run_status(Command::new("vgchange").args(["-ay", "instantOS"]))
    };

    let result = (|| -> Result<Option<MatchingInstallation>> {
        if activation.is_err() {
            return Ok(None);
        }
        let fs_type = blkid_type("/dev/instantOS/root")?;
        probe_marker("/dev/instantOS/root", fs_type.as_deref(), intent_sha256)
    })();

    if !root_was_active {
        let _ = run_status(Command::new("vgchange").args(["-an", "instantOS"]));
    }
    run_status(Command::new("cryptsetup").args(["close", &mapper_name]))
        .context("Failed to close temporary encryption mapper")?;
    result
}

fn open_luks_read_only(device: &str, mapper_name: &str, password: &str) -> Result<bool> {
    let mut child = Command::new("cryptsetup")
        .args(["open", "--readonly", device, mapper_name, "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to start cryptsetup for installation check")?;

    child
        .stdin
        .as_mut()
        .context("Failed to open cryptsetup input")?
        .write_all(password.as_bytes())
        .context("Failed to provide encryption password")?;
    let output = child
        .wait_with_output()
        .context("Failed to wait for cryptsetup")?;
    Ok(output.status.success())
}

fn blkid_type(device: &str) -> Result<Option<String>> {
    let output = Command::new("blkid")
        .args(["-o", "value", "-s", "TYPE", device])
        .output()
        .context("Failed to inspect root filesystem type")?;
    if !output.status.success() {
        return Ok(None);
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!value.is_empty()).then_some(value))
}

pub fn write_completed_marker(root: &Path, intent_sha256: &str) -> Result<PathBuf> {
    let path = root.join(INSTALLATION_MARKER.trim_start_matches('/'));
    let parent = path.parent().context("Installation marker has no parent")?;
    fs::create_dir_all(parent)?;

    let marker = InstallationMarker {
        schema_version: IDENTITY_SCHEMA_VERSION,
        intent_sha256: intent_sha256.to_string(),
        installed_at: Utc::now(),
        installer_version: env!("CARGO_PKG_VERSION").to_string(),
    };
    let content = toml::to_string_pretty(&marker)?;
    let temporary = path.with_extension("toml.tmp");
    let mut file = fs::File::create(&temporary)?;
    file.write_all(content.as_bytes())?;
    file.sync_all()?;
    fs::rename(&temporary, &path)?;
    fs::File::open(parent)?.sync_all()?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arch::engine::{BootMode, GpuKind, SystemInfo};

    fn context_with_answers(answers: &[(StepId, &str)]) -> InstallContext {
        let mut context = InstallContext::new();
        context.system_info = SystemInfo {
            boot_mode: BootMode::UEFI64,
            has_intel_cpu: true,
            gpus: vec![GpuKind::Intel, GpuKind::Nvidia],
            architecture: "x86_64".to_string(),
            ..SystemInfo::default()
        };
        for (id, answer) in answers {
            context.set_answer(*id, (*answer).to_string());
        }
        context
    }

    #[test]
    fn fingerprint_is_independent_of_answer_and_gpu_order() {
        let first =
            context_with_answers(&[(StepId::Hostname, "instant"), (StepId::Kernel, "linux")]);
        let mut second =
            context_with_answers(&[(StepId::Kernel, "linux"), (StepId::Hostname, "instant")]);
        second.system_info.gpus.reverse();

        assert_eq!(fingerprint(&first).unwrap(), fingerprint(&second).unwrap());
    }

    #[test]
    fn fingerprint_excludes_secrets_and_volatile_checks() {
        let mut first = context_with_answers(&[(StepId::Hostname, "instant")]);
        first.set_answer(StepId::Password, "first-password".to_string());
        first.system_info.internet_connected = true;
        first.system_info.total_ram_gb = Some(16);

        let mut second = context_with_answers(&[(StepId::Hostname, "instant")]);
        second.set_answer(StepId::Password, "second-password".to_string());
        second.system_info.internet_connected = false;
        second.system_info.total_ram_gb = Some(15);

        assert_eq!(fingerprint(&first).unwrap(), fingerprint(&second).unwrap());
    }

    #[test]
    fn resumable_state_fingerprint_includes_secrets() {
        assert_ne!(
            configuration_fingerprint("Password = 'first'"),
            configuration_fingerprint("Password = 'second'")
        );
    }

    #[test]
    fn fingerprint_changes_for_installed_configuration() {
        let first = context_with_answers(&[(StepId::Hostname, "first")]);
        let second = context_with_answers(&[(StepId::Hostname, "second")]);

        assert_ne!(fingerprint(&first).unwrap(), fingerprint(&second).unwrap());
    }

    #[test]
    fn completed_marker_round_trips() {
        let root = tempfile::tempdir().unwrap();
        let path = write_completed_marker(root.path(), "abc123").unwrap();
        let marker = read_marker(&path).unwrap().unwrap();

        assert_eq!(marker.schema_version, IDENTITY_SCHEMA_VERSION);
        assert_eq!(marker.intent_sha256, "abc123");
        assert_eq!(marker.installer_version, env!("CARGO_PKG_VERSION"));
    }
}
