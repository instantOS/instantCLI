use super::CommandRunner;
use crate::arch::arch_linux_variant::ArchLinuxVariant;
use crate::arch::execution::pacman::Pacman;
use anyhow::{Context, Result};
use std::process::Command;

use crate::arch::config::{DesktopEnvironment, DisplayManager};
use crate::arch::engine::{InstallContext, InstallPlan, SessionAnswers, StepId, SystemInfo};
use crate::common::config_edit::{set_keys, set_keys_in_section, update_file};
use crate::ui::nerd_font::NerdFont;

/// URL for the instantOS dotfiles repository
const INSTANTOS_DOTFILES_REPO: &str = "https://github.com/instantOS/dotfiles";

struct SetupOptions {
    username: Option<String>,
    desktop: DesktopEnvironment,
    display_manager: DisplayManager,
    use_plymouth: bool,
    autologin: bool,
    use_xorg: bool,
    minimal_mode: bool,
    system_info: SystemInfo,
    linux_variant: ArchLinuxVariant,
}

impl SetupOptions {
    fn from_wizard(context: &InstallContext, override_user: Option<String>) -> Result<Self> {
        // Parsed exactly as for a fresh install; setup never asks about disk
        // encryption, so autologin defaults to off.
        let linux_variant = ArchLinuxVariant::detect().context("could not detect the variant")?;
        let session = SessionAnswers::from_context(context, false, linux_variant)?;

        Ok(Self {
            username: override_user.or_else(|| context.get_answer(&StepId::Username).cloned()),
            desktop: session.desktop,
            display_manager: session.display_manager,
            use_plymouth: session.use_plymouth,
            autologin: session.autologin,
            use_xorg: session.use_xorg,
            minimal_mode: session.minimal_mode,
            system_info: context.system_info.clone(),
            linux_variant,
        })
    }

    fn from_install_plan(plan: &InstallPlan) -> Self {
        let linux_variant = plan.linux_variant;
        Self {
            username: Some(plan.username.as_str().to_owned()),
            desktop: plan.desktop,
            display_manager: plan.display_manager,
            use_plymouth: plan.use_plymouth && linux_variant.supports_plymouth_configuration(),
            autologin: plan.autologin,
            use_xorg: plan.use_xorg,
            minimal_mode: plan.minimal_mode,
            system_info: plan.system_info.clone(),
            linux_variant,
        }
    }
}

/// Convert an existing installation to instantOS. Artix also needs the init
/// packages for the services enabled below; fresh installs get those in Config.
pub async fn setup_instantos(
    context: &InstallContext,
    executor: &dyn CommandRunner,
    override_user: Option<String>,
) -> Result<()> {
    let options = SetupOptions::from_wizard(context, override_user)?;
    if !options.linux_variant.supports_offline_installation()
        && crate::arch::offline::mode().is_offline()
    {
        anyhow::bail!(
            "Offline installation is not supported on {}. Use a network-connected Artix system.",
            options.linux_variant.label()
        );
    }

    // Fresh installations prepare standard repositories in Config. Conversion
    // owns that work here, before installing service integrations or instantOS.
    if !options.minimal_mode {
        // Enable multilib for 32-bit support (Steam, Wine, etc.)
        // This is idempotent - only enables if not already enabled
        println!("Enabling the native 32-bit repository...");
        Pacman::for_variant(options.linux_variant)
            .enable_32bit_repositories(executor.dry_run())
            .await?;

        // Artix reaches the Arch `extra`/`multilib` repositories through
        // `artix-archlinux-support`; instantOS packages depend on them.
        if options.linux_variant.is_artix() {
            Pacman::for_variant(options.linux_variant)
                .enable_arch_repository_support(executor)
                .await?;
        }
    }

    if options.linux_variant.is_artix() {
        ensure_artix_init_service_packages(&options, executor)?;
    }
    setup_instantos_with_options(&options, executor).await
}

pub async fn setup_instantos_for_install(
    plan: &InstallPlan,
    executor: &dyn CommandRunner,
) -> Result<()> {
    let options = SetupOptions::from_install_plan(plan);
    setup_instantos_with_options(&options, executor).await
}

async fn setup_instantos_with_options(
    options: &SetupOptions,
    executor: &dyn CommandRunner,
) -> Result<()> {
    println!("Setting up instantOS...");

    let minimal_mode = options.minimal_mode;

    if !minimal_mode {
        // Set up instantOS repository and install instantOS packages
        setup_instant_repo(options.linux_variant, executor).await?;
        install_instant_packages(options, executor)?;

        // Configure Plymouth theme (after instantOS packages are installed)
        super::config::configure_plymouth(
            options.use_plymouth,
            minimal_mode,
            options.linux_variant,
            executor,
        )?;

        // Update /etc/os-release to identify as instantOS
        update_os_release(executor)?;

        // Configure the GRUB theme after its package is installed. This also
        // refreshes an existing grub.cfg even if GRUB_THEME was written by an
        // earlier installer step before the theme files existed.
        crate::arch::execution::bootloader::configure_grub_theme(executor)?;
    }

    let username = options.username.clone();

    // This reuses the same functions as ins arch install for consistency
    println!("Configuring user groups...");
    super::config::ensure_groups_exist(executor)?;
    if let Some(user) = username.as_ref() {
        super::config::add_user_to_groups(user, executor)?;

        // Create standard XDG user directories (Desktop, Documents, etc.)
        println!("Creating XDG user directories for {}...", user);
        let mut cmd_xdg = Command::new("su");
        cmd_xdg.arg("-c").arg("xdg-user-dirs-update").arg(user);
        executor.run_best_effort(&mut cmd_xdg, "XDG user directory creation");
    } else {
        println!("No username provided, skipping user group membership.");
    }

    if !minimal_mode {
        if let Some(user) = username.clone() {
            setup_user_dotfiles(&user, executor)?;
            setup_wallpaper(&user, executor);
        } else {
            println!("Skipping dotfiles setup: No user specified and SUDO_USER not found.");
        }
    }

    setup_backlight_udev_rule(executor)?;
    enable_services(executor, options)?;
    super::config::configure_environment(executor)?;

    Ok(())
}

fn ensure_artix_init_service_packages(
    options: &SetupOptions,
    executor: &dyn CommandRunner,
) -> Result<()> {
    let packages = super::services::artix_init_service_packages(
        options.linux_variant,
        options.desktop,
        options.display_manager,
        options.minimal_mode,
        options.system_info.vm_type.as_deref(),
    );
    if packages.is_empty() {
        return Ok(());
    }

    println!(
        "Ensuring Artix init service packages: {}",
        packages.join(", ")
    );
    let package_refs: Vec<&str> = packages.iter().map(String::as_str).collect();
    Pacman::for_variant(options.linux_variant).install(&package_refs, executor)
}

/// Set up the instantOS repository in pacman.conf.
/// Note: This does NOT enable multilib. For fresh installations, multilib is enabled
/// during the Config step. For `ins arch setup` on existing systems, users already
/// have their own multilib configuration.
async fn setup_instant_repo(
    linux_variant: ArchLinuxVariant,
    executor: &dyn CommandRunner,
) -> Result<()> {
    println!("Setting up instantOS repository...");
    let offline_content = crate::arch::offline::instant_mirrorlist_override();
    Pacman::for_variant(linux_variant)
        .setup_instant_repo(executor.dry_run(), offline_content.as_deref())
        .await?;

    println!("Updating repositories...");
    let mut cmd = Command::new("pacman");
    cmd.arg("-Sy");
    executor.run(&mut cmd)?;

    Ok(())
}

/// Install instantOS packages from the [instant] repository.
///
/// Fresh installations get standard packages in Config. Existing Artix
/// systems additionally need their init integration packages before setup.
fn install_instant_packages(options: &SetupOptions, executor: &dyn CommandRunner) -> Result<()> {
    let mut packages =
        crate::arch::execution::packages::build_instant_package_plan(options.minimal_mode);
    if options.use_xorg {
        packages.push("xorg-server".to_string());
    }
    if !options.minimal_mode && options.system_info.has_bluetooth {
        println!("Bluetooth adapter detected, adding bluetooth management");
        packages.push("blueman".to_string());
    }
    if packages.is_empty() {
        println!("Minimal mode enabled, skipping instantOS packages.");
        return Ok(());
    }
    println!("Installing packages: {}", packages.join(", "));
    let package_refs: Vec<&str> = packages.iter().map(|s| s.as_str()).collect();
    Pacman::for_variant(options.linux_variant).install(&package_refs, executor)?;
    Ok(())
}

fn setup_user_dotfiles(username: &str, executor: &dyn CommandRunner) -> Result<()> {
    println!("Setting up dotfiles for user: {}", username);

    let check_cmd_str = "ins dot repo list";
    let mut cmd_check = Command::new("su");
    cmd_check.arg("-c").arg(check_cmd_str).arg(username);

    let repo_exists = if let Some(output) = executor.run_with_output(&mut cmd_check)? {
        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout.contains("dotfiles")
    } else {
        false
    };

    if !repo_exists {
        // Clone dotfiles. Offline installs clone the bundled snapshot and
        // record the canonical URL as origin so a later online
        // `ins dot update` still pulls from the network. The snapshot is
        // copied into the target before this runs (see
        // `offline::copy_dotfiles_snapshot`). Dotfiles are never optional:
        // opportunistic mode falls back to the network when the snapshot
        // does not work, and if no source succeeds the install fails.
        let mode = crate::arch::offline::mode();
        let clone = |url: &str, origin: Option<&str>| -> Result<()> {
            let origin_flag = origin
                .map(|origin_url| format!(" --origin {origin_url}"))
                .unwrap_or_default();
            let mut cmd = Command::new("su");
            cmd.arg("-c")
                .arg(format!("ins dot repo clone {url} --read-only{origin_flag}"))
                .arg(username);
            executor.run(&mut cmd)
        };

        match mode {
            crate::arch::offline::Mode::Online => clone(INSTANTOS_DOTFILES_REPO, None)?,
            crate::arch::offline::Mode::Strict => {
                clone(
                    crate::arch::offline::DOTFILES_SNAPSHOT,
                    Some(INSTANTOS_DOTFILES_REPO),
                )?;
            }
            crate::arch::offline::Mode::Opportunistic => {
                if clone(
                    crate::arch::offline::DOTFILES_SNAPSHOT,
                    Some(INSTANTOS_DOTFILES_REPO),
                )
                .is_err()
                {
                    println!(
                        "{} Snapshot clone failed; falling back to a network clone from {INSTANTOS_DOTFILES_REPO}.",
                        NerdFont::Warning
                    );
                    clone(INSTANTOS_DOTFILES_REPO, None)?;
                }
            }
        }
    } else {
        println!("Dotfiles repository already exists, skipping clone.");
    }

    // Apply dotfiles
    // su -c "ins dot apply" username
    let apply_cmd_str = "ins dot apply";
    let mut cmd_apply = Command::new("su");
    cmd_apply.arg("-c").arg(apply_cmd_str).arg(username);

    executor.run(&mut cmd_apply)?;

    let mut cmd_chsh = Command::new("chsh");
    cmd_chsh.arg("-s").arg("/bin/zsh").arg(username);

    executor.run(&mut cmd_chsh)?;

    // Run the same synchronous bootstrap as the user's first terminal so
    // plugin managers and their caches are ready before the first login.
    // A login su sets HOME and starts in the target user's home directory;
    // use sh to launch exactly one interactive zsh after changing the shell.
    if crate::arch::offline::mode() == crate::arch::offline::Mode::Strict {
        println!(
            "{} Strict offline install: skipping zsh initialization, which may download plugins. It will run when you first open a terminal.",
            NerdFont::Warning
        );
    } else {
        println!("Initializing zsh for user: {}", username);
        let mut cmd_zsh = Command::new("su");
        cmd_zsh
            .arg("--login")
            .arg("--shell")
            .arg("/bin/sh")
            .arg("--command")
            .arg("/bin/zsh -ic exit")
            .arg(username);
        if !executor.run_best_effort(&mut cmd_zsh, "Zsh initialization") {
            println!(
                "{} Zsh setup could not finish; opening a terminal after installation will run it again.",
                NerdFont::Warning
            );
        }
    }

    Ok(())
}

/// Best-effort nicety: the command falls back through the available wallpaper
/// sources, but if every source is unreachable the failure must still not
/// abort the installation. The user can pick a wallpaper from the instant
/// settings afterwards.
fn setup_wallpaper(username: &str, executor: &dyn CommandRunner) {
    // The wallpaper picker is a best-effort download; offline installs skip
    // it outright rather than wait for timeouts on every candidate source.
    if crate::arch::offline::mode().is_offline() {
        println!(
            "{} Offline install: skipping wallpaper download; you can pick one from the instant settings later.",
            NerdFont::Warning
        );
        return;
    }

    println!("Setting up wallpaper for user: {}", username);

    let wallpaper_cmd_str = "ins wallpaper random";
    let mut cmd = Command::new("su");
    cmd.arg("-c").arg(wallpaper_cmd_str).arg(username);

    if !executor.run_best_effort(&mut cmd, "Random wallpaper download") {
        println!(
            "{} You can set a wallpaper yourself later from the instant settings.",
            NerdFont::Warning
        );
    }
}

fn enable_services(executor: &dyn CommandRunner, options: &SetupOptions) -> Result<()> {
    println!("Enabling services...");

    let linux_variant = options.linux_variant;

    let mut services: Vec<&str> = super::services::required_system_services(linux_variant)
        .iter()
        .map(|service| service.service_name)
        .collect();
    let best_effort_services: Vec<&str> =
        super::services::vm_guest_service(linux_variant, options.system_info.vm_type.as_deref())
            .into_iter()
            .map(|service| service.service_name)
            .collect();

    if linux_variant.is_artix()
        && matches!(
            options.system_info.vm_type.as_deref(),
            Some("vmware" | "oracle")
        )
    {
        println!(
            "Warning: Guest service integration for this VM is unavailable on {}; skipping it.",
            linux_variant.label()
        );
    }

    let selected_dm = options.display_manager;
    let selected_dm_service = selected_dm.answer_value();

    // Check if other display managers are enabled. The abstraction knows how
    // each init records that, so this works outside systemd too.
    let mut other_dm_enabled = false;

    for check_dm in &["sddm", "gdm", "lightdm"] {
        if *check_dm == selected_dm_service {
            continue;
        }
        if !executor.dry_run() && super::services::service_enabled(linux_variant, check_dm)? {
            println!("Detected enabled display manager: {}", check_dm);
            other_dm_enabled = true;
            break;
        }
    }

    if !other_dm_enabled && !options.minimal_mode && options.desktop.requires_display_manager() {
        match selected_dm {
            crate::arch::config::DisplayManager::Gdm => {
                services.push(selected_dm_service);
                configure_gdm_session(options, executor)?;
                if options.autologin {
                    configure_gdm_autologin(options, executor)?;
                }
            }
            crate::arch::config::DisplayManager::Lightdm => {
                services.push(selected_dm_service);
                configure_lightdm_session(options, executor)?;
                if options.autologin {
                    configure_lightdm_autologin(options, executor)?;
                }
            }
            crate::arch::config::DisplayManager::None => {
                println!(
                    "{} No display manager selected. The system will boot to a text console.",
                    NerdFont::Warning
                );
                println!(
                    "   You will need to start your GUI session yourself after login (e.g. by running the compositor directly)."
                );
            }
        }
    } else if other_dm_enabled {
        println!(
            "Skipping {} setup because another display manager is enabled.",
            selected_dm_service
        );
    } else if options.minimal_mode {
        println!("Skipping display manager setup because minimal mode is enabled.");
    } else {
        println!("Skipping display manager setup because no graphical desktop was selected.");
    }

    super::services::enable_services(linux_variant, &services, &best_effort_services, executor)?;

    // The user-level SSH agent socket is a systemd user unit; Artix has no
    // equivalent to enable here.
    if linux_variant.is_systemd() {
        println!("Enabling user-level ssh-agent.socket...");
        let mut cmd = Command::new("systemctl");
        cmd.arg("--global").arg("enable").arg("ssh-agent.socket");
        executor.run(&mut cmd)?;
    }

    Ok(())
}

fn update_os_release(executor: &dyn CommandRunner) -> Result<()> {
    println!("Updating /etc/os-release...");

    if executor.dry_run() {
        println!("[DRY RUN] Update /etc/os-release with instantOS values");
        return Ok(());
    }

    let path = "/etc/os-release";
    if !std::path::Path::new(path).exists() {
        println!("Warning: /etc/os-release not found");
        return Ok(());
    }

    let changed = update_file(path, |content| {
        set_keys(
            content,
            &[
                ("NAME", "\"instantOS\""),
                ("PRETTY_NAME", "\"instantOS\""),
                // os-release(5) allows quoting, but ID and ID_LIKE are
                // token-like values that consumers commonly exact-match
                // unquoted (`grep ^ID=`), so write them bare.
                ("ID", "instantos"),
                ("ID_LIKE", "arch"),
            ],
        )
    })?;

    if changed {
        println!("Updated /etc/os-release");
    } else {
        println!("/etc/os-release already up to date");
    }

    Ok(())
}

fn configure_lightdm_session(options: &SetupOptions, executor: &dyn CommandRunner) -> Result<()> {
    let desktop = options.desktop;
    let Some(session_name) = desktop.session_name() else {
        return Ok(());
    };

    println!("Configuring LightDM default session to {}...", session_name);

    if executor.dry_run() {
        println!(
            "[DRY RUN] Set LightDM user-session and autologin-session to {}",
            session_name
        );
        return Ok(());
    }

    let config_path = "/etc/lightdm/lightdm.conf";
    if !std::path::Path::new(config_path).exists() {
        println!(
            "Warning: {} not found, cannot configure LightDM defaults",
            config_path
        );
        return Ok(());
    }

    let changed = update_file(config_path, |content| {
        set_keys_in_section(
            content,
            "Seat:*",
            &[
                ("user-session", session_name),
                ("autologin-session", session_name),
            ],
        )
    })?;

    if changed {
        println!("Updated lightdm.conf with default session settings");
    } else {
        println!("lightdm.conf already configured for the selected session");
    }

    Ok(())
}

fn configure_lightdm_autologin(options: &SetupOptions, executor: &dyn CommandRunner) -> Result<()> {
    println!("Configuring LightDM autologin...");

    let username = options
        .username
        .as_deref()
        .context("Username not set for autologin")?;
    let session_name = options.desktop.session_name();

    if executor.dry_run() {
        println!("[DRY RUN] Enable autologin for user: {}", username);
        if let Some(session_name) = session_name {
            println!(
                "[DRY RUN] Set LightDM autologin-session to {}",
                session_name
            );
        }
        return Ok(());
    }

    let config_path = "/etc/lightdm/lightdm.conf";
    if !std::path::Path::new(config_path).exists() {
        println!(
            "Warning: {} not found, cannot configure autologin",
            config_path
        );
        return Ok(());
    }

    let changed = update_file(config_path, |content| {
        let mut keys = vec![
            ("autologin-user", username),
            ("autologin-user-timeout", "0"),
        ];
        if let Some(session_name) = session_name {
            keys.push(("autologin-session", session_name));
        }
        set_keys_in_section(content, "Seat:*", &keys)
    })?;

    if changed {
        println!("Updated lightdm.conf with autologin settings");
    } else {
        println!("lightdm.conf already configured or keys not found");
    }

    Ok(())
}

fn setup_backlight_udev_rule(executor: &dyn CommandRunner) -> Result<()> {
    println!("Configuring backlight udev rules...");

    if executor.dry_run() {
        println!("[DRY RUN] Create /etc/udev/rules.d/90-backlight.rules");
        return Ok(());
    }

    let rules_path = "/etc/udev/rules.d/90-backlight.rules";
    let rules_content = r#"ACTION=="add", SUBSYSTEM=="backlight", RUN+="/bin/chgrp video $sys$devpath/brightness", RUN+="/bin/chmod g+w $sys$devpath/brightness""#;

    if let Some(parent) = std::path::Path::new(rules_path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(rules_path, rules_content)?;
    println!("Created {}", rules_path);

    // udevadm can fail inside chroot; the rules file is already in place
    let mut cmd = Command::new("udevadm");
    cmd.arg("control").arg("--reload-rules");
    executor.run_best_effort(&mut cmd, "udev rule reload");

    let mut cmd_trigger = Command::new("udevadm");
    cmd_trigger.arg("trigger");
    executor.run_best_effort(&mut cmd_trigger, "udev trigger");

    Ok(())
}

fn configure_gdm_session(options: &SetupOptions, executor: &dyn CommandRunner) -> Result<()> {
    let desktop = options.desktop;
    let session_name = if options.use_xorg {
        desktop.session_name()
    } else {
        desktop.gdm_session_name()
    };
    let Some(session_name) = session_name else {
        return Ok(());
    };
    let username = options
        .username
        .as_deref()
        .context("Username not set for GDM session configuration")?;

    println!(
        "Configuring GDM default session for {} to {}...",
        username, session_name
    );

    if executor.dry_run() {
        println!(
            "[DRY RUN] Set GDM user session for {} to {} in AccountsService",
            username, session_name
        );
        return Ok(());
    }

    let dir_path = "/var/lib/AccountsService/users";
    let file_path = format!("{}/{}", dir_path, username);

    std::fs::create_dir_all(dir_path)?;

    let content = if std::path::Path::new(&file_path).exists() {
        std::fs::read_to_string(&file_path)?
    } else {
        String::new()
    };

    let edit = set_keys_in_section(&content, "User", &[("Session", session_name)]);
    if edit.changed {
        std::fs::write(&file_path, edit.content)?;
    }
    Ok(())
}

fn configure_gdm_autologin(options: &SetupOptions, executor: &dyn CommandRunner) -> Result<()> {
    println!("Configuring GDM autologin...");

    let username = options
        .username
        .as_deref()
        .context("Username not set for GDM autologin")?;

    if executor.dry_run() {
        println!(
            "[DRY RUN] Enable GDM autologin for user: {} in /etc/gdm/custom.conf",
            username
        );
        return Ok(());
    }

    let config_path = "/etc/gdm/custom.conf";
    if !std::path::Path::new(config_path).exists() {
        println!(
            "Warning: {} not found, cannot configure GDM autologin",
            config_path
        );
        return Ok(());
    }

    let changed = update_file(config_path, |content| {
        set_keys_in_section(
            content,
            "daemon",
            &[
                ("AutomaticLoginEnable", "true"),
                ("AutomaticLogin", username),
            ],
        )
    })?;

    if changed {
        println!("Updated custom.conf with GDM autologin settings");
    } else {
        println!("custom.conf already configured for GDM autologin");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arch::arch_linux_variant::{ArchLinuxVariant, ArtixInitSystem};
    use crate::arch::execution::mock::MockRunner;

    fn artix_options(init: ArtixInitSystem) -> SetupOptions {
        SetupOptions {
            username: Some("test-user".to_owned()),
            desktop: DesktopEnvironment::Tty,
            display_manager: DisplayManager::Gdm,
            use_plymouth: false,
            autologin: false,
            use_xorg: false,
            minimal_mode: false,
            system_info: SystemInfo::default(),
            linux_variant: ArchLinuxVariant::Artix(init),
        }
    }

    #[test]
    fn artix_enable_services_uses_rc_update_and_skips_systemd_units() {
        let executor = MockRunner::new();
        let options = artix_options(ArtixInitSystem::OpenRC);

        enable_services(&executor, &options).unwrap();

        let log = executor.command_log();
        for expected in [
            "rc-update add NetworkManager default",
            "rc-update add sshd default",
            "rc-update add chrony default",
        ] {
            assert!(log.contains(&expected.to_string()), "{log:?}");
        }
        assert!(
            !log.iter()
                .any(|command| command.contains("systemd-timesyncd"))
        );
        assert!(
            !log.iter()
                .any(|command| command.contains("ssh-agent.socket"))
        );
    }

    #[test]
    fn artix_setup_forces_plymouth_off() {
        let mut plan = crate::arch::engine::test_install_plan();
        plan.linux_variant = ArchLinuxVariant::Artix(ArtixInitSystem::OpenRC);
        plan.use_plymouth = true;

        let options = SetupOptions::from_install_plan(&plan);

        assert!(!options.use_plymouth);
        assert_eq!(options.linux_variant, plan.linux_variant);
    }
}
