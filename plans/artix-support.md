# Artix support

## Scope

Support Arch Linux with systemd and Artix Linux with OpenRC, runit, s6, or dinit. `ins arch install` can run from supported live media or a running supported system onto another disk. `ins arch setup` converts an existing supported system to instantOS. The minimal profile keeps the base distribution; the full profile adds instantOS.

CPU architecture, distribution/init variant, installation profile, and installation source are separate concepts. See [GLOSSARY.md](../GLOSSARY.md) and the [terminology audit](artix-terminology-audit.md).

## Variant detection

`src/arch/arch_linux_variant.rs` defines `ArchLinuxVariant`, `ArtixInitSystem`, and `InitSystem`. The variant combines base distribution and init while excluding unsupported combinations. Read os-release and resolve `/sbin/init` below the inspected root rather than querying PID 1: artix-chroot uses a PID namespace. Absolute symlinks stay within that root. Missing or unknown init executables are errors; installed utilities do not select the init.

- Arch/instantOS with systemd selects Arch.
- Artix/instantOS with a supported non-systemd init selects that Artix variant.
- Unsupported distributions and Arch/non-systemd or Artix/systemd combinations are errors.
- Tests supply the variant explicitly when constructing plans. Development/E2E runs can set `INS_ARCH_LINUX_VARIANT=arch|artix-openrc|artix-runit|artix-s6|artix-dinit`.
- Summaries show Unknown when detection cannot establish a variant.
- Before chroot re-entry, verify that the mounted target matches the plan.

The variant owns native/additional repository names, bootstrap/chroot/fstab commands, signing keyring selection, bootstrap init packages, init service package names, installer capabilities, and encrypted-root kernel parameters.

## Package sources and bootstrap

`HostProfile` identifies disposable live media versus a running source system. `PackageSource` uses in-place configuration only on disposable media or inside the target. A running source gets an isolated pacman configuration and mirrorlist; its own package configuration is not rewritten.

Use `bootstrap_packages` to invoke pacstrap on Arch or basestrap on Artix. Both support the isolated `-C`/`-M` flow. Bootstrap installs the selected init explicitly on Artix together with its elogind integration, kernel, firmware, and Artix mirrorlist. Fstab/chroot use genfstab/arch-chroot or fstabgen/artix-chroot.

Pacman constructors bind the current system or an explicit variant/root. Detection errors never silently default to Arch. Retry recovery can run reflector only for Arch; Artix retains its native mirror servers.

Enable 32-bit packages through multilib on Arch and lib32 on Artix. Full instantOS on Artix also needs `artix-archlinux-support`, populated Arch signing keys, and the additional Arch extra/multilib repositories using mirrorlist-arch. Minimal Artix does not enable that compatibility setup.

## Mirror regions

archlinux.org region selection applies only to Arch. Artix keeps its source mirrorlist. Region providers publish names and country codes; no choices means an empty list, rather than a second failed/not-applicable flag. The wizard waits for provider data before checking availability and asks only when there are choices. `is_relevant` governs answer relevance, while `is_available` governs whether loaded data offers a question; unavailable choices do not invalidate saved answers. Plan construction filters stale region answers from an Artix plan.

## Target configuration

- Generate matching /etc/machine-id and D-Bus identity without systemd tools on Artix.
- Use zoneinfo/hwclock and locale configuration directly where systemd tools are unavailable or inside a chroot.
- Write native OpenRC hostname/keymap/font settings in addition to shared configuration files.
- Use busybox/udev/keymap/consolefont/encrypt/lvm2/resume initramfs hooks on Artix and systemd hooks on Arch where required by the existing installer policy.
- Select the corresponding cryptdevice or rd.luks kernel parameter for encrypted root.
- The installer does not implement Artix Plymouth configuration or Artix offline bundles. These are installer capability limits, not intrinsic OS restrictions.
- Artix LTS Nvidia uses nvidia-open-dkms and the matching headers because the Arch prebuilt LTS package is unavailable there.

## Services and conversion

`SystemService` associates a service name with its daemon package. Required system services, display-manager integration, and optional VM integration share package requirements between fresh install planning and conversion. Artix installs separate init service packages.

NetworkManager is installed and enabled on every supported variant. Artix adds networkmanager-openrc/runit/s6/dinit; it does not replace NetworkManager with ConnMan. The live-media connection prompt prefers nmtui and can use connmanctl or iwctl when nmtui is absent.

- systemd: systemctl enable.
- OpenRC: rc-update add to default; consolefont uses boot explicitly when needed.
- runit: link the service into the default runsvdir.
- s6: require current s6-frontend. Enable each daemon's -srv service with dependency pulling, then commit/install one batch. Use live install --init inside the fresh target chroot. Query enabled state from set status, not obsolete filesystem markers.
- dinit: require dinitctl --offline enable.

There are no legacy s6 or missing-tool symlink fallbacks. The shared service API does not call systemd targets, s6 sets, or dinit dependencies runlevels.

Fresh installs prepare standard repositories/packages in Config. Conversion prepares them before shared instantOS setup. Post does not repeat that preparation. Optional guest activation remains best effort; failures of required services abort setup.

## Identity and validation

Both public installation intent and private execution-state fingerprints include the variant. Intent excludes secrets; resume binding includes the exact configuration. Completed identity schema v2 deliberately invalidates older identities.

Validate with cargo fmt, cargo check, focused/full Rust unit tests, and cargo clippy. Regression coverage includes all supported variants, package requirements, native service commands and s6 batching, mirror recovery, stale-answer normalization, initramfs/GRUB configuration, root identity checks, and variant-bound fingerprints.

Actual installation and boot tests on Artix media remain external validation.
