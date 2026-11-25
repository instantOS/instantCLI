use super::CommandRunner;
use crate::arch::arch_linux_variant::{ArchLinuxVariant, ArtixInitSystem};
use crate::arch::engine::InstallPlan;
use crate::arch::engine::system_configuration::{InitramfsConfiguration, PlymouthTheme};
use crate::arch::execution::pacman::Pacman;
use crate::arch::mkinitcpio::MkinitcpioConfig;
use crate::common::config_edit::set_keys;
use crate::common::locale_gen::apply_enable_disable;
use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// Groups that users should be added to for normal desktop usage
pub const USER_GROUPS: &[&str] = &["wheel", "video", "docker", "sys", "rfkill"];

/// System groups that need to exist but users shouldn't be members of
pub const SYSTEM_GROUPS: &[&str] = &["nobody"];

/// Ensure the user and system groups exist.
/// This creates both user groups (that users should be members of) and
/// system groups (that need to exist but users shouldn't be members of).
///
/// This is called by both `configure_users` during fresh install and
/// `setup_instantos` during `ins arch setup`.
pub fn ensure_groups_exist(executor: &dyn CommandRunner) -> Result<()> {
    // Ensure user groups exist
    for group in USER_GROUPS {
        let mut cmd = Command::new("groupadd");
        cmd.arg("-f").arg(group);
        executor.run(&mut cmd)?;
    }

    // Ensure system groups exist (these are not for user membership)
    for group in SYSTEM_GROUPS {
        let mut cmd = Command::new("groupadd");
        cmd.arg("-f").arg(group);
        executor.run(&mut cmd)?;
    }

    Ok(())
}

/// Add an existing user to the standard user groups.
///
/// This is used by `ins arch setup` to add an existing user to the required groups.
pub fn add_user_to_groups(username: &str, executor: &dyn CommandRunner) -> Result<()> {
    println!(
        "Adding user {} to groups: {}",
        username,
        USER_GROUPS.join(", ")
    );
    let mut cmd = Command::new("usermod");
    cmd.arg("-aG").arg(USER_GROUPS.join(",")).arg(username);
    executor.run(&mut cmd)?;
    Ok(())
}

pub async fn install_config(plan: &InstallPlan, executor: &dyn CommandRunner) -> Result<()> {
    println!("Configuring system (inside chroot)...");

    Pacman::for_variant(plan.linux_variant())
        .prepare_repositories(plan.setup().repositories(), executor)
        .await?;
    Pacman::for_variant(plan.linux_variant())
        .configure_settings(executor.dry_run())
        .await?;

    install_standard_packages(plan, executor)?;
    configure_machine_identity(executor, plan.linux_variant())?;
    configure_timezone(plan, executor)?;
    configure_locale(plan, executor)?;
    configure_network(plan, executor)?;
    configure_users(plan, executor)?;
    configure_environment(executor)?;
    configure_vconsole(plan, executor)?;
    configure_sudo(executor)?;
    configure_mkinitcpio(plan, executor)?;
    configure_ssh_host_keys(executor)?;

    Ok(())
}

/// Root of the filesystem the `Config` step configures: inside the chroot,
/// `/` *is* the target. Unit tests point this at a fixture so the identity
/// reset — which is destructive by nature — can be exercised without touching
/// the machine running the tests.
fn target_root() -> std::path::PathBuf {
    #[cfg(test)]
    {
        test_target_root()
    }

    #[cfg(not(test))]
    {
        std::path::PathBuf::from("/")
    }
}

#[cfg(test)]
fn test_target_root() -> std::path::PathBuf {
    // Each test thread owns its fixture. Thread-local destruction removes it
    // on success or panic, without sharing paths with another test process.
    thread_local! {
        static FIXTURE: tempfile::TempDir = {
            let root = tempfile::tempdir().expect("create isolated installer target");
            for directory in ["etc", "var/lib/dbus"] {
                std::fs::create_dir_all(root.path().join(directory))
                    .expect("create installer fixture directories");
            }
            root
        };
    }
    FIXTURE.with(|root| root.path().to_path_buf())
}

/// The machine identity files systemd derives the system's identity from.
///
/// Both are cleared before regeneration so a stale value cannot survive: the
/// target is built by `pacstrap`, so anything found here was either a
/// placeholder or — in a copied-root scenario — the source system's identity.
/// Two machines sharing a machine-id break `systemd-firstboot`, journald and
/// anything else that keys on it.
fn machine_identity_files() -> Vec<std::path::PathBuf> {
    let root = target_root();
    vec![
        root.join("etc/machine-id"),
        root.join("var/lib/dbus/machine-id"),
    ]
}

/// Give the target its own machine identity.
///
/// `pacstrap` builds a brand-new root, so on a live ISO this normally finds
/// nothing to do. It runs unconditionally because the alternative — a target
/// that inherits the source system's identity — is a silent, hard-to-debug
/// failure that only appears once the machine is booted from the new disk.
fn configure_machine_identity(
    executor: &dyn CommandRunner,
    linux_variant: ArchLinuxVariant,
) -> Result<()> {
    println!("Setting up machine identity...");

    let files = machine_identity_files();
    if executor.dry_run() {
        for path in &files {
            println!("[DRY RUN] rm -f {}", path.display());
        }
        if linux_variant.is_systemd() {
            println!("[DRY RUN] systemd-machine-id-setup");
        } else {
            println!("[DRY RUN] dbus-uuidgen --ensure=/etc/machine-id");
            println!("[DRY RUN] ln -sfn /etc/machine-id /var/lib/dbus/machine-id");
        }
        return Ok(());
    }

    remove_machine_identity(&files)?;

    if linux_variant.is_systemd() {
        // Best effort: a target without systemd has no identity to speak of,
        // and failing the whole Config step over it would be a worse outcome
        // than a missing machine-id systemd will generate on first boot.
        let mut cmd = Command::new("systemd-machine-id-setup");
        if !executor.run_best_effort(&mut cmd, "machine-id generation") {
            println!(
                "Warning: could not generate a machine-id; systemd will create one on first boot."
            );
        }
        return Ok(());
    }

    ensure_artix_machine_id(executor)
}

/// Generate `/etc/machine-id` with the D-Bus tool Artix ships, falling back to
/// the kernel's random UUID when `dbus-uuidgen` is not installed.
///
/// `/var/lib/dbus/machine-id` must resolve to the same value as
/// `/etc/machine-id`; a symlink is what a stock Artix installation uses.
fn ensure_artix_machine_id(executor: &dyn CommandRunner) -> Result<()> {
    let root = target_root();
    let machine_id = root.join("etc/machine-id");
    let dbus_id = root.join("var/lib/dbus/machine-id");

    let mut cmd = Command::new("dbus-uuidgen");
    cmd.arg(format!("--ensure={}", machine_id.display()));
    if executor.run(&mut cmd).is_err() {
        println!("dbus-uuidgen is unavailable; generating a machine-id directly.");
        write_random_machine_id(&machine_id)?;
    }

    link_dbus_machine_id(&dbus_id)
}

/// Write a random 32-character machine-id without dashes, the format
/// `machine-id(5)` requires.
fn write_random_machine_id(path: &Path) -> Result<()> {
    let uuid = std::fs::read_to_string("/proc/sys/kernel/random/uuid")
        .context("Failed to read a random UUID for /etc/machine-id")?;
    let compact: String = uuid.chars().filter(|character| *character != '-').collect();
    let compact = compact.trim();
    if compact.len() != 32
        || !compact
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        anyhow::bail!("the kernel returned an invalid random UUID: {uuid:?}");
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    std::fs::write(path, format!("{compact}\n"))
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

/// Point `/var/lib/dbus/machine-id` at `/etc/machine-id`, replacing any stale
/// file or link.
fn link_dbus_machine_id(dbus_id: &Path) -> Result<()> {
    if let Some(parent) = dbus_id.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    match std::fs::remove_file(dbus_id) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(anyhow::anyhow!(
                "Failed to replace {}: {error}",
                dbus_id.display()
            ));
        }
    }

    std::os::unix::fs::symlink("/etc/machine-id", dbus_id)
        .with_context(|| format!("Failed to link {} to /etc/machine-id", dbus_id.display()))
}

/// Delete any existing machine identity so regeneration cannot be skipped.
///
/// An absent file is the normal case on a fresh root and is not an error;
/// anything else is, because a stale identity is precisely what this exists
/// to prevent.
fn remove_machine_identity(paths: &[std::path::PathBuf]) -> Result<()> {
    for path in paths {
        match std::fs::remove_file(path) {
            Ok(()) => println!("Removed existing {}", path.display()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(anyhow::anyhow!(
                    "Failed to remove {} before regenerating the machine identity: {error}",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

/// Generate the target's SSH host keys.
///
/// `ssh-keygen -A` only creates keys that are missing, so an existing key is
/// never overwritten. Without it a target that somehow received the source
/// system's key would present the same host identity to every machine it ever
/// connected from. Best effort: when `openssh` is not installed yet the
/// command is simply not there, and systemd's own `sshdgenkey` will cover it
/// at first boot.
fn configure_ssh_host_keys(executor: &dyn CommandRunner) -> Result<()> {
    println!("Setting up SSH host keys...");

    if executor.dry_run() {
        println!("[DRY RUN] ssh-keygen -A");
        return Ok(());
    }

    let mut cmd = Command::new("ssh-keygen");
    cmd.arg("-A");
    if !executor.run_best_effort(&mut cmd, "SSH host key generation") {
        println!(
            "Warning: could not generate SSH host keys now; openssh may not be installed yet."
        );
    }

    Ok(())
}

/// Configure global environment variables
pub fn configure_environment(executor: &dyn CommandRunner) -> Result<()> {
    println!("Configuring global environment variables...");

    if executor.dry_run() {
        println!("[DRY RUN] Creating /etc/profile.d/instantos.sh");
        return Ok(());
    }

    // Ensure directory exists
    std::fs::create_dir_all("/etc/profile.d")?;

    // Resolve EDITOR at install time
    let editor = if std::path::Path::new("/usr/bin/nvim").exists() {
        "/usr/bin/nvim"
    } else if std::path::Path::new("/usr/bin/vim").exists() {
        "/usr/bin/vim"
    } else if std::path::Path::new("/usr/bin/nano").exists() {
        "/usr/bin/nano"
    } else {
        "vi"
    };

    let content = format!(
        r#"# Global environment variables for instantOS
export PAGER=less
export EDITOR={}
export XDG_MENU_PREFIX=gnome-
export _JAVA_AWT_WM_NONREPARENTING=1
"#,
        editor
    );

    std::fs::write("/etc/profile.d/instantos.sh", content)?;

    Ok(())
}

fn install_standard_packages(plan: &InstallPlan, executor: &dyn CommandRunner) -> Result<()> {
    println!("Installing standard packages...");
    let packages = plan.standard_packages();
    let package_refs: Vec<&str> = packages.iter().map(|s| s.as_str()).collect();
    Pacman::for_variant(plan.linux_variant()).install(&package_refs, executor)?;
    Ok(())
}

fn configure_mkinitcpio(plan: &InstallPlan, executor: &dyn CommandRunner) -> Result<()> {
    let initramfs = plan.initramfs();
    if matches!(initramfs, InitramfsConfiguration::Unchanged) {
        return Ok(());
    }
    println!("Configuring initramfs...");
    if executor.dry_run() {
        match initramfs {
            InitramfsConfiguration::Systemd {
                encryption,
                btrfs,
                plymouth,
            } => {
                println!(
                    "[DRY RUN] Configure systemd initramfs: encryption={encryption}, btrfs={btrfs}, Plymouth={}",
                    plymouth.is_some()
                );
            }
            InitramfsConfiguration::Busybox { encryption, btrfs } => {
                println!(
                    "[DRY RUN] Configure busybox initramfs: encryption={encryption}, btrfs={btrfs}"
                );
            }
            InitramfsConfiguration::Unchanged => return Ok(()),
        }
        println!("[DRY RUN] mkinitcpio -P");
        return Ok(());
    }

    let conf_path = "/etc/mkinitcpio.conf";
    let content = std::fs::read_to_string(conf_path).context("Failed to read mkinitcpio.conf")?;

    let mut config = MkinitcpioConfig::parse(&content)?;
    apply_initramfs_hooks(plan.initramfs(), &mut config);

    std::fs::write(conf_path, config.to_string())?;

    // Regenerate initramfs
    executor.run(Command::new("mkinitcpio").arg("-P"))?;

    Ok(())
}

/// Apply the variant's initramfs hook policy to a parsed `mkinitcpio.conf`.
///
/// systemd targets boot with systemd hooks; Artix targets keep the busybox
/// hooks and must never gain an `sd-*` hook, which would wait for a systemd
/// that is not PID 1.
fn apply_initramfs_hooks(initramfs: &InitramfsConfiguration, config: &mut MkinitcpioConfig) {
    let (use_encryption, use_btrfs, use_plymouth) = match initramfs {
        InitramfsConfiguration::Unchanged => return,
        InitramfsConfiguration::Systemd {
            encryption,
            btrfs,
            plymouth,
        } => (*encryption, *btrfs, plymouth.is_some()),
        InitramfsConfiguration::Busybox { encryption, btrfs } => (*encryption, *btrfs, false),
    };

    if use_btrfs {
        config.ensure_module("btrfs");
    }

    if matches!(initramfs, InitramfsConfiguration::Systemd { .. }) {
        // Switch to systemd hooks
        if config.contains_hook("base") && config.contains_hook("udev") {
            config.replace_hook("udev", "systemd");
        }

        // Plymouth should be after systemd but before encrypt/sd-encrypt
        // And definitely before sd-encrypt to show password prompt
        if use_plymouth {
            config.ensure_hook_position(
                "plymouth",
                &["base", "systemd", "udev"],       // After these
                &["sd-encrypt", "encrypt", "lvm2"], // Before these
            );
        }

        // Ensure keyboard and keymap/sd-vconsole are present
        if !config.contains_hook("sd-vconsole") {
            if config.contains_hook("keymap") {
                config.replace_hook("keymap", "sd-vconsole");
            } else {
                // Ensure sd-vconsole comes after keyboard (or just add it if keyboard not present)
                config.ensure_hook_position("sd-vconsole", &["keyboard"], &[]);
            }
        }

        // Remove consolefont if present as sd-vconsole handles it
        config.remove_hook("consolefont");

        // Add encryption hooks in correct order: block -> sd-encrypt -> lvm2 -> resume -> filesystems
        if use_encryption && config.contains_hook("block") && config.contains_hook("filesystems") {
            // Replace legacy encrypt hook if present
            if config.contains_hook("encrypt") {
                config.replace_hook("encrypt", "sd-encrypt");
            } else {
                config.ensure_hook("sd-encrypt");
            }

            // Ensure correct ordering for full disk encryption with LVM and resume
            config.ensure_hook_position("sd-encrypt", &["block"], &["filesystems"]);
            config.ensure_hook_position("lvm2", &["sd-encrypt"], &["filesystems"]);
            config.ensure_hook_position("resume", &["lvm2"], &["filesystems"]);
        }

        return;
    }

    // Artix: busybox initramfs. Undo any systemd hooks a copied configuration
    // may carry before adding the classic equivalents.
    config.remove_hook("sd-vconsole");
    config.remove_hook("sd-encrypt");
    if config.contains_hook("systemd") {
        config.replace_hook("systemd", "udev");
    }

    if !config.contains_hook("keymap") {
        config.ensure_hook_position(
            "keymap",
            &["udev", "keyboard"],
            &["encrypt", "lvm2", "filesystems"],
        );
    }
    if !config.contains_hook("consolefont") {
        config.ensure_hook_position(
            "consolefont",
            &["keymap"],
            &["encrypt", "lvm2", "filesystems"],
        );
    }

    if use_encryption && config.contains_hook("block") && config.contains_hook("filesystems") {
        config.ensure_hook("encrypt");
        config.ensure_hook_position("encrypt", &["block"], &["filesystems"]);
        config.ensure_hook_position("lvm2", &["encrypt"], &["filesystems"]);
        config.ensure_hook_position("resume", &["lvm2"], &["filesystems"]);
    }
}

fn configure_vconsole(plan: &InstallPlan, executor: &dyn CommandRunner) -> Result<()> {
    let keymap = plan.keymap().as_str();
    let font = plan.console_font().installed_name();
    let openrc = matches!(
        plan.linux_variant(),
        ArchLinuxVariant::Artix(ArtixInitSystem::OpenRC)
    );
    let non_default_font = plan.console_font().name() != crate::common::console_font::DEFAULT_NAME;

    let contents = format!("KEYMAP={}\nFONT={}\n", keymap, font);
    println!(
        "Setting console keymap to {} and font to {}",
        keymap,
        plan.console_font().name()
    );

    if executor.dry_run() {
        if plan.console_font().data().is_some() {
            println!(
                "[DRY RUN] Install selected font as /usr/share/kbd/consolefonts/ins-selected.psf"
            );
        }
        println!("[DRY RUN] Write /etc/vconsole.conf:\n{contents}");
        if openrc {
            println!("[DRY RUN] Write /etc/conf.d/keymaps: keymap=\"{keymap}\"");
            println!("[DRY RUN] Write /etc/conf.d/consolefont: consolefont=\"{font}\"");
            if non_default_font {
                println!("[DRY RUN] rc-update add consolefont boot");
            }
        }
        return Ok(());
    }

    let root = target_root();
    crate::common::console_font::install_snapshot(&root, plan.console_font())?;
    std::fs::write(root.join("etc/vconsole.conf"), contents)
        .context("Writing console keyboard and font configuration")?;

    if openrc {
        // OpenRC reads the keymap and font from /etc/conf.d rather than
        // /etc/vconsole.conf, which is a systemd convention.
        let conf_d = root.join("etc/conf.d");
        std::fs::create_dir_all(&conf_d)
            .with_context(|| format!("Failed to create {}", conf_d.display()))?;
        set_conf_d_value(&conf_d.join("keymaps"), "keymap", keymap)?;
        set_conf_d_value(&conf_d.join("consolefont"), "consolefont", font)?;

        if non_default_font {
            executor.run(Command::new("rc-update").args(["add", "consolefont", "boot"]))?;
        }
    }

    Ok(())
}

/// Update one quoted key in an OpenRC `/etc/conf.d` file, preserving every
/// other setting the package shipped.
fn set_conf_d_value(path: &Path, key: &str, value: &str) -> Result<()> {
    let content = std::fs::read_to_string(path).unwrap_or_default();
    // OpenRC sources these files as shell code. Values must remain literal.
    let quoted = crate::common::shell::shell_quote(value);
    let edit = set_keys(&content, &[(key, &quoted)]);
    std::fs::write(path, edit.content).with_context(|| format!("Writing {}", path.display()))
}

fn configure_timezone(plan: &InstallPlan, executor: &dyn CommandRunner) -> Result<()> {
    let timezone = plan.timezone().as_str();

    println!("Setting timezone to {}", timezone);

    // Try timedatectl first
    // timedatectl set-timezone "$REGION"
    // timedatectl needs a running D-Bus/systemd, which the installer's chroot
    // does not provide; the call only stalls there before failing. Go
    // straight to the manual configuration in that case, and on Artix where
    // systemd is not present at all.
    if plan.linux_variant().is_systemd() && !super::is_chroot() {
        let mut cmd = Command::new("timedatectl");
        cmd.arg("set-timezone").arg(timezone);

        // We try to run timedatectl. If it fails (e.g. no D-Bus on the host),
        // we fall back. We suppress the error from executor.run by checking
        // the result.
        if executor.run(&mut cmd).is_ok() {
            // timedatectl set-ntp true
            let mut cmd_ntp = Command::new("timedatectl");
            cmd_ntp.arg("set-ntp").arg("true");
            // NTP might not be controllable in chroot, but that is not fatal
            executor.run_best_effort(&mut cmd_ntp, "timedatectl NTP enable");
            return Ok(());
        }
    }

    if executor.dry_run() {
        println!("[DRY RUN] ln -sf {} /etc/localtime", timezone);
        return Ok(());
    }

    println!("Falling back to manual timezone configuration...");

    // ln -sf /usr/share/zoneinfo/Region/City /etc/localtime
    let source = format!("/usr/share/zoneinfo/{}", timezone);
    let target = target_root().join("etc/localtime");

    // Remove existing link/file if it exists to avoid error
    if target.exists() {
        std::fs::remove_file(&target)?;
    }
    std::os::unix::fs::symlink(&source, &target)?;

    // hwclock --systohc
    let mut cmd_hw = Command::new("hwclock");
    cmd_hw.arg("--systohc");
    executor.run(&mut cmd_hw)?;

    Ok(())
}

fn configure_locale(plan: &InstallPlan, executor: &dyn CommandRunner) -> Result<()> {
    let locale = plan.locale().as_str();

    println!("Setting locale to {}", locale);

    // localectl talks to systemd-localed over D-Bus, which neither Artix nor
    // the installer's chroot provides. Extract just the LANG part, e.g.
    // "en_US.UTF-8" from "en_US.UTF-8 UTF-8".
    let lang = locale.split_whitespace().next().unwrap_or(locale);
    let use_localectl = plan.linux_variant().is_systemd() && !super::is_chroot();

    if executor.dry_run() {
        println!("[DRY RUN] Uncommenting {} in /etc/locale.gen", locale);
        println!("[DRY RUN] locale-gen");
        if use_localectl {
            println!("[DRY RUN] localectl set-locale LANG={}", lang);
        } else {
            println!("[DRY RUN] Write /etc/locale.conf: LANG={}", lang);
            if plan.linux_variant().is_artix() {
                println!("[DRY RUN] Write /etc/env.d/02locale: LANG={}", lang);
            }
        }
        return Ok(());
    }

    let root = target_root();
    // Read /etc/locale.gen
    let locale_gen_path = root.join("etc/locale.gen");
    let content = std::fs::read_to_string(&locale_gen_path)
        .with_context(|| format!("Failed to read {}", locale_gen_path.display()))?;

    // Enable the selected locale, preserving the file's formatting. The
    // answer is always an available locale, so this only ever uncomments;
    // the write is skipped when nothing changed.
    if let Some(updated) = apply_enable_disable(&content, &[locale.to_owned()], &[]) {
        std::fs::write(&locale_gen_path, updated)?;
    }

    // Run locale-gen
    let mut cmd = Command::new("locale-gen");
    executor.run(&mut cmd)?;

    // Set the system locale. localectl talks to systemd-localed over
    // D-Bus, which the installer's chroot does not run (the call only
    // stalls there), so write /etc/locale.conf directly in that case —
    // the same thing localectl would do.
    if use_localectl {
        let mut cmd = Command::new("localectl");
        cmd.arg("set-locale").arg(format!("LANG={}", lang));
        executor.run(&mut cmd)?;
    } else {
        std::fs::write(root.join("etc/locale.conf"), format!("LANG={}\n", lang))
            .context("Writing /etc/locale.conf")?;

        if plan.linux_variant().is_artix() {
            // Artix's OpenRC profile scripts read /etc/env.d before
            // /etc/locale.conf exists on a fresh boot.
            let env_d = root.join("etc/env.d");
            std::fs::create_dir_all(&env_d)
                .with_context(|| format!("Failed to create {}", env_d.display()))?;
            std::fs::write(env_d.join("02locale"), format!("LANG={}\n", lang))
                .context("Writing /etc/env.d/02locale")?;
        }
    }

    Ok(())
}

fn configure_network(plan: &InstallPlan, executor: &dyn CommandRunner) -> Result<()> {
    let hostname = plan.hostname().as_str();
    let openrc = matches!(
        plan.linux_variant(),
        ArchLinuxVariant::Artix(ArtixInitSystem::OpenRC)
    );

    println!("Setting hostname to {}", hostname);

    if executor.dry_run() {
        println!("[DRY RUN] echo '{}' > /etc/hostname", hostname);
        println!("[DRY RUN] Writing /etc/hosts");
        if openrc {
            println!("[DRY RUN] Write /etc/conf.d/hostname: hostname=\"{hostname}\"");
        }
        return Ok(());
    }

    let root = target_root();
    std::fs::create_dir_all(root.join("etc"))
        .with_context(|| format!("Failed to create {}", root.join("etc").display()))?;
    std::fs::write(root.join("etc/hostname"), format!("{}\n", hostname))
        .context("Writing /etc/hostname")?;

    let hosts_content = format!(
        "127.0.0.1\tlocalhost\n::1\t\tlocalhost\n127.0.1.1\t{}.localdomain\t{}\n",
        hostname, hostname
    );
    std::fs::write(root.join("etc/hosts"), hosts_content).context("Writing /etc/hosts")?;

    if openrc {
        // OpenRC's hostname service reads the value from /etc/conf.d.
        let conf_d = root.join("etc/conf.d");
        std::fs::create_dir_all(&conf_d)
            .with_context(|| format!("Failed to create {}", conf_d.display()))?;
        std::fs::write(
            conf_d.join("hostname"),
            format!("hostname=\"{}\"\n", hostname),
        )
        .context("Writing the OpenRC hostname configuration")?;
    }

    Ok(())
}

fn configure_users(plan: &InstallPlan, executor: &dyn CommandRunner) -> Result<()> {
    let username = plan.username().as_str();
    let password = plan.password().expose();

    println!("Configuring user: {}", username);

    // Set root password
    // echo "root:password" | chpasswd
    let root_input = format!("root:{}", password);
    let mut cmd_root = Command::new("chpasswd");
    executor.run_with_secret_input(&mut cmd_root, &root_input)?;

    // Ensure all required groups exist
    ensure_groups_exist(executor)?;

    // Check if user already exists (idempotent)
    let user_exists = if executor.dry_run() {
        false // In dry-run, always show what would happen
    } else {
        Command::new("id")
            .arg(username)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };

    if user_exists {
        println!(
            "User {} already exists, ensuring group membership and shell...",
            username
        );
        // Add user to groups if not already a member
        add_user_to_groups(username, executor)?;

        // Ensure shell is zsh
        let mut cmd_chsh = Command::new("chsh");
        cmd_chsh.arg("-s").arg("/bin/zsh").arg(username);
        executor.run(&mut cmd_chsh)?;
    } else {
        let shell = "/bin/zsh";

        let mut cmd_user = Command::new("useradd");
        cmd_user
            .arg("-m")
            .arg("-G")
            .arg(USER_GROUPS.join(","))
            .arg("-s")
            .arg(shell)
            .arg(username);

        executor.run(&mut cmd_user)?;
    }

    // Set user password
    let user_input = format!("{}:{}", username, password);
    let mut cmd_pass = Command::new("chpasswd");
    executor.run_with_secret_input(&mut cmd_pass, &user_input)?;

    if plan.storage().keeps_home() {
        if !executor.dry_run() {
            warn_about_home_ownership(username)?;
        }
    } else {
        // Initialize standard XDG directories for a freshly installed home.
        let mut cmd_xdg = Command::new("su");
        cmd_xdg.arg("-c").arg("xdg-user-dirs-update").arg(username);
        executor.run(&mut cmd_xdg)?;
    }

    Ok(())
}

fn warn_about_home_ownership(username: &str) -> Result<()> {
    use std::os::unix::fs::MetadataExt;

    let account = nix::unistd::User::from_name(username)?
        .with_context(|| format!("Could not read the installed account {username}"))?;
    let home = std::fs::symlink_metadata(&account.dir)
        .with_context(|| format!("Could not inspect {}", account.dir.display()))?;
    if home.uid() != account.uid.as_raw() || home.gid() != account.gid.as_raw() {
        eprintln!(
            "Warning: {} is owned by UID:GID {}:{}, and the account {username} uses {}:{}. Check ownership before logging in.",
            account.dir.display(),
            home.uid(),
            home.gid(),
            account.uid,
            account.gid,
        );
    }
    Ok(())
}

pub fn configure_sudo(executor: &dyn CommandRunner) -> Result<()> {
    println!("Configuring sudoers...");
    // Uncomment %wheel ALL=(ALL:ALL) ALL

    if executor.dry_run() {
        println!("[DRY RUN] Uncommenting %wheel in /etc/sudoers");
        println!("[DRY RUN] Adding 'Defaults env_reset,pwfeedback' to /etc/sudoers");
    } else {
        let sudoers_path = "/etc/sudoers";
        let content =
            std::fs::read_to_string(sudoers_path).context("Failed to read /etc/sudoers")?;

        let mut new_lines = Vec::new();
        let mut has_pwfeedback = false; // Track if pwfeedback already exists

        for line in content.lines() {
            // Check if pwfeedback line already exists
            if line.contains("Defaults") && line.contains("pwfeedback") {
                has_pwfeedback = true;
            }

            if line.contains("%wheel ALL=(ALL:ALL) ALL") && line.trim().starts_with('#') {
                new_lines.push(line.replacen('#', "", 1).trim().to_string());
            } else {
                new_lines.push(line.to_string());
            }
        }

        // Add defaults only if not present (FIXED: Check has_pwfeedback first)
        if !has_pwfeedback {
            new_lines.push("Defaults env_reset,pwfeedback".to_string());
        }

        std::fs::write(sudoers_path, new_lines.join("\n"))?;
    }

    Ok(())
}

pub fn configure_plymouth(plymouth: &PlymouthTheme, executor: &dyn CommandRunner) -> Result<()> {
    let theme = plymouth.theme();
    println!("Configuring Plymouth...");

    if executor.dry_run() {
        println!("[DRY RUN] Setting Plymouth theme to {theme}");
        println!("[DRY RUN] plymouth-set-default-theme -R {theme}");
        return Ok(());
    }

    let theme_dir = format!("/usr/share/plymouth/themes/{}", theme);
    if !std::path::Path::new(&theme_dir).exists() {
        println!(
            "Warning: Plymouth theme '{}' not found at {}. Skipping theme apply.",
            theme, theme_dir
        );
        return Ok(());
    }

    // Configure Plymouth theme
    let plymouth_conf = "/etc/plymouth/plymouthd.conf";

    // Create Plymouth config directory if it doesn't exist
    if let Err(e) = std::fs::create_dir_all("/etc/plymouth") {
        println!("Warning: Failed to create /etc/plymouth: {}", e);
        return Ok(());
    }

    // Note: If encryption is used, the Plymouth theme will not be visible during the
    // password prompt because the theme files are on the encrypted partition.
    let config_content = format!("[Daemon]\nTheme={}\nShowDelay=0\n", theme);

    if let Err(e) = std::fs::write(plymouth_conf, config_content) {
        println!(
            "Warning: Failed to write /etc/plymouth/plymouthd.conf: {}",
            e
        );
        return Ok(());
    }

    // Set the default theme.
    let mut cmd = Command::new("plymouth-set-default-theme");
    cmd.arg("-R").arg(theme);
    if let Err(e) = executor.run(&mut cmd) {
        println!("Warning: Failed to apply Plymouth theme: {}", e);
        return Ok(());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::arch::arch_linux_variant::{ArchLinuxVariant, ArtixInitSystem};
    use crate::arch::execution::CommandRunner;
    use crate::arch::execution::mock::MockRunner;
    use std::process::{Command, Output};

    #[test]
    fn openrc_configuration_preserves_literal_shell_values() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("keymaps");
        for value in [
            "us",
            "$(printf${IFS}injected)",
            "`printf injected`",
            "a'\"b\\c",
        ] {
            std::fs::write(&path, "# Keep package settings\nother=yes\nkeymap=old\n").unwrap();
            super::set_conf_d_value(&path, "keymap", value).unwrap();
            let content = std::fs::read_to_string(&path).unwrap();
            assert!(content.contains("# Keep package settings\nother=yes\n"));
            super::set_conf_d_value(&path, "keymap", value).unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), content);

            let output = Command::new("sh")
                .args(["-c", ". \"$1\"; printf '%s' \"$keymap\"", "sh"])
                .arg(&path)
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
            assert_eq!(String::from_utf8(output.stdout).unwrap(), value);
        }
    }

    /// A runner whose commands always fail, used to exercise fallbacks.
    struct FailingRunner;

    impl CommandRunner for FailingRunner {
        fn dry_run(&self) -> bool {
            false
        }

        fn run(&self, _command: &mut Command) -> anyhow::Result<()> {
            anyhow::bail!("simulated failure")
        }

        fn run_with_input(&self, _command: &mut Command, _input: &str) -> anyhow::Result<()> {
            anyhow::bail!("simulated failure")
        }

        fn run_with_output(&self, _command: &mut Command) -> anyhow::Result<Option<Output>> {
            anyhow::bail!("simulated failure")
        }

        fn log(&self, _message: &str) {}
    }

    fn artix_plan(init: ArtixInitSystem) -> crate::arch::engine::InstallPlan {
        let mut plan = crate::arch::engine::test_install_plan();
        plan = plan.with_test_inputs(|inputs| {
            inputs.linux_variant = ArchLinuxVariant::Artix(init);
        });
        plan
    }

    fn encrypted_storage() -> crate::arch::engine::StoragePlan {
        use crate::arch::engine::{
            DiskPath, EncryptionPassword, EncryptionPlan, FilesystemPlan, StoragePlan,
        };
        StoragePlan::Automatic {
            disk: DiskPath::parse("/dev/test-disk").unwrap(),
            filesystem: FilesystemPlan::Ext4,
            encryption: Some(EncryptionPlan {
                password: EncryptionPassword::parse("test-secret").unwrap(),
            }),
        }
    }

    #[test]
    fn console_configuration_persists_font_and_keymap_and_plans_font_package() {
        let mut plan = crate::arch::engine::test_install_plan();
        plan = plan.with_test_inputs(|inputs| {
            inputs.console_font =
                crate::common::console_font::ConsoleFont::parse("ter-v32n").unwrap();
        });
        super::configure_vconsole(&plan, &MockRunner::new()).unwrap();
        let contents =
            std::fs::read_to_string(super::target_root().join("etc/vconsole.conf")).unwrap();
        assert_eq!(contents, "KEYMAP=us\nFONT=ter-v32n\n");
        assert!(
            plan.standard_packages()
                .to_vec()
                .contains(&"terminus-font".to_string())
        );
        plan = plan.with_test_inputs(|inputs| {
            inputs.console_font = crate::common::console_font::ConsoleFont::default();
        });
        assert!(
            plan.standard_packages()
                .to_vec()
                .contains(&"kbd".to_string())
        );
    }

    #[test]
    fn selected_snapshot_is_written_to_target_without_original_package() {
        let mut plan = crate::arch::engine::test_install_plan();
        let directory = tempfile::tempdir().unwrap();
        let data = crate::common::console_font::tests::psf1();
        std::fs::write(directory.path().join("custom.psf"), &data).unwrap();
        plan = plan.with_test_inputs(|inputs| {
            inputs.console_font = crate::common::console_font::discover(directory.path())
                .unwrap()
                .into_iter()
                .find(|font| font.name() == "custom")
                .unwrap();
        });
        let answer = plan.console_font().to_answer().unwrap();
        drop(directory);
        plan = plan.with_test_inputs(|inputs| {
            inputs.console_font = crate::common::console_font::ConsoleFont::parse(&answer).unwrap();
        });
        super::configure_vconsole(&plan, &MockRunner::new()).unwrap();
        let root = super::target_root();
        assert_eq!(
            std::fs::read(root.join("usr/share/kbd/consolefonts/ins-selected.psf")).unwrap(),
            data
        );
        assert_eq!(
            std::fs::read_to_string(root.join("etc/vconsole.conf")).unwrap(),
            "KEYMAP=us\nFONT=ins-selected\n"
        );
        assert!(plan.standard_packages().iter().any(|p| p == "kbd"));
        assert!(plan.console_font().legacy_package().is_none());
    }

    #[test]
    fn test_ensure_groups_exist_commands() {
        let mock = MockRunner::new();
        super::ensure_groups_exist(&mock).unwrap();

        let log = mock.command_log();
        // Should have groupadd commands for each user group + system groups
        assert!(
            log.iter()
                .all(|c| c.contains("groupadd") && c.contains("-f"))
        );
        // USER_GROUPS: wheel, video, docker, sys, rfkill
        assert!(log.iter().any(|c| c.contains("wheel")));
        assert!(log.iter().any(|c| c.contains("video")));
        assert!(log.iter().any(|c| c.contains("docker")));
        assert!(log.iter().any(|c| c.contains("sys")));
        assert!(log.iter().any(|c| c.contains("rfkill")));
        // SYSTEM_GROUPS: nobody
        assert!(log.iter().any(|c| c.contains("nobody")));
        // Total: 5 user groups + 1 system group = 6
        assert_eq!(log.len(), 6);
    }

    #[test]
    fn test_add_user_to_groups_commands() {
        let mock = MockRunner::new();
        super::add_user_to_groups("testuser", &mock).unwrap();

        let log = mock.command_log();
        assert_eq!(log.len(), 1);
        assert!(log[0].contains("usermod"));
        assert!(log[0].contains("-aG"));
        assert!(log[0].contains("testuser"));
    }

    #[test]
    fn the_target_gets_its_own_machine_identity() {
        // `MockRunner` stands in for the chroot and `target_root` for the
        // test fixture, so this exercises the real code path: any identity
        // left in the target is cleared and a fresh one generated.
        let mock = MockRunner::new();
        super::configure_machine_identity(&mock, ArchLinuxVariant::Arch).unwrap();

        let log = mock.command_log();
        assert!(
            log.iter()
                .any(|command| command.starts_with("systemd-machine-id-setup")),
            "a machine identity must be generated: {log:?}"
        );
        for path in super::machine_identity_files() {
            assert!(
                !path.exists(),
                "{} survived into the configured system",
                path.display()
            );
        }
    }

    #[test]
    fn artix_machine_identity_uses_dbus_uuidgen_and_links_the_dbus_file() {
        let mock = MockRunner::new();
        super::configure_machine_identity(&mock, ArchLinuxVariant::Artix(ArtixInitSystem::OpenRC))
            .unwrap();

        let log = mock.command_log();
        assert!(
            log.iter()
                .any(|command| command.starts_with("dbus-uuidgen --ensure=")),
            "dbus-uuidgen must generate the identity: {log:?}"
        );
        let dbus_id = super::target_root().join("var/lib/dbus/machine-id");
        assert_eq!(
            std::fs::read_link(&dbus_id).unwrap(),
            std::path::Path::new("/etc/machine-id")
        );
    }

    #[test]
    fn artix_machine_identity_falls_back_to_a_random_uuid() {
        let runner = FailingRunner;
        super::configure_machine_identity(
            &runner,
            ArchLinuxVariant::Artix(ArtixInitSystem::OpenRC),
        )
        .unwrap();

        let machine_id = std::fs::read_to_string(super::target_root().join("etc/machine-id"))
            .expect("the fallback writes /etc/machine-id");
        let machine_id = machine_id.trim();
        assert_eq!(machine_id.len(), 32, "{machine_id:?}");
        assert!(
            machine_id
                .chars()
                .all(|character| character.is_ascii_hexdigit()),
            "{machine_id:?}"
        );
        assert_eq!(
            std::fs::read_link(super::target_root().join("var/lib/dbus/machine-id")).unwrap(),
            std::path::Path::new("/etc/machine-id")
        );
    }

    #[test]
    fn artix_console_configuration_writes_openrc_files_and_enables_consolefont() {
        let mut plan = artix_plan(ArtixInitSystem::OpenRC);
        plan = plan.with_test_inputs(|inputs| {
            inputs.console_font =
                crate::common::console_font::ConsoleFont::parse("ter-v32n").unwrap();
        });
        let mock = MockRunner::new();

        super::configure_vconsole(&plan, &mock).unwrap();

        let root = super::target_root();
        assert_eq!(
            std::fs::read_to_string(root.join("etc/vconsole.conf")).unwrap(),
            "KEYMAP=us\nFONT=ter-v32n\n"
        );
        // The config editor preserves existing trailing newlines; new files
        // have none. Both forms are valid shell configuration.
        assert_eq!(
            std::fs::read_to_string(root.join("etc/conf.d/keymaps"))
                .unwrap()
                .trim_end_matches('\n'),
            "keymap=us"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("etc/conf.d/consolefont"))
                .unwrap()
                .trim_end_matches('\n'),
            "consolefont=ter-v32n"
        );
        assert!(
            mock.command_log()
                .contains(&"rc-update add consolefont boot".to_string()),
            "{:?}",
            mock.command_log()
        );
    }

    #[test]
    fn artix_default_font_does_not_enable_the_consolefont_service() {
        let plan = artix_plan(ArtixInitSystem::OpenRC);
        let mock = MockRunner::new();

        super::configure_vconsole(&plan, &mock).unwrap();

        assert!(
            !mock
                .command_log()
                .iter()
                .any(|command| command.contains("consolefont")),
            "{:?}",
            mock.command_log()
        );
    }

    #[test]
    fn artix_openrc_hostname_is_written_to_conf_d() {
        let plan = artix_plan(ArtixInitSystem::OpenRC);

        super::configure_network(&plan, &MockRunner::new()).unwrap();

        assert_eq!(
            std::fs::read_to_string(super::target_root().join("etc/conf.d/hostname")).unwrap(),
            "hostname=\"test-host\"\n"
        );
    }

    #[test]
    fn artix_timezone_uses_zoneinfo_and_hwclock() {
        let plan = artix_plan(ArtixInitSystem::OpenRC);
        let mock = MockRunner::new();

        super::configure_timezone(&plan, &mock).unwrap();

        assert_eq!(
            std::fs::read_link(super::target_root().join("etc/localtime")).unwrap(),
            std::path::Path::new("/usr/share/zoneinfo/UTC")
        );
        assert_eq!(mock.command_log(), vec!["hwclock --systohc"]);
    }

    #[test]
    fn artix_locale_is_written_to_locale_conf_and_env_d() {
        let root = super::target_root();
        std::fs::create_dir_all(root.join("etc")).unwrap();
        std::fs::write(root.join("etc/locale.gen"), "#en_US.UTF-8 UTF-8\n").unwrap();
        let plan = artix_plan(ArtixInitSystem::OpenRC);

        super::configure_locale(&plan, &MockRunner::new()).unwrap();

        assert_eq!(
            std::fs::read_to_string(root.join("etc/locale.conf")).unwrap(),
            "LANG=en_US.UTF-8\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("etc/env.d/02locale")).unwrap(),
            "LANG=en_US.UTF-8\n"
        );
    }

    #[test]
    fn artix_encryption_adds_init_service_packages() {
        let mut plan = artix_plan(ArtixInitSystem::OpenRC);
        plan = plan.with_test_inputs(|inputs| {
            inputs.storage = encrypted_storage();
        });

        let packages = plan.standard_packages().to_vec();

        assert!(packages.contains(&"lvm2".to_string()));
        assert!(packages.contains(&"cryptsetup".to_string()));
        assert!(packages.contains(&"lvm2-openrc".to_string()));
        assert!(packages.contains(&"cryptsetup-openrc".to_string()));
    }

    #[test]
    fn artix_never_plans_plymouth() {
        let mut plan = artix_plan(ArtixInitSystem::OpenRC);
        plan = plan.with_test_inputs(|inputs| {
            inputs.use_plymouth = true;
        });

        assert!(
            !plan
                .standard_packages()
                .to_vec()
                .contains(&"plymouth".to_string())
        );
    }

    #[test]
    fn artix_initramfs_uses_busybox_encryption_hooks() {
        use crate::arch::mkinitcpio::MkinitcpioConfig;

        let mut plan = artix_plan(ArtixInitSystem::OpenRC);
        plan = plan.with_test_inputs(|inputs| {
            inputs.storage = encrypted_storage();
        });
        let mut config = MkinitcpioConfig::parse(
            "HOOKS=(base udev autodetect modconf kms keyboard keymap consolefont block filesystems fsck)",
        )
        .unwrap();

        super::apply_initramfs_hooks(plan.initramfs(), &mut config);

        assert!(config.contains_hook("udev"));
        assert!(config.contains_hook("keymap"));
        assert!(config.contains_hook("consolefont"));
        assert!(config.contains_hook("encrypt"));
        assert!(config.contains_hook("lvm2"));
        assert!(config.contains_hook("resume"));
        assert!(!config.contains_hook("systemd"));
        assert!(!config.contains_hook("sd-encrypt"));
        assert!(!config.contains_hook("sd-vconsole"));
    }

    #[test]
    fn artix_initramfs_removes_systemd_hooks_from_a_copied_configuration() {
        use crate::arch::mkinitcpio::MkinitcpioConfig;

        let mut plan = artix_plan(ArtixInitSystem::S6);
        plan = plan.with_test_inputs(|inputs| {
            inputs.storage = encrypted_storage();
        });
        let mut config = MkinitcpioConfig::parse(
            "HOOKS=(base systemd sd-vconsole block sd-encrypt filesystems)",
        )
        .unwrap();

        super::apply_initramfs_hooks(plan.initramfs(), &mut config);

        assert!(config.contains_hook("udev"));
        assert!(config.contains_hook("encrypt"));
        assert!(!config.contains_hook("systemd"));
        assert!(!config.contains_hook("sd-encrypt"));
        assert!(!config.contains_hook("sd-vconsole"));
    }

    #[test]
    fn arch_initramfs_keeps_the_systemd_hooks() {
        use crate::arch::mkinitcpio::MkinitcpioConfig;

        let mut plan = crate::arch::engine::test_install_plan();
        plan = plan.with_test_inputs(|inputs| {
            inputs.storage = encrypted_storage();
        });
        let mut config = MkinitcpioConfig::parse(
            "HOOKS=(base udev autodetect modconf kms keyboard keymap consolefont block filesystems fsck)",
        )
        .unwrap();

        super::apply_initramfs_hooks(plan.initramfs(), &mut config);

        assert!(config.contains_hook("systemd"));
        assert!(config.contains_hook("sd-vconsole"));
        assert!(config.contains_hook("sd-encrypt"));
        assert!(!config.contains_hook("encrypt"));
        assert!(!config.contains_hook("consolefont"));
    }

    #[test]
    fn an_existing_machine_identity_is_cleared_before_regeneration() {
        let files = super::machine_identity_files();
        for path in &files {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, "0123456789abcdef0123456789abcdef\n").unwrap();
        }

        super::remove_machine_identity(&files).unwrap();

        for path in &files {
            assert!(!path.exists(), "{} was not cleared", path.display());
        }
    }

    #[test]
    fn an_absent_machine_identity_is_not_an_error() {
        // The normal case on a fresh `pacstrap` root.
        let dir = tempfile::tempdir().unwrap();
        super::remove_machine_identity(&[dir.path().join("machine-id")]).unwrap();
    }

    #[test]
    fn a_removal_failure_is_reported_rather_than_ignored() {
        // A directory where a file is expected cannot be removed, and
        // silently continuing would leave a stale identity in place.
        let dir = tempfile::tempdir().unwrap();
        let directory = dir.path().join("machine-id");
        std::fs::create_dir(&directory).unwrap();

        let error = super::remove_machine_identity(&[directory]).unwrap_err();
        assert!(error.to_string().contains("machine-id"));
    }

    #[test]
    fn ssh_host_keys_are_generated_without_overwriting() {
        // `ssh-keygen -A` only fills in missing keys, so calling it is safe
        // and idempotent.
        let mock = MockRunner::new();
        super::configure_ssh_host_keys(&mock).unwrap();

        assert_eq!(mock.command_log(), vec!["ssh-keygen -A"]);
    }
}
