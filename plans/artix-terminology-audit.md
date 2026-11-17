# Artix terminology audit

Scope: all production types, fields, helpers, diagnostics, and documentation added or extended by the Artix worktree diff, including the subsequent review fixes. Existing terminology is changed only where Artix support changes its meaning. External package, command, service, DMI, and kernel names remain their real names.

## Domain boundaries

| Previous term | Decision | Meaning |
| --- | --- | --- |
| `Platform` / `platform.rs` | `ArchLinuxVariant` / `arch_linux_variant.rs` | Supported base distribution/init combination, separate from CPU architecture and installation profile. |
| `platform` fields and arguments | `linux_variant` | The selected combination carried by a plan, package source, or pacman context. |
| `INS_PLATFORM` | `INS_ARCH_LINUX_VARIANT` | Explicit testing/development override; no legacy alias. Stable variant identifiers remain `arch`, `artix-openrc`, etc. |
| `ArtixInit` | `ArtixInitSystem` | A non-systemd init supported by Artix. Keep the restricted enum so Artix/systemd cannot be constructed. |
| `InitSystem` | Retain | The actual init, independent of distribution. Its display label belongs on this type. |
| Foreign systemd host treated as Arch | Remove | Tests supply their variant explicitly. Detection rejects unsupported distributions, and summaries show Unknown rather than inventing Arch. |
| `HostProfile`, `InstallEnvironment`, `TargetRelation` | Retain | Installation source and disk safety, separate from the distribution/init choice. Correct LiveIso descriptions and labels to include Artix media. |
| `OperatingSystem::Artix` | Retain | Distribution identity, consistent with existing distro detection and family checks. |
| Minimal/full installation profile | Retain | Vanilla base system versus instantOS additions; not another distribution/init variant. |

## Package and execution language

| Previous term | Decision | Meaning |
| --- | --- | --- |
| `repositories` on the variant | `native_repositories` | Repositories belonging to the base distribution. |
| `arch_repositories` | `additional_arch_repositories` | Arch repositories added to Artix, not Arch's native repository set. |
| `init_packages` | `bootstrap_init_packages` | Explicit init package plus elogind integration. Avoid relying on the elogind package's transitive dependencies to select init. |
| `service_package` | `init_service_package` | Init integration package, not the daemon itself. |
| `Service { name, package }` | `SystemService { service_name, daemon_package }` | A system daemon's service/package pairing, not a unit file or user service. |
| `core_services`, `guest_service` | `required_system_services`, `vm_guest_service` | Required system services and optional VM integration. Avoid confusing core services with the Arch core repository. |
| `artix_service_packages` | `artix_init_service_packages` | Shared init integration requirements for installation and conversion. |
| `enable_multilib` | `enable_32bit_repositories` | Enables multilib on Arch and lib32 on Artix. |
| Arch repository bridge | Arch repository support | Compatibility packages, keys, and additional repositories. The package retains its real name, `artix-archlinux-support`. |
| `enable_arch_repos` / `enable_artix_arch_bridge` | `enable_additional_arch_repositories` / `enable_arch_repository_support` | Distinguish config editing from preparing the complete compatibility setup. |
| `enable_repository_in_string` | `uncomment_repository` | Describes exactly the supported edit. |
| `enable_arch_repositories_in_string` | `append_missing_arch_repositories` | Appends missing sections; does not reconfigure existing ones. |
| `repository_list` | `format_repository_names` | Produces display text, not a collection of repositories. |
| `pacstrap`, `run_pacstrap`, `derive_pacstrap_config` helpers | `bootstrap_packages`, `bootstrap_base_system`, `derive_bootstrap_pacman_config` | Shared operations that use pacstrap or basestrap. Actual executable names remain unchanged. |
| pacstrap-named temporary files | Bootstrap-named temporary files | Installer-owned files used by either distribution's bootstrap command. |
| `configure_host_pacman` | `configure_bootstrap_pacman` | Can configure an isolated source rather than the host's pacman files. |
| `Pacman::current`, `for_variant`, `for_target_variant` | Retain the constructor boundaries | Distinguish detected current-system operation from explicit variant/root binding. |
| `PackageSource`, `PackageSourceFiles`, `HostPacman`, isolated/in-place | Retain | Package configuration ownership/isolation, not an installation source or another distro choice. |
| Generic runlevel argument/default | Remove | systemd targets, s6 sets, and dinit dependencies are not runlevels. Shared setup enables services for the next normal boot; OpenRC's special boot-runlevel console font operation stays explicit. |
| `supports_plymouth`, `supports_offline` | `supports_plymouth_configuration`, `supports_offline_installation` | Installer capabilities, not intrinsic OS limitations. Documentation no longer claims Artix cannot use Plymouth. |
| `uses_systemd_initramfs` | Retain | Installer hook policy. A systemd runtime does not inherently require a systemd initramfs. |
| `ARTIX_TOOLS` | `ARTOOLS_BASE` | Names the real dependency package, matching `ARCH_INSTALL_SCRIPTS` convention. |

## Mirror state and small helpers

`mirror_region_for` was redundant indirection over a valid normalization rule. Plan construction now filters a stale mirror-region answer directly when building an Artix plan. The rule remains necessary when reusing saved answers.

The deeper issue was `MirrorRegionsFetchFailed`: it represented both a failed fetch and a choice that does not apply to Artix. Remove this second availability state. Providers publish an empty region list when there are no choices; `MirrorRegionQuestion::is_available` checks that list after the provider finishes. `is_relevant` governs saved-answer validity independently of provider availability. Empty-region paths also clear the region-code map.

Remove wrappers that only forward values or format a single call site:

- `SessionAnswers::for_platform`: normalize Plymouth while parsing session answers using the explicit variant.
- `build_grub_encryption_param`: call the variant's encryption-parameter method directly; keep tests of the resulting GRUB config.
- `wizard_banner`: format the banner at its only call site.
- `configure_pacman_target`: call pacman settings at the configuration phase directly.
- Setup's `artix_service_packages(options)`: call the shared service-package planner directly.
- `detect_network_tool`: call selection with the actual availability lookup at its only call site.
- `ArchLinuxVariant::init_system_label`: ask the init system for its own label.
- Pacman's variant getter: use its private field internally; it adds no public behavior.
- Generic `enable_service`: only its OpenRC console-font caller needed a different runlevel; issue that native command explicitly and keep shared service activation batched.

Keep small functions that provide a meaningful boundary: constructors bind a filesystem root/variant, capability predicates name installer policy, native service activation owns init-specific behavior, configuration editors preserve files, and DMI parsing is separate from reading hardware data. Line count alone is not a reason to inline these.

## Other added/changed terms checked

- `NetworkSetupTool` and `select_network_setup_tool` describe live-media network configuration. NetworkManager remains the installed daemon on Arch and every supported Artix init. nmtui is preferred when available; ConnMan/iwd tools are alternate ways to connect the installation source.
- Machine identity, machine-id, D-Bus identity, OpenRC conf.d, vconsole, mkinitcpio hooks, LUKS parameters, and fstab retain their established Linux meanings. Their helpers perform substantive configuration or parsing.
- `vm_type_from_dmi` and `detect_vm_type_from_dmi` retain parsing/detection boundaries. An unrecognized DMI value does not establish bare metal; correct the test wording.
- `pid1_is_installer` becomes `pid1_indicates_chroot`: a command named chroot is evidence about the environment, not proof of installer identity.
- Installation intent, configuration fingerprint, completed marker, and resume state retain distinct meanings. Both fingerprints include `linux_variant`; the public completed identity still excludes secrets.
- UI and diagnostics say Arch Linux variant for the distro/init selection, init system for its boot manager, architecture for CPU architecture, and installation profile for minimal/full selection.
- Preserve real external names, including NetworkManager, nmtui, pacstrap, basestrap, lib32, multilib, VMware Virtual Platform, and the kernel's fw_platform_size path.

Canonical domain definitions are in [GLOSSARY.md](../GLOSSARY.md). No ADR is needed: these naming and internal API changes are readily reversible.
