# Installation environments

Run `ins arch install` for the interactive installer. On the instantOS live ISO
or a running Arch-family system, it uses the local installation tools. On an
x86_64 foreign Linux distribution such as Ubuntu or Debian, it automatically
creates an authenticated Arch bootstrap environment and runs the same installer
there. Select Minimal Mode for vanilla Arch instead of instantOS.

## Reuse an existing home partition

In manual partitioning, choose **Keep an existing partition** at the
`/home` prompt. The installer can reuse a separate, unencrypted ext2, ext3,
ext4, or XFS partition on the selected disk. The review lists the action for
each selected device. Root and boot are still formatted. The new system mounts
the kept partition at `/home` and skips applying the default instantOS dotfiles
and wallpaper. Use the same username if you want to reuse its existing home
directory. Check that the new account's numeric UID and GID match the files.

For an unattended install, set `HomePartition = "keep:/dev/nvme0n1p3"`.
Omit `HomePartition` or set it to `"root"` to use the new root filesystem.
A plain partition path selects formatting as ext4.

## Unattended install configurations

Run `ins arch --skill` for the authoring guide. Generate a starter config on the
machine that will run the installer so its boot mode and hardware are recorded.

```sh
(umask 077; ins arch config example > questions.toml)
ins arch config explain PartitioningMethod
```

Edit the disk and credentials, then run `ins arch check -f questions.toml`.
The check validates the config and prints the resolved storage operation without
executing install steps. It uses the current environment's timezone database and
`/etc/locale.gen`. Manual partitioning also checks the size of the selected boot
partition. Add `--target` to require all named block devices to exist. Use
`ins arch config explain` for the full field list.

Run `ins arch dualboot info --output json` on the install host to discover whole
disk targets, existing partitions, OS hints, free space, and resize options.
The probe is read-only. If `target_error` or `free_space_error` reports a device
permission problem, rerun it through `sudo`. OS names are inferred from
filesystem metadata. Disk availability and resize bounds describe the current
probe and can change before installation starts.
If the partition table probe fails, free space is unknown and the installer
requires a successful probe before attempting dual-boot partitioning.

To use existing free space, set `PartitioningMethod = "dual_boot"` and
`DualBootPartition = "__free_space__"`. To shrink a partition, set
`DualBootPartition` to its device, `DualBootInstructions = "auto"`, and
`DualBootSize` to the chosen byte count as a string. Review the proposed disk,
partition, and size before running the install.

After reviewing the plan, use `sudo ins arch exec -f questions.toml` to install.
Foreign hosts support complete installations. Individual execution steps need a
persistent Arch environment. `exec --dry-run` previews the execution steps.

## Host requirements

- x86_64 Linux, root access, working internet and sufficient space in `/var/tmp`
  for the downloaded archive, extracted bootstrap and installation tools.
- A compatible `ins` binary: a static musl build or a GNU build compatible with
  the host's glibc. A binary built on a newer distribution can require a newer
  glibc than the source OS provides.
- GnuPG, tar with zstd support, and util-linux mount/namespace tools. On Ubuntu
  and Debian: `sudo apt install gnupg tar zstd util-linux mount coreutils`.
- Hosts with mounted btrfs filesystems also need `btrfs-progs` so every member
  device, including multi-device filesystems, can be protected.

Missing prerequisites are reported before installation. Pacman and
arch-install-scripts packages from the host distribution are not used. Their
versions and keyrings can lag behind current Arch packages; the bootstrap
provides a single, current Arch toolchain and trust database.

The interactive installer checks HTTPS connectivity and offers network
configuration on the source host before preparing Arch, using `nmtui`,
`connmanctl`, or `iwctl` in that order of preference. If none is available,
configure the connection using the host's network settings and select Check again.
The bootstrap shares that connection; it does not run a network configuration
UI or expose the host's NetworkManager sockets. If connectivity fails during
preparation or installation, fix it on the source host and rerun the same
command. Saved answers remain available; execution restarts as described below.

## Target selection and lifecycle

The target must be a separate, unused disk. The bootstrap records the source's
mounted devices and their block-device ancestors before changing environments,
including LVM/MD parents, swap and btrfs members. Device identities protect
aliases as well as ordinary `/dev` paths. Unmount an unused spare disk on the
source OS before launching the installer. Installing over the running system
requires the live ISO. Overlay/container/network roots with no identifiable
backing block devices are refused.

Disk validation, opening cfdisk, and disk preparation also perform a live
Linux exclusive-open probe. This refuses disks claimed by mounted partitions,
swap or device mappings in any mount namespace, including claims made after
the initial source snapshot. The probe is released before the installer's own
tools claim the disk; it is not a reservation for the installation's duration.
Do not mount, activate or otherwise use the target concurrently on the host.

Answers, logs and execution state use the source's private `/run/ins-install`
directory. They remain available after the bootstrap exits and disappear when
the host reboots. Each fresh bootstrap starts fresh execution state, since disk
mounts from a previous namespace cannot be resumed. Retrying an interrupted
installation reruns its disk preparation; it does not resume partially
completed package installation. A completed installation with the same identity
is still detected before disk writes.

UEFI installations from a running system use GRUB's standard removable-media
fallback path with `--no-nvram`; the installer does not change the source
machine's firmware boot order. Choose the target disk in the firmware boot
menu. BIOS installations write GRUB to the selected target disk's MBR.
Reboot/shutdown requests from the finished menu run on the source only after
the bootstrap has released its resources.

## Implementation boundaries

`arch/bootstrap/download.rs` discovers an available dated release through
Arch's HTTPS release API, fetches its archive from the Arch mirror and its
detached signature from archlinux.org, and verifies the signature before
extracting anything. GnuPG uses a private keyring containing the bundled release
key, pinned to full primary fingerprint
`3E80CA1A8B89F69CBA57D98A76A5EF9054449A5C`. A signing-key change fails closed
and requires updating the bundled key and pin after independently verifying
the new release signer. The key comes from Pierre Schmitz's release signing
identity documented on [Arch's download page](https://archlinux.org/download/).

The launcher owns the temporary directory in the original mount namespace.
A supervisor runs as PID 1 in a private mount/PID namespace. It binds the
bootstrap root onto itself, mounts procfs, exposes `/dev`, and binds `/sys`
read-only without its efivarfs submount. It shares only the installer state
directory and read-only udev/kernel-module directories; the source's systemd
and D-Bus sockets are not exposed. Networking uses the host's network namespace
and a copied resolver configuration.

The bootstrap is an installation source, distinct from the target chroot under
`/mnt`. An explicit root marker prevents the existing chroot detector from
confusing those environments. The supervisor records created swap, encryption
mappings and volume groups, stops installer descendants, unmounts the target
and releases those resources on success, error, SIGINT or SIGTERM. Source swap
is excluded from the target fstab. Existing encryption resource names are
refused before disk preparation.

Resource journal updates are published by atomic rename, so interrupting a
registration leaves either the previous complete journal or the new complete
journal available for cleanup.

Only the outer launcher removes the temporary directory, after namespace
execution ends. It never traverses the namespace's bind mounts during removal.
The supervisor performs no recursive directory deletion.

Pure installer tests live in instantCLI. An ignored test,
`a_later_mount_in_another_namespace_refuses_the_whole_disk`, verifies the live
claim against a disposable loop disk mounted in a separate namespace. Run it
as root with loop-device support, util-linux and e2fsprogs installed. Full disk,
interruption, preservation and standalone-boot tests live in the `host-ubuntu` flow of
[instantOS-e2e](https://github.com/instantOS/instantOS-e2e).
