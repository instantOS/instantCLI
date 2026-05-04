use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use clap::Args;

use crate::common::blockdev::{BlockDevice, load_lsblk};
use crate::common::commands::{ensure_commands, run_interactive_status, run_status};
use crate::menu_utils::{ConfirmResult, FzfPreview, FzfResult, FzfSelectable, FzfWrapper, Header};
use crate::ui::catppuccin::{colors, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

const DEFAULT_MOUNTPOINT: &str = "/mnt/instantos";
const DEFAULT_SHELL: &str = "/bin/bash";
const ROOT_MARKER: &str = "/etc/os-release";

#[derive(Args, Debug, Clone)]
pub struct ChrootOptions {
    /// Restrict scanning to one physical disk, e.g. /dev/nvme0n1
    #[arg(long)]
    pub disk: Option<String>,

    /// Bypass root detection and try this root device directly
    #[arg(long)]
    pub root: Option<String>,

    /// Mountpoint for the installed system
    #[arg(long, default_value = DEFAULT_MOUNTPOINT)]
    pub mountpoint: PathBuf,

    /// Shell to start inside the chroot
    #[arg(long, default_value = DEFAULT_SHELL)]
    pub shell: String,

    /// Leave mounts and opened LUKS mappings active after the chroot exits
    #[arg(long)]
    pub keep_mounted: bool,
}

#[derive(Debug, Clone)]
struct ChrootCandidate {
    root_device: String,
    disk: Option<String>,
    fs_type: Option<String>,
    size_bytes: Option<u64>,
    encrypted: bool,
    evidence: Vec<String>,
    opened_mappers: Vec<String>,
    boot_device: Option<String>,
}

impl std::fmt::Display for ChrootCandidate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.root_device)
    }
}

impl FzfSelectable for ChrootCandidate {
    fn fzf_display_text(&self) -> String {
        let icon = if self.encrypted {
            NerdFont::Lock
        } else {
            NerdFont::HardDrive
        };
        let disk = self.disk.as_deref().unwrap_or("unknown disk");
        let fs = self.fs_type.as_deref().unwrap_or("unknown fs");
        let size = self
            .size_bytes
            .map(crate::arch::dualboot::format_size)
            .unwrap_or_else(|| "unknown size".to_string());

        format!(
            "{} {}  {}  {}  {}",
            format_icon_colored(icon, colors::SAPPHIRE),
            self.root_device,
            disk,
            fs,
            size
        )
    }

    fn fzf_preview(&self) -> FzfPreview {
        let mut preview = PreviewBuilder::new()
            .header(NerdFont::Terminal, "instantOS chroot candidate")
            .field("Root", &self.root_device)
            .field("Disk", self.disk.as_deref().unwrap_or("unknown"))
            .field("Filesystem", self.fs_type.as_deref().unwrap_or("unknown"))
            .field(
                "Size",
                &self
                    .size_bytes
                    .map(crate::arch::dualboot::format_size)
                    .unwrap_or_else(|| "unknown".to_string()),
            )
            .field("Encrypted", if self.encrypted { "yes" } else { "no" });

        if let Some(boot) = &self.boot_device {
            preview = preview.field("Boot", boot);
        }

        preview = preview.blank().text("Verification evidence:");
        for item in &self.evidence {
            preview = preview.bullet(item);
        }
        preview.build()
    }
}

#[derive(Debug, Default)]
struct MountSession {
    mountpoint: PathBuf,
    mounted_paths: Vec<PathBuf>,
    opened_mappers: Vec<String>,
    keep_mounted: bool,
}

impl MountSession {
    fn new(mountpoint: PathBuf, keep_mounted: bool) -> Self {
        Self {
            mountpoint,
            mounted_paths: Vec::new(),
            opened_mappers: Vec::new(),
            keep_mounted,
        }
    }

    fn record_mount(&mut self, path: PathBuf) {
        self.mounted_paths.push(path);
    }

    fn record_opened_mappers(&mut self, mappers: impl IntoIterator<Item = String>) {
        self.opened_mappers.extend(mappers);
    }

    fn cleanup(&mut self) {
        if self.keep_mounted {
            println!("Leaving {} mounted by request.", self.mountpoint.display());
            return;
        }

        for mount in self.mounted_paths.iter().rev() {
            if let Err(err) = run_status(Command::new("umount").arg(mount)) {
                eprintln!(
                    "Warning: failed to unmount {}: {err}. Inspect with: findmnt {}",
                    mount.display(),
                    mount.display()
                );
            }
        }

        for mapper in self.opened_mappers.iter().rev() {
            if let Err(err) = run_status(Command::new("cryptsetup").arg("close").arg(mapper)) {
                eprintln!(
                    "Warning: failed to close {mapper}: {err}. Try manually: cryptsetup close {mapper}"
                );
            }
        }
    }
}

impl Drop for MountSession {
    fn drop(&mut self) {
        self.cleanup();
    }
}

pub fn handle_chroot(options: ChrootOptions, debug: bool) -> Result<()> {
    ensure_commands(&["lsblk", "findmnt", "mount", "umount", "arch-chroot"])?;
    ensure_mountpoint_available(&options.mountpoint)?;

    let mut candidate = if let Some(root) = options.root.as_deref() {
        candidate_from_root(root, options.disk.as_deref())?
    } else {
        let candidates = scan_candidates(options.disk.as_deref(), debug)?;
        select_candidate(candidates)?
    };

    if candidate.encrypted || !candidate.opened_mappers.is_empty() {
        ensure_commands(&["cryptsetup"])?;
    }

    println!("Selected root: {}", candidate.root_device);
    let mut session = MountSession::new(options.mountpoint.clone(), options.keep_mounted);
    session.record_opened_mappers(candidate.opened_mappers.drain(..));

    mount_target(&candidate, &mut session)?;
    let verification = verify_instantos_root(&options.mountpoint)?;
    if !verification.is_instantos {
        if options.root.is_some() && confirm_unverified_root(&options.mountpoint)? {
            eprintln!(
                "Warning: continuing into an unverified root at {}",
                options.mountpoint.display()
            );
        } else {
            bail!(
                "{} does not look like an instantOS installation",
                options.mountpoint.display()
            );
        }
    }

    mount_boot_if_present(&candidate, &options.mountpoint, &mut session)?;

    println!(
        "Entering chroot at {}. Exit the shell to return.",
        options.mountpoint.display()
    );
    run_interactive_status(
        Command::new("arch-chroot")
            .arg(&options.mountpoint)
            .arg(&options.shell)
            .arg("-l"),
    )
    .with_context(|| format!("Failed to chroot into {}", options.mountpoint.display()))?;

    Ok(())
}

fn candidate_from_root(root: &str, disk: Option<&str>) -> Result<ChrootCandidate> {
    if !Path::new(root).exists() {
        bail!("Root device does not exist: {root}");
    }

    Ok(ChrootCandidate {
        root_device: root.to_string(),
        disk: disk.map(str::to_string),
        fs_type: blkid_type(root).ok().flatten(),
        size_bytes: None,
        encrypted: false,
        evidence: vec!["root device provided explicitly".to_string()],
        opened_mappers: Vec::new(),
        boot_device: None,
    })
}

fn scan_candidates(disk_filter: Option<&str>, debug: bool) -> Result<Vec<ChrootCandidate>> {
    let mut tree = load_lsblk()?;
    if let Some(disk) = disk_filter {
        tree.blockdevices.retain(|device| {
            device.path() == disk || device.name == disk.trim_start_matches("/dev/")
        });
    } else if let Ok(Some(boot_disk)) = crate::arch::disks::get_boot_disk() {
        tree.blockdevices
            .retain(|device| device.path() != boot_disk);
    }

    let mut candidates = Vec::new();
    let mut seen = HashSet::new();

    for disk in tree.blockdevices.iter().filter(|device| device.is_disk()) {
        if disk.name.starts_with("loop") || disk.name.starts_with("sr") {
            continue;
        }

        for candidate in scan_disk_for_candidates(disk, debug)? {
            if seen.insert(candidate.root_device.clone()) {
                candidates.push(candidate);
            }
        }
    }

    if candidates.is_empty() {
        bail!("No instantOS installation candidates found");
    }

    Ok(candidates)
}

fn scan_disk_for_candidates(disk: &BlockDevice, debug: bool) -> Result<Vec<ChrootCandidate>> {
    let mut candidates = Vec::new();
    let boot_device = find_boot_device(disk);

    for child in &disk.children {
        if child.is_partition() && child.is_linux_root_fs() {
            let root = child.path();
            if let Some(evidence) = probe_instantos_device(&root, debug)? {
                candidates.push(ChrootCandidate {
                    root_device: root,
                    disk: Some(disk.path()),
                    fs_type: child.fstype.clone(),
                    size_bytes: child.size,
                    encrypted: false,
                    evidence,
                    opened_mappers: Vec::new(),
                    boot_device: boot_device.clone(),
                });
            }
        }

        if child.is_luks() {
            candidates.extend(scan_luks_partition(
                disk,
                child,
                boot_device.clone(),
                debug,
            )?);
        }
    }

    Ok(candidates)
}

fn scan_luks_partition(
    disk: &BlockDevice,
    luks: &BlockDevice,
    boot_device: Option<String>,
    debug: bool,
) -> Result<Vec<ChrootCandidate>> {
    ensure_commands(&["cryptsetup"])?;
    let luks_path = luks.path();
    let mapper_name = mapper_name_for_luks(&luks.name);
    let mapper_path = format!("/dev/mapper/{mapper_name}");
    let mut opened_mappers = Vec::new();

    if !Path::new(&mapper_path).exists() {
        let password = match FzfWrapper::password(&format!("Password for {luks_path}:"))? {
            FzfResult::Selected(password) => password,
            FzfResult::Cancelled => return Ok(Vec::new()),
            FzfResult::Error(err) => bail!(err),
            FzfResult::MultiSelected(_) => return Ok(Vec::new()),
        };

        open_luks(&luks_path, &mapper_name, &password)
            .with_context(|| format!("Failed to unlock {luks_path}"))?;
        opened_mappers.push(mapper_name);
    }

    let _ = run_status(Command::new("vgchange").arg("-ay"));
    let tree = load_lsblk()?;
    let roots = find_linux_children_for_path(&tree.blockdevices, &mapper_path);
    let mut candidates = Vec::new();

    for root in roots {
        if let Some(evidence) = probe_instantos_device(&root.path(), debug)? {
            candidates.push(ChrootCandidate {
                root_device: root.path(),
                disk: Some(disk.path()),
                fs_type: root.fstype.clone(),
                size_bytes: root.size,
                encrypted: true,
                evidence,
                opened_mappers: opened_mappers.clone(),
                boot_device: boot_device.clone(),
            });
        }
    }

    if candidates.is_empty() {
        for mapper in opened_mappers.iter().rev() {
            let _ = run_status(Command::new("cryptsetup").arg("close").arg(mapper));
        }
    }

    Ok(candidates)
}

fn select_candidate(candidates: Vec<ChrootCandidate>) -> Result<ChrootCandidate> {
    if candidates.len() == 1 {
        return Ok(candidates.into_iter().next().expect("candidate exists"));
    }

    let result = FzfWrapper::builder()
        .header(Header::fancy("Select instantOS installation"))
        .prompt("Chroot")
        .select(candidates.clone())?;

    match result {
        FzfResult::Selected(candidate) => {
            close_unselected_mappers(&candidates, &candidate);
            Ok(candidate)
        }
        FzfResult::Cancelled => {
            close_all_candidate_mappers(&candidates);
            bail!("No installation selected")
        }
        FzfResult::Error(err) => {
            close_all_candidate_mappers(&candidates);
            bail!(err)
        }
        FzfResult::MultiSelected(_) => {
            close_all_candidate_mappers(&candidates);
            bail!("Unexpected multi-select result")
        }
    }
}

fn close_unselected_mappers(candidates: &[ChrootCandidate], selected: &ChrootCandidate) {
    let selected_mappers = selected
        .opened_mappers
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    let mappers = candidates
        .iter()
        .flat_map(|candidate| candidate.opened_mappers.iter().cloned())
        .filter(|mapper| !selected_mappers.contains(mapper))
        .collect::<HashSet<_>>();

    close_mappers(mappers);
}

fn close_all_candidate_mappers(candidates: &[ChrootCandidate]) {
    let mappers = candidates
        .iter()
        .flat_map(|candidate| candidate.opened_mappers.iter().cloned())
        .collect::<HashSet<_>>();

    close_mappers(mappers);
}

fn close_mappers(mappers: HashSet<String>) {
    for mapper in mappers {
        if let Err(err) = run_status(Command::new("cryptsetup").arg("close").arg(&mapper)) {
            eprintln!("Warning: failed to close unused mapper {mapper}: {err}");
        }
    }
}

fn mount_target(candidate: &ChrootCandidate, session: &mut MountSession) -> Result<()> {
    fs::create_dir_all(&session.mountpoint)
        .with_context(|| format!("Failed to create {}", session.mountpoint.display()))?;
    run_status(
        Command::new("mount")
            .arg(&candidate.root_device)
            .arg(&session.mountpoint),
    )
    .with_context(|| format!("Failed to mount {}", candidate.root_device))?;
    session.record_mount(session.mountpoint.clone());
    Ok(())
}

fn mount_boot_if_present(
    candidate: &ChrootCandidate,
    mountpoint: &Path,
    session: &mut MountSession,
) -> Result<()> {
    let Some(boot_device) = &candidate.boot_device else {
        return Ok(());
    };

    let boot_relative = choose_boot_mount_relative(mountpoint);
    let target = mountpoint.join(boot_relative);
    fs::create_dir_all(&target)
        .with_context(|| format!("Failed to create {}", target.display()))?;
    run_status(Command::new("mount").arg(boot_device).arg(&target))
        .with_context(|| format!("Failed to mount boot partition {boot_device}"))?;
    session.record_mount(target);
    Ok(())
}

fn choose_boot_mount_relative(root: &Path) -> &'static str {
    let fstab = root.join("etc/fstab");
    if let Ok(content) = fs::read_to_string(fstab)
        && content.lines().any(|line| {
            let trimmed = line.trim();
            !trimmed.starts_with('#') && trimmed.split_whitespace().nth(1) == Some("/boot/efi")
        })
    {
        return "boot/efi";
    }

    if root.join("boot/efi").exists() {
        "boot/efi"
    } else {
        "boot"
    }
}

fn probe_instantos_device(device: &str, debug: bool) -> Result<Option<Vec<String>>> {
    let tempdir = tempfile::Builder::new()
        .prefix("ins-dev-chroot-")
        .tempdir()
        .context("Failed to create temporary mountpoint")?;

    if debug {
        eprintln!("Probing {device}");
    }

    if let Err(err) = run_status(
        Command::new("mount")
            .arg("-o")
            .arg("ro")
            .arg(device)
            .arg(tempdir.path()),
    ) {
        if debug {
            eprintln!("Skipping {device}: {err}");
        }
        return Ok(None);
    }

    let verification = verify_instantos_root(tempdir.path())?;
    let unmount_result = run_status(Command::new("umount").arg(tempdir.path()));
    if let Err(err) = unmount_result {
        eprintln!(
            "Warning: failed to unmount probe mount {}: {err}",
            tempdir.path().display()
        );
    }

    if verification.is_instantos {
        Ok(Some(verification.evidence))
    } else {
        Ok(None)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RootVerification {
    is_instantos: bool,
    evidence: Vec<String>,
}

fn verify_instantos_root(root: &Path) -> Result<RootVerification> {
    let mut evidence = Vec::new();
    let os_release = root.join(ROOT_MARKER.trim_start_matches('/'));
    let mut is_instantos = false;

    if let Ok(content) = fs::read_to_string(&os_release) {
        if os_release_has_id(&content, "instantos") {
            is_instantos = true;
            evidence.push("ID=instantos in /etc/os-release".to_string());
        }
    }

    for (path, label) in [
        ("etc/instant", "/etc/instant exists"),
        ("usr/bin/ins", "/usr/bin/ins exists"),
        ("usr/bin/instantwm", "/usr/bin/instantwm exists"),
    ] {
        if root.join(path).exists() {
            evidence.push(label.to_string());
        }
    }

    if evidence.is_empty() {
        evidence.push("no instantOS markers found".to_string());
    }

    Ok(RootVerification {
        is_instantos,
        evidence,
    })
}

fn os_release_has_id(content: &str, expected: &str) -> bool {
    content.lines().any(|line| {
        let Some(value) = line.strip_prefix("ID=") else {
            return false;
        };
        value.trim().trim_matches('"') == expected
    })
}

fn confirm_unverified_root(mountpoint: &Path) -> Result<bool> {
    let result = FzfWrapper::builder()
        .confirm(format!(
            "{} is mounted but does not verify as instantOS.\n\nContinue anyway?",
            mountpoint.display()
        ))
        .yes_text("Chroot Anyway")
        .no_text("Abort")
        .confirm_dialog()?;

    Ok(matches!(result, ConfirmResult::Yes))
}

fn ensure_mountpoint_available(mountpoint: &Path) -> Result<()> {
    let output = Command::new("findmnt")
        .arg("-R")
        .arg(mountpoint)
        .output()
        .context("Failed to run findmnt")?;

    if output.status.success() {
        bail!(
            "{} is already mounted. Use --mountpoint or unmount it first.",
            mountpoint.display()
        );
    }

    Ok(())
}

fn open_luks(device: &str, mapper_name: &str, password: &str) -> Result<()> {
    let mut child = Command::new("cryptsetup")
        .arg("open")
        .arg(device)
        .arg(mapper_name)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to spawn cryptsetup")?;

    {
        use std::io::Write;
        let stdin = child
            .stdin
            .as_mut()
            .context("Failed to open cryptsetup stdin")?;
        stdin
            .write_all(password.as_bytes())
            .context("Failed to send password to cryptsetup")?;
    }

    let output = child.wait_with_output().context("cryptsetup failed")?;
    if !output.status.success() {
        bail!(
            "cryptsetup failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(())
}

fn mapper_name_for_luks(name: &str) -> String {
    let sanitized = name
        .trim_start_matches("/dev/")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>();
    format!("ins-dev-{sanitized}")
}

fn blkid_type(device: &str) -> Result<Option<String>> {
    let output = Command::new("blkid")
        .args(["-o", "value", "-s", "TYPE", device])
        .output()
        .context("Failed to run blkid")?;

    if !output.status.success() {
        return Ok(None);
    }

    let fs = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!fs.is_empty()).then_some(fs))
}

fn find_boot_device(disk: &BlockDevice) -> Option<String> {
    disk.children
        .iter()
        .find(|child| child.is_partition() && child.is_efi())
        .map(BlockDevice::path)
        .or_else(|| {
            disk.children
                .iter()
                .find(|child| {
                    child.is_partition()
                        && child
                            .fstype
                            .as_deref()
                            .is_some_and(|fs| fs.eq_ignore_ascii_case("ext4"))
                        && child.size.unwrap_or(0) <= 2 * 1024 * 1024 * 1024
                })
                .map(BlockDevice::path)
        })
}

fn find_linux_children_for_path(devices: &[BlockDevice], path: &str) -> Vec<BlockDevice> {
    let mut results = Vec::new();
    for device in devices {
        collect_linux_children_for_path(device, path, false, &mut results);
    }
    results
}

fn collect_linux_children_for_path(
    device: &BlockDevice,
    path: &str,
    under_target: bool,
    results: &mut Vec<BlockDevice>,
) {
    let is_target = device.path() == path;
    let now_under_target = under_target || is_target;

    if now_under_target && !is_target && device.is_linux_root_fs() {
        results.push(device.clone());
    }

    for child in &device.children {
        collect_linux_children_for_path(child, path, now_under_target, results);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::blockdev::LsblkOutput;
    use tempfile::TempDir;

    fn parse_tree(json: &str) -> LsblkOutput {
        serde_json::from_str(json).expect("valid lsblk json")
    }

    #[test]
    fn parses_plaintext_root_candidates_from_lsblk() {
        let tree = parse_tree(
            r#"{
              "blockdevices": [{
                "name": "sda", "type": "disk", "size": 1000,
                "children": [
                  {"name": "sda1", "type": "part", "size": 100, "fstype": "vfat", "parttype": "c12a7328-f81f-11d2-ba4b-00a0c93ec93b"},
                  {"name": "sda2", "type": "part", "size": 900, "fstype": "ext4"}
                ]
              }]
            }"#,
        );

        let disk = &tree.blockdevices[0];
        assert_eq!(find_boot_device(disk).as_deref(), Some("/dev/sda1"));
        assert!(disk.children[1].is_linux_root_fs());
    }

    #[test]
    fn finds_lvm_root_below_luks_mapper() {
        let tree = parse_tree(
            r#"{
              "blockdevices": [{
                "name": "nvme0n1", "type": "disk",
                "children": [{
                  "name": "nvme0n1p2", "type": "part", "fstype": "crypto_LUKS",
                  "children": [{
                    "name": "mapper/ins-dev-nvme0n1p2", "type": "crypt",
                    "children": [{
                      "name": "mapper/instantOS-root", "type": "lvm", "fstype": "ext4", "size": 123
                    }]
                  }]
                }]
              }]
            }"#,
        );

        let roots =
            find_linux_children_for_path(&tree.blockdevices, "/dev/mapper/ins-dev-nvme0n1p2");
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].path(), "/dev/mapper/instantOS-root");
    }

    #[test]
    fn verifies_instantos_os_release() {
        let temp = TempDir::new().unwrap();
        fs::create_dir_all(temp.path().join("etc")).unwrap();
        fs::write(
            temp.path().join("etc/os-release"),
            "NAME=\"instantOS\"\nID=instantos\n",
        )
        .unwrap();

        let result = verify_instantos_root(temp.path()).unwrap();
        assert!(result.is_instantos);
        assert!(
            result
                .evidence
                .contains(&"ID=instantos in /etc/os-release".to_string())
        );
    }

    #[test]
    fn rejects_non_instantos_os_release() {
        let temp = TempDir::new().unwrap();
        fs::create_dir_all(temp.path().join("etc")).unwrap();
        fs::write(temp.path().join("etc/os-release"), "ID=arch\n").unwrap();

        let result = verify_instantos_root(temp.path()).unwrap();
        assert!(!result.is_instantos);
    }

    #[test]
    fn chooses_boot_efi_when_fstab_uses_it() {
        let temp = TempDir::new().unwrap();
        fs::create_dir_all(temp.path().join("etc")).unwrap();
        fs::write(
            temp.path().join("etc/fstab"),
            "UUID=abc /boot/efi vfat defaults 0 2\n",
        )
        .unwrap();

        assert_eq!(choose_boot_mount_relative(temp.path()), "boot/efi");
    }
}
