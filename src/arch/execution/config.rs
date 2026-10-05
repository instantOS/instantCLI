use super::CommandRunner;
use crate::arch::engine::InstallPlan;
use crate::arch::execution::pacman::Pacman;
use crate::arch::mkinitcpio::MkinitcpioConfig;
use crate::common::locale_gen::apply_enable_disable;
use anyhow::{Context, Result};
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

    // Enable multilib for 32-bit support (needed for lib32-vulkan-* GPU drivers)
    // This runs before package installation so lib32 packages can be installed.
    println!("Enabling multilib repository...");
    Pacman::current()
        .enable_multilib(executor.dry_run())
        .await?;

    // Update repos after enabling multilib
    sync_repos(executor)?;

    configure_pacman_target(executor).await?;
    install_standard_packages(plan, executor)?;
    configure_machine_identity(executor)?;
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
    use std::sync::OnceLock;

    static FIXTURE: OnceLock<std::path::PathBuf> = OnceLock::new();

    FIXTURE
        .get_or_init(|| {
            let root = std::env::temp_dir().join("ins-execution-config-target");
            for directory in ["etc", "var/lib/dbus"] {
                let _ = std::fs::create_dir_all(root.join(directory));
            }
            root
        })
        .clone()
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
fn configure_machine_identity(executor: &dyn CommandRunner) -> Result<()> {
    println!("Setting up machine identity...");

    let files = machine_identity_files();
    if executor.dry_run() {
        for path in &files {
            println!("[DRY RUN] rm -f {}", path.display());
        }
        println!("[DRY RUN] systemd-machine-id-setup");
        return Ok(());
    }

    remove_machine_identity(&files)?;

    // Best effort: a target without systemd has no identity to speak of, and
    // failing the whole Config step over it would be a worse outcome than a
    // missing machine-id systemd will generate on first boot.
    let mut cmd = Command::new("systemd-machine-id-setup");
    if !executor.run_best_effort(&mut cmd, "machine-id generation") {
        println!(
            "Warning: could not generate a machine-id; systemd will create one on first boot."
        );
    }

    Ok(())
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

fn sync_repos(executor: &dyn CommandRunner) -> Result<()> {
    println!("Updating package databases...");
    let mut cmd = Command::new("pacman");
    cmd.arg("-Sy");
    executor.run(&mut cmd)?;
    Ok(())
}

fn install_standard_packages(plan: &InstallPlan, executor: &dyn CommandRunner) -> Result<()> {
    println!("Installing standard packages...");
    let packages = crate::arch::execution::packages::build_standard_package_plan(plan)?;
    let package_refs: Vec<&str> = packages.iter().map(|s| s.as_str()).collect();
    Pacman::current().install(&package_refs, executor)?;
    Ok(())
}

async fn configure_pacman_target(executor: &dyn CommandRunner) -> Result<()> {
    println!("Configuring target pacman settings...");
    Pacman::current()
        .configure_settings(executor.dry_run())
        .await?;
    Ok(())
}

/// Packages required for configuration steps (installed in a single batch elsewhere)
pub fn config_package_list(plan: &InstallPlan) -> Vec<String> {
    let mut packages = vec!["kbd".to_string()];
    if let Some(package) = plan.console_font.legacy_package() {
        packages.push(package.to_string());
    }

    if plan.storage.encryption().is_some() {
        packages.push("lvm2".to_string());
        packages.push("cryptsetup".to_string());
    }

    if plan.use_plymouth && !plan.minimal_mode {
        packages.push("plymouth".to_string());
    }

    packages
}

fn configure_mkinitcpio(plan: &InstallPlan, executor: &dyn CommandRunner) -> Result<()> {
    let use_encryption = plan.storage.encryption().is_some();
    let use_plymouth = plan.use_plymouth;
    let use_btrfs = plan.storage.filesystem().is_btrfs();

    if !use_encryption && !use_plymouth && !use_btrfs {
        return Ok(());
    }

    if use_encryption {
        println!("Configuring mkinitcpio for encryption...");
    }
    if use_plymouth && !plan.minimal_mode {
        println!("Configuring mkinitcpio for Plymouth...");
    }

    if executor.dry_run() {
        if use_btrfs {
            println!("[DRY RUN] Adding 'btrfs' to MODULES in /etc/mkinitcpio.conf");
        }
        if use_plymouth && !plan.minimal_mode {
            println!("[DRY RUN] Adding 'plymouth' to HOOKS in /etc/mkinitcpio.conf");
        }
        if use_encryption {
            println!("[DRY RUN] Adding 'encrypt lvm2' to HOOKS in /etc/mkinitcpio.conf");
        }
        println!("[DRY RUN] mkinitcpio -P");
        return Ok(());
    }

    let conf_path = "/etc/mkinitcpio.conf";
    let content = std::fs::read_to_string(conf_path).context("Failed to read mkinitcpio.conf")?;

    let mut config = MkinitcpioConfig::parse(&content)?;

    if use_btrfs {
        config.ensure_module("btrfs");
    }

    // Switch to systemd hooks
    if config.contains_hook("base") && config.contains_hook("udev") {
        config.replace_hook("udev", "systemd");
    }

    // Plymouth should be after systemd but before encrypt/sd-encrypt
    // And definitely before sd-encrypt to show password prompt
    if use_plymouth && !plan.minimal_mode {
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

    std::fs::write(conf_path, config.to_string())?;

    // Regenerate initramfs
    executor.run(Command::new("mkinitcpio").arg("-P"))?;

    Ok(())
}

fn configure_vconsole(plan: &InstallPlan, executor: &dyn CommandRunner) -> Result<()> {
    let keymap = plan.keymap.as_str();

    let contents = format!(
        "KEYMAP={}\nFONT={}\n",
        keymap,
        plan.console_font.installed_name()
    );
    println!(
        "Setting console keymap to {} and font to {}",
        keymap,
        plan.console_font.name()
    );

    if executor.dry_run() {
        if plan.console_font.data().is_some() {
            println!(
                "[DRY RUN] Install selected font as /usr/share/kbd/consolefonts/ins-selected.psf"
            );
        }
        println!("[DRY RUN] Write /etc/vconsole.conf:\n{contents}");
    } else {
        if let Some(data) = plan.console_font.data() {
            let directory = target_root().join("usr/share/kbd/consolefonts");
            std::fs::create_dir_all(&directory)
                .context("Creating target console font directory")?;
            std::fs::write(directory.join("ins-selected.psf"), data)
                .context("Installing selected console font")?;
        }
        std::fs::write(target_root().join("etc/vconsole.conf"), contents)
            .context("Writing console keyboard and font configuration")?;
    }

    Ok(())
}

fn configure_timezone(plan: &InstallPlan, executor: &dyn CommandRunner) -> Result<()> {
    let timezone = plan.timezone.as_str();

    println!("Setting timezone to {}", timezone);

    // Try timedatectl first
    // timedatectl set-timezone "$REGION"
    // timedatectl needs a running D-Bus/systemd, which the installer's chroot
    // does not provide; the call only stalls there before failing. Go
    // straight to the manual configuration in that case.
    if !super::is_chroot() {
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
    let target = "/etc/localtime";

    // Remove existing link/file if it exists to avoid error
    if std::path::Path::new(target).exists() {
        std::fs::remove_file(target)?;
    }
    std::os::unix::fs::symlink(&source, target)?;

    // hwclock --systohc
    let mut cmd_hw = Command::new("hwclock");
    cmd_hw.arg("--systohc");
    executor.run(&mut cmd_hw)?;

    Ok(())
}

fn configure_locale(plan: &InstallPlan, executor: &dyn CommandRunner) -> Result<()> {
    let locale = plan.locale.as_str();

    println!("Setting locale to {}", locale);

    if executor.dry_run() {
        println!("[DRY RUN] Uncommenting {} in /etc/locale.gen", locale);
        println!("[DRY RUN] locale-gen");
        // Extract just the LANG part, e.g., "en_US.UTF-8" from "en_US.UTF-8 UTF-8"
        let lang = locale.split_whitespace().next().unwrap_or(locale);
        println!("[DRY RUN] localectl set-locale LANG={}", lang);
    } else {
        // Read /etc/locale.gen
        let locale_gen_path = "/etc/locale.gen";
        let content =
            std::fs::read_to_string(locale_gen_path).context("Failed to read /etc/locale.gen")?;

        // Enable the selected locale, preserving the file's formatting. The
        // answer is always an available locale, so this only ever uncomments;
        // the write is skipped when nothing changed.
        if let Some(updated) = apply_enable_disable(&content, &[locale.to_owned()], &[]) {
            std::fs::write(locale_gen_path, updated)?;
        }

        // Run locale-gen
        let mut cmd = Command::new("locale-gen");
        executor.run(&mut cmd)?;

        // Set the system locale. localectl talks to systemd-localed over
        // D-Bus, which the installer's chroot does not run (the call only
        // stalls there), so write /etc/locale.conf directly in that case —
        // the same thing localectl would do.
        // Extract just the LANG part, e.g., "en_US.UTF-8" from "en_US.UTF-8 UTF-8"
        let lang = locale.split_whitespace().next().unwrap_or(locale);
        if super::is_chroot() {
            std::fs::write("/etc/locale.conf", format!("LANG={}\n", lang))?;
        } else {
            let mut cmd = Command::new("localectl");
            cmd.arg("set-locale").arg(format!("LANG={}", lang));
            executor.run(&mut cmd)?;
        }
    }

    Ok(())
}

fn configure_network(plan: &InstallPlan, executor: &dyn CommandRunner) -> Result<()> {
    let hostname = plan.hostname.as_str();

    println!("Setting hostname to {}", hostname);

    if executor.dry_run() {
        println!("[DRY RUN] echo '{}' > /etc/hostname", hostname);
        println!("[DRY RUN] Writing /etc/hosts");
    } else {
        std::fs::write("/etc/hostname", format!("{}\n", hostname))?;

        let hosts_content = format!(
            "127.0.0.1\tlocalhost\n::1\t\tlocalhost\n127.0.1.1\t{}.localdomain\t{}\n",
            hostname, hostname
        );
        std::fs::write("/etc/hosts", hosts_content)?;
    }

    Ok(())
}

fn configure_users(plan: &InstallPlan, executor: &dyn CommandRunner) -> Result<()> {
    let username = plan.username.as_str();
    let password = plan.password.expose();

    println!("Configuring user: {}", username);

    // Set root password
    // echo "root:password" | chpasswd
    let root_input = format!("root:{}", password);
    let mut cmd_root = Command::new("chpasswd");
    executor.run_with_input(&mut cmd_root, &root_input)?;

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
    executor.run_with_input(&mut cmd_pass, &user_input)?;

    // Create standard XDG user directories (Desktop, Documents, etc.)
    let mut cmd_xdg = Command::new("su");
    cmd_xdg.arg("-c").arg("xdg-user-dirs-update").arg(username);
    executor.run(&mut cmd_xdg)?;

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

pub fn configure_plymouth(
    use_plymouth: bool,
    minimal_mode: bool,
    executor: &dyn CommandRunner,
) -> Result<()> {
    if !use_plymouth || minimal_mode {
        return Ok(());
    }

    println!("Configuring Plymouth...");

    if executor.dry_run() {
        println!("[DRY RUN] Setting Plymouth theme to instantos");
        println!("[DRY RUN] plymouth-set-default-theme -R instantos");
        return Ok(());
    }

    let theme = "instantos";
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
    use crate::arch::execution::mock::MockRunner;

    #[test]
    fn console_configuration_persists_font_and_keymap_and_plans_font_package() {
        let mut plan = crate::arch::engine::test_install_plan();
        plan.console_font = crate::arch::console_font::ConsoleFont::parse("ter-v32n").unwrap();
        super::configure_vconsole(&plan, &MockRunner::new()).unwrap();
        let contents =
            std::fs::read_to_string(super::target_root().join("etc/vconsole.conf")).unwrap();
        assert_eq!(contents, "KEYMAP=us\nFONT=ter-v32n\n");
        assert!(super::config_package_list(&plan).contains(&"terminus-font".to_string()));
        plan.console_font = crate::arch::console_font::ConsoleFont::default();
        assert!(super::config_package_list(&plan).contains(&"kbd".to_string()));
    }

    #[test]
    fn selected_snapshot_is_written_to_target_without_original_package() {
        let mut plan = crate::arch::engine::test_install_plan();
        let directory = tempfile::tempdir().unwrap();
        let data = crate::arch::console_font::tests::psf1();
        std::fs::write(directory.path().join("custom.psf"), &data).unwrap();
        plan.console_font = crate::arch::console_font::discover(directory.path())
            .unwrap()
            .into_iter()
            .find(|font| font.name() == "custom")
            .unwrap();
        let answer = plan.console_font.to_answer().unwrap();
        drop(directory);
        plan.console_font = crate::arch::console_font::ConsoleFont::parse(&answer).unwrap();
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
        assert_eq!(super::config_package_list(&plan), ["kbd"]);
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
        super::configure_machine_identity(&mock).unwrap();

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
