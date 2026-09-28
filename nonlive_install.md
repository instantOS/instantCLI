# Non-live installation for `ins arch` — feasibility report

**Date:** 2026-09-26
**Scope:** Can `ins arch` install instantOS from an already-running operating system
(Arch, instantOS, Ubuntu) instead of from the instantOS live ISO? And is the
"install a distro onto the drive the current OS is running from" trick real, or is it
genuinely hacky?

---

## 0. Verdict up front

| # | Feature | Feasible? | Rough size | Verdict |
|---|---------|-----------|------------|---------|
| A | Install to a **different disk** from a running Arch/instantOS | **Yes, easy** | Small (~1–2 days) | Mostly guard/state plumbing; the execution layer is already target-relative |
| B | Install from **Ubuntu / any foreign distro** to a different disk | **Yes** | Medium (~1–2 weeks) | The Arch toolchain is one `apt install` away; real work is bootstrap UX + footguns |
| C | Install **alongside on the same disk** as the running OS (dual boot) | **Partially** | Medium-hard | Blocked by the root-disk guard + "resize needs unmounted". Free-space-only is easy; shrinking the running root is not |
| D | Install **over the running root** (in-place replacement) | **Yes, but hard** | Large (weeks + test infra) | Real projects do this (bootc, nixos-in-place, takeover.sh). All of them use the same trick. Do it last, behind a VM harness |
| E | **tmpfs `pivot_root` self-install** ("become the rescue env, free the disk") | **Yes, very hard** | Very large | Not worth it in `ins`. The ISO already *is* a tmpfs Arch system; if you want this, the honest form is a separate `takeover`-style script |

**Short answer to the second question:** it is not a hack in the sense of being
novel or clever-bad. It is a hack in the sense of being *outside the kernel's supported
model, unrecoverable if interrupted, and always a pile of `umount -l` + `pivot_root`*.
But it is 20-year-old, documented in the ArchWiki, and shipped in production by Fedora
(`bootc install to-existing-root`) and the NixOS community (`nixos-in-place`).

**The single most valuable thing you can do first is not any of these: it is a
disk-level end-to-end harness** (loopback device + `questions.toml` + QEMU). See §7.0.

---

## 1. The question actually contains three different features

The phrase "install onto a drive the current OS is running from" covers three
distinct problems with wildly different difficulty. Conflating them is the main
reason this looks scary.

1. **Source ≠ target disk.** I am running Arch on `/dev/nvme0n1`, I want instantOS on
   `/dev/sdb`. Nothing about this is dangerous — the running system's files are never
   touched. This is *the same operation* the live ISO performs; the only difference is
   that `/` is a real filesystem instead of a ramdisk. **This is feature A and it is
   nearly free.**

2. **Target disk = source disk, but target partitions ≠ running partitions.** Dual boot
   on the same disk. Needs the running root to shrink or to be left alone, and needs
   the ESP to be shared. **Feature C.**

3. **Target partition = the running root partition.** You are asking the kernel to
   destroy the filesystem that your own userspace, kernel modules, open file
   descriptors and shell are executing from. **Feature D.** This is the one that
   requires the exotic tricks.

Everything in §3–§5 maps onto one of these.

---

## 2. How the installer works today, and where "live ISO" is baked in

### 2.1 The shape of the thing

`ins arch install` is a two-phase wizard/executor (`src/arch/cli/commands/install.rs:85`):

```
ensure TTY (relaunch in a GUI terminal otherwise)   install.rs:89-98
confirm_battery_power()                            install.rs:100
ensure_interactive_internet() (offers nmtui)        install.rs:107
ensure_root()  (re-exec under sudo)                 install.rs:114
gate: distro must contain "Arch"/"instantOS"        install.rs:120-131
gate: architecture must be x86_64                   install.rs:133-144
wizard  (build_steps()) -> /etc/instant/questions.toml
exec    (handle_exec_command) -> 6 execution steps
```

The six execution steps (`src/arch/execution/mod.rs:466-473`) and their chroot policy
(`src/arch/execution/step.rs:3-41`):

| Step | Runs where | What it does |
|------|-----------|--------------|
| `Disk` | host | partition, mkfs, mount under `/mnt` |
| `Base` | host | mirrors, `pacstrap /mnt …` |
| `Fstab` | host | `genfstab -U /mnt` → `/mnt/etc/fstab` |
| `Config` | **chroot** | multilib, packages, tz, locale, users, sudo, mkinitcpio |
| `Bootloader` | **chroot** | `grub-install`, `grub-mkconfig` (+ verification) |
| `Post` | **chroot** | instantOS repo, `instantes`, DE, services, dotfiles |

`Config`/`Bootloader`/`Post` are handed off by copying the current executable into the
target as `/usr/bin/ins-install` and re-running `arch-chroot /mnt /usr/bin/ins-install
arch exec <step>` (`src/arch/execution/mod.rs:602-663`, `setup_chroot` at `:713-789`).

**This architecture is the good news.** The step boundary is already "host-side work
that touches `/mnt`" vs. "target-side work that needs a chroot". Nothing about it
assumes a ramdisk. `paths::CHROOT_MOUNT = "/mnt"` is a single constant
(`src/arch/execution/paths.rs:8`).

### 2.2 Every place "live ISO" is assumed

| # | Assumption | Location | Effect off-ISO |
|---|-----------|----------|----------------|
| L1 | Installer only reachable from the live session | `src/welcome/ui.rs:156` (`is_live_session` gates the menu item); `src/autostart.rs:108` | The menu entry is simply absent on a normal system |
| L2 | Source must be Arch-family | `install.rs:120-131`; soft warning at `cli/commands/mod.rs:29-34` | Hard-refuses Ubuntu (soft-warns elsewhere) |
| L3 | `/run/archiso/cowspace` grows to 70 % RAM | `src/common/distro.rs:258-297`, called at `execution/mod.rs:372-377` | No-op. Correct. |
| L4 | Offline bundle at `/run/archiso/bootmnt/offline-repo` | `src/arch/offline.rs:27,31,37` | `mode()` returns `Online`. Correct *if* there is a network. |
| L5 | Live-only dependency bootstrap (fzf, git, gum, cfdisk, btrfs-progs, ntfsprogs, ntfs-3g) | `src/arch/cli/commands/ask.rs:153-194` | **Skipped** → see L6 |
| L6 | fzf is auto-installed only on a live ISO | `src/menu_utils/fzf/utils.rs:104-122` (mise is the only off-ISO fallback) | `ins arch install` on a plain Arch with no fzf/gum fails at the first dialog |
| L7 | `arch-chroot` is assumed present | `execution/mod.rs:616` | Present on Arch (arch-install-scripts); on Ubuntu needs `apt install arch-install-scripts` |
| L8 | The selected disk is *not* the running disk | `src/arch/questions/disk.rs:251-284` (hard error for root device **and** boot disk) | **Inert on a live ISO** (root is a tmpfs/overlay so `get_boot_disk()` returns nothing useful), **blocking on a real system.** This is the single biggest blocker. |
| L9 | Disk can be exclusively unmounted | `src/arch/questions/disk.rs:35-99` → `disks::prepare_disk` (`src/arch/disks.rs:176-200`) | Works for a foreign disk; would `umount /` for a same-disk target |
| L10 | `/mnt` is a fresh, empty, dedicated mount point | `fstab.rs:8,15`, `base.rs:248`, `disk/filesystem.rs:43-106`, `disk/mount.rs:46-50`, `disk/automatic.rs:98`, `disk/encryption.rs:121`, `finished.rs:103` | Fine for A/B; for D `/mnt` lives *on the target* which is `/` |

**The pattern is clear:** almost every live-ISO assumption is either a no-op off-ISO
(L3, L4) or a *missing gate* that needs to become a *conditional guard* (L1, L2, L5,
L6, L8, L9). None of them require rearchitecting the execution layer.

### 2.3 Two landmines that only exist off-ISO

These are the kind of thing that will bite during implementation and are worth calling
out now:

- **`/etc/instant/installdryrun` forces dry-run mode** (`execution/mod.rs:351-359`).
  On a live ISO `/etc` is a tmpfs, so this file only exists if someone put it there on
  purpose. On a real system it is a real file on the real root, and a leftover from
  any past experiment silently turns every future install into a no-op. **Scope this
  flag to live mode, or move the whole state directory.**

- **`/etc/instant/*` is shared with the source system.** `questions.toml`,
  `install_state.toml`, `upload_state.toml` and `/var/log/instantos/install.log`
  (`execution/paths.rs`) all live on the *source* root. On a live ISO that is throwaway
  RAM; on a real Arch these are the source system's own instantOS state. A non-live
  install should stage its state somewhere ephemeral (`/run/ins-install/…`) and only
  copy the final questions file into the target.

- **`get_mounted_partitions` prefix-matches** (`disks.rs:143`): `source.starts_with(disk)`.
  `/dev/sda` matches `/dev/sdaa1`. Harmless on a live ISO; on a real system with several
  disks this can produce a spurious "disk in use" abort. Worth tightening to
  `source == disk || source.starts_with(&format!("{disk}p"))` for NVMe-style names.

- **The idempotency probe can false-positive.** `find_matching_installation`
  (`execution/mod.rs:415-431`, `src/arch/installation_identity.rs:81-104`) enumerates
  Linux filesystems *under the target disk* and reads `/etc/instant/installation.toml`
  from each. If the target disk is the source disk, it will probe the **running
  instantOS system** and can report `AlreadyInstalled` for a brand-new install. Must be
  disabled for same-disk modes.

---

## 3. Feature A — install to a different disk from a running Arch

**This is the cheapest useful win, and the code is 90 % there already.**

### What already works, untouched

- Partitioning, mkfs, btrfs subvolumes, LUKS+LVM — all operate on an explicit device
  from the `StoragePlan` and have no live-ISO dependency (`src/arch/execution/disk/*`).
- `pacstrap`, `genfstab`, `arch-chroot` — present on any Arch system that has
  `arch-install-scripts`, which is a one-line pacman install.
- The chroot hand-off. `setup_chroot` copies `std::env::current_exe()` (the release
  binary at `/usr/local/bin/ins`, or the `.deb`-installed one) to
  `/mnt/usr/bin/ins-install`. Nothing about this depends on the source being a ramdisk.
- Dual-boot ESP reuse, `os-prober`, GRUB theme — unchanged.
- `--config` unattended mode (`scripts/install-src/environment.sh:110-111`) — this is
  the natural non-interactive entry point and already exists.

### What must change

1. **Relax the disk guard (L8), carefully.** `DiskQuestion::validate`
   (`questions/disk.rs:251-284`) must keep refusing the root device and boot disk
   *unless* the chosen target-relation mode explicitly allows it. Introduce an
   explicit mode rather than deleting the guard.
2. **Make `PrepareDiskStep` (L9) refuse to unmount the source root.** If the selected
   disk contains the running root, hard-fail with a clear message pointing at the
   relevant mode (§5/§6) rather than attempting `umount /` and failing obscurely.
3. **Replace `/run/archiso/*` state with an ephemeral state dir** when not live (§2.3).
4. **Add a dependency preflight for non-live hosts** (L5/L6): check `fzf`, `gum`,
   `cfdisk`, `btrfs-progs`, `ntfsprogs`, `ntfs-3g`, `arch-install-scripts`
   (`pacstrap`, `arch-chroot`, `genfstab`) *before* the wizard, and offer to install
   them with `ensure_all`. The `Dependency` machinery already exists and
   `src/common/deps.rs` already has Apt mappings for `fzf`, `git`, `cfdisk`.
5. **Surface the install menu item on non-live Arch** (L1) or at least document
   `ins arch install` as the entry point.
6. **Regenerate `machine-id` and SSH host keys in the target.** There is currently
   *zero* `machine-id` handling anywhere in the repo (`rg machine-id` → no hits). On a
   live ISO this never mattered because the target was brand new from `pacstrap`. From
   a running Arch, a naive "copy the host" step would produce two machines with the
   same identity. `pacstrap` builds a fresh root so this only bites in the
   copy-the-host variants, but `systemd-machine-id-setup` in `Config` is cheap
   insurance.
7. **Adjust the finish menu.** `finished.rs:103` runs `df -h /mnt`; fine while the
   target is still mounted, but the "reboot into instantOS" framing is wrong when the
   machine will reboot into the *old* system. Offer "reboot" / "keep using the current
   system" / "shutdown", and mention which disk to boot from.

### Effort

Small. The diff is concentrated in `install.rs`, `questions/disk.rs`,
`execution/paths.rs`, `offline.rs`, and one new preflight module. No changes to
`execution/disk/*`, `execution/base.rs`, `execution/bootloader.rs`, or the chroot
hand-off. **This is the feature to build first**, partly because it builds the
preflight/state infrastructure that B and C need.

---

## 4. Feature B — install from Ubuntu (or any foreign distro)

**Feasible, and the blocker is much smaller than it looks.**

### 4.1 The toolchain is one `apt install` away — verified

On this very machine (Ubuntu 24.04.5 LTS, noble):

```
$ apt-cache policy arch-install-scripts pacman-package-manager archlinux-keyring
arch-install-scripts:      Candidate: 28-1     (noble/universe)
pacman-package-manager:    Candidate: 6.0.2-6ubuntu2
archlinux-keyring:         Candidate: 0~20240313-1
btrfs-progs:               Installed: 6.6.3-1.1build2   (noble/main)
```

`arch-install-scripts` ships `pacstrap`, `arch-chroot` and `genfstab` — i.e. *every*
non-`ins` tool the execution layer shells out to (L7). Debian/Ubuntu have packaged
this since Buster; it is in `noble/universe` today.

### 4.2 The distro-agnostic fallback

If the host has no `pacstrap` (Void, Gentoo, a minimal container), the ArchWiki route
works from literally any distro: download the bootstrap rootfs tarball, extract, chroot.
Verified present on the official mirror:

```
https://geo.mirror.pkgbuild.com/iso/latest/archlinux-bootstrap-x86_64.tar.zst   (121 MiB)
https://geo.mirror.pkgbuild.com/iso/latest/archlinux-bootstrap-x86_64.tar.zst.sig
```

(As of 2026-09-01 the file is named `archlinux-bootstrap-*`; older docs referencing
`archlinux-base-*` are stale — there is no `archlinux-base` on the mirror anymore.)

Caveats worth knowing: the `.sig` must be fetched from `archlinux.org`, **not** from a
mirror, and verified with GnuPG; inside the bootstrap you still need
`arch-install-scripts` to get `pacstrap`. This is strictly more work than the Ubuntu
path, so it should be the fallback, not the default.

### 4.3 Real footguns (from the ArchWiki + Debian packaging)

- **`/run/shm`**: on some Debian-based hosts `/dev/shm` is a symlink to `/run/shm`,
  which does not exist in the Arch target → `pacstrap` errors with *"could not determine
  cachedir mount point"*. Fix: `mkdir -p /run/shm` in the target.
- **`pacman.conf` and pacman **hooks** leak from the host** into the target install
  (arch-install-scripts#60). Use `pacstrap -C` with a shipped canonical
  `pacman.conf` rather than inheriting the host's. `ins` already fetches/derives
  mirrorlists (`base.rs:36-112`), so shipping a canonical conf is consistent.
- **`genfstab` on SELinux hosts** emits `seclabel` options that keep Arch from booting.
  Not a Debian problem but the code should strip unknown options defensively
  (`fstab.rs:8` currently appends genfstab's stdout verbatim).
- **Debian's `pacstrap` is old** (28-1 in noble vs. 31-1 in unstable). Verify it handles
  the current `linux` package set; if not, prefer the bootstrap-tarball path.

### 4.4 The `ins` binary itself is already fine on Ubuntu

- Release artifact `ins-x86_64-unknown-linux-gnu` (glibc, dynamically linked) —
  runs on Ubuntu.
- `utils/build_deb.sh` already produces a `.deb`, and `utils/build_appimage.sh` an
  AppImage. The project already treats Ubuntu as a host platform.
- README documents `apt install` build deps for Ubuntu.
- `src/common/deps.rs` already has `PackageManager::Apt` entries for `fzf`, `git`,
  `cfdisk`. **Missing Apt entries:** `gum` (not packaged by Ubuntu — use the existing
  release-binary/AUR/cargo fallback chain the `Dependency` system already implements),
  `btrfs-progs` (present in noble/main, just unmapped), `ntfsprogs` (present).
- `acpi` → Ubuntu ships it in `acpid`/`acpm` under a different name; `install.rs:15`
  uses `.unwrap_or(false)` so it degrades to "assume plugged in". Acceptable.
- `nmtui` exists on Ubuntu via `network-manager`. Fine.

### 4.5 Effort

Medium. The work is: a bootstrap-preflight step (apt/pacman/bootstrap-tarball), a
`SourceEnvironment` concept threaded into the plan, Apt mappings for the remaining
deps, the `/run/shm` + `pacman.conf` fixes, and a decision about how much of the
existing `--config` unattended flow to expose. **The execution layer needs no
architectural change** — the Arch tools run from the Arch target or from `arch-chroot`
regardless of what the host is.

---

## 5. Feature C — alongside on the *same* disk

**Partially feasible. The interesting sub-cases have very different answers.**

### 5.1 The good news: the dual-boot machinery already exists

`StoragePlan::DualBoot` → `dualboot::prepare_dualboot_disk`
(`src/arch/execution/disk/dualboot.rs:20-183`) already:
- reuses an existing ESP ≥ 260 MiB without reformatting (`:76-89`),
- optionally shrinks an existing NTFS/ext partition (`:185-339`),
- then formats and mounts the new partitions,
- and installs GRUB with `os-prober` so the old OS stays bootable.

So the *partitioning and bootloader* half of "dual boot on the same disk" is already
written. What is missing is the ability to be *running from* that disk.

### 5.2 The blocker: `umount` then shrink

```rust
// src/arch/execution/disk/dualboot.rs:283-308
if let Some(mount_point) = &partition.mount_point {
    executor.run(Command::new("umount").arg(mount_point))?;
}
match fs_type {
    "ntfs" => ntfsresize --force --size <n> <part>,
    "ext4"|"ext3"|"ext2" => { e2fsck -f <part>; resize2fs <part> <n>K }
}
```

`resize2fs` **cannot shrink a mounted filesystem** ("On-line shrinking not supported"),
and you cannot unmount `/`. Also note the resize path only accepts `ntfs|ext2|ext3|ext4`
(`dualboot.rs:219`); btrfs is *detected* (`dualboot/resize/btrfs.rs`) but not
*shrunk* by the executor.

### 5.3 The sub-cases, ranked

**(a) Free space already exists on the target disk → EASY.**
Shrink nothing. `DualBoot` already finds an adjacent free region
(`find_adjacent_free_region`, `dualboot.rs:341-354`) and skips the resize when
`shrink_bytes == 0` (`:236-255`). The only code change is relaxing the disk guard
(L8) *for this case* and adding an explicit "your root is on this disk, only free
space will be used" confirmation. **This is the highest value-per-effort item in
feature C**, and it becomes possible the moment feature A lands.

**(b) Shrink a partition that is NOT the running root → EASY.**
A second data partition, a Windows NTFS partition, an unused ext4 partition: unmount it
and the existing code works today. The disk guard (L8) is the only thing in the way,
and it is arguably *too strict* here — it refuses the whole disk when only some
partitions are in use. Relaxing it to "refuse only partitions that are currently the
root, or hold open files from the running system" is the right general fix.

**(c) Shrink the running root, btrfs → MEDIUM.**
btrfs is the one filesystem that supports **online shrink**
(`btrfs filesystem resize -<n>G /`; documented as online for both directions in the
btrfs docs and confirmed by the SLES resize matrix). Add a `btrfs` arm to
`auto_resize_partition`, skip the `umount`, run `btrfs filesystem resize`, then
`sfdisk -N`. Caveats: the minimum size is `btrfs filesystem usage` "Free (estimated) (min: X)"
— already parsed by `resize/btrfs.rs:80-90`; the shrink is IO-intensive and
relocates data; `/home` as a subvolume means shrinking the *filesystem* affects all
subvolumes; and shrinking the FS before shrinking the partition is mandatory ordering
(shrink FS → shrink partition, never the reverse).

**(d) Shrink the running root, ext4 → HARD (needs a reboot).**
The standard trick: add `resize2fs` to the **initramfs** and do the shrink from there,
before `/` is mounted. Or: shrink from a systemd unit at next boot into `rescue.target`.
Both are automatable but they are a *different execution model* from the current
"everything happens in one foreground wizard run" — the install would need to
suspend, reboot, and resume, which the `InstallState` resume machinery
(`execution/state.rs`) is actually well positioned to support. Genuinely feasible, but
it is a design project, not a patch.

**(e) Shrink the running root, XFS → IMPOSSIBLE.**
XFS has no shrink at all. `xfs_growfs` grows; there is no `xfs_shrinkfs`. Ubuntu's
default is ext4, Fedora's used to be XFS, so this will be a real user. Fail with a
clear message pointing at feature D or at "boot a live ISO for this one".

### 5.4 Shared-ESP hazards for same-disk installs

- `grub-install` on UEFI writes to the ESP that the *source* OS also uses. The existing
  `--recheck` + `os-prober` path should preserve the old entry, but the ESP must not
  be reformatted. `EspNeedsFormat` (`engine/context.rs:17-22`) is the existing
  opt-in for that and must stay off by default here.
- The ArchWiki's dual-boot advice applies: prefer *not* overwriting the NVRAM default
  if you want the old OS to keep booting (see the archinstall `--removable` note).
- EFI variables require efivarfs; available on any real system, absent in some
  containers. Non-issue for real hardware.

---

## 6. Feature D — installing over the running root

### 6.1 Why the naive version cannot work

You cannot `mkfs` the device you are running from. Concretely:

1. `umount /` fails with `EBUSY` — it is your root, by definition.
2. Even with `umount -l` (lazy detach), your `bash`, your `ins` binary, `systemd`,
   every daemon, and the kernel module `.ko` files in `/usr/lib/modules` are *open file
   descriptors* pointing at inodes on that filesystem. Writing over them gives
   `ETXTBSY` or, worse, succeeds and then the running process executes garbage.
3. `mkfs.ext4` on that device destroys the metadata of the live filesystem
   immediately. The machine dies within seconds, in an unrecoverable state, holding an
   open write to a filesystem it just erased.
4. The bootloader is usually on the *same physical disk* (or the same ESP), so step 5
   of the install destroys the only way back in.
5. Any interruption — power loss, OOM, a hung `mkfs` — leaves a machine that cannot
   boot, with no live medium and no other OS.

So the "hacky" reputation is earned. But every project that does this solves the same
four problems the same way.

### 6.2 The three real designs

#### Design 1 — `pivot_root` into a RAM-resident installer (the "takeover" pattern)

Reference implementations: `peva3/takeover.sh`, `hugopoi/self-flash-image`, the
ivarch "resize a live root fs" HOWTO, `nixos-in-place`.

```
1. systemd isolate rescue.target            # stop everything that holds files open
2. mount -t tmpfs tmpfs /takeover           # 2–4 GiB of RAM
3. rsync or untar a minimal Arch into /takeover
4. mount --bind /takeover /takeover         # pivot_root requires a mount point
5. pivot_root /takeover /takeover/oldroot   # / is now RAM
6. umount -l /oldroot                       # the real root is now free  ← the whole point
7. ... do the actual install, now unimpeded ...
```

**Why instantOS is an unusually good fit for this:** the live ISO *already is* a
ramdisk Arch system built by `mkarchiso`. The "rescue environment" that `takeover.sh`
has to go assemble from a live host is, for instantOS, a 1.5 GiB ISO you can
`curl` + `losetup -P` + mount, or a local `.iso` file. So the honest version of this
feature for `ins` is not "build a rescue env from the running system" — it is
**"loop-mount the instantOS ISO and pivot into it."** That reuses the ISO's own
machinery (offline bundle included, `offline.rs:27`) instead of inventing a rescue
rootfs.

**Costs / risks:**
- 1.5 GiB download + 2–4 GiB free RAM, and the ISO's `cowspace` needs to be
  remounted larger inside the pivot (the code for this *exists*:
  `distro.rs:263-297 increase_cowspace`).
- `pivot_root` requires non-shared mount propagation; systemd 257+ defaults `/` to
  shared, so you need `mount --make-private /` (or `unshare -m` — but note
  `unshare -m` only changes *that process's* namespace, which is actually what you
  want, at the cost of not affecting already-running threads).
- `pivot_root` only works for initrd, not initramfs (`pivot_root(2)`).
- **The session-continuity problem**: after you pivot, your terminal, your SSH
  connection and your `ins` process are all attached to the old root. `takeover.sh`
  solves this with a static `fakeinit` that re-execs a shell after the pivot, and by
  telling you to open a *new* SSH session first. `ins` cannot do this without becoming
  a different program.
- The undo path is `pivot_root` back. If you get that wrong, the machine is gone.
- `takeover.sh`'s own README: *"You should always test this in a VM first… don't
  blame me if it eats your dog."*

#### Design 2 — "alongside + cleanup on first boot" (the `bootc` model)

This is what production actually ships. `bootc install to-existing-root`
(bootc.dev, Fedora/CentOS) with `--replace=alongside` (the default):

- Wipes `/boot` and `/boot/efi`.
- **Keeps everything else in place.** Does not try to erase the old root.
- Installs the new deployment into the ostree layout on the same root filesystem.
- On first boot into the new system, the old root is available at `/sysroot`, and an
  optional first-boot cleanup unit deletes it.
- Explicitly documents the consequences: prior `/etc` data is *not* automatically
  migrated, `/etc/fstab` entries from the old system are not honoured (use
  `systemd.mount-extra` kargs instead), and previous mount points/subvolumes are not
  remounted under `/sysroot`.
- Key constraint stated in their docs: *"because the filesystem is reused, it's
  required that the target system kernel support the root storage setup already
  initialized."*

`nixos-in-place` is the same idea with different specifics: install NixOS into
`/nixos` on the existing root, put GRUB on top of the existing bootloader, bind the old
`/` to `/old-root`, and let the user delete the rest after first boot.

**This is much easier than Design 1 and much safer**, because at no point does anything
destroy a filesystem that is in use. The cost is honesty: the result is *not* a clean
install, and the user must be told so. `bootc` is upfront about this in their man page.

For instantOS, Design 2 maps surprisingly well:
- `StoragePlan::Manual` with the **existing root device** as the root partition,
  **not formatted** (instantOS already supports reusing an ESP without formatting —
  `EspNeedsFormat` — and `mount::format_and_mount_partitions` would need a
  "format: no" option per partition, which is a small, natural extension).
- The install then writes the new system *over* the old tree, skipping in-use paths
  (`/proc`, `/sys`, `/dev`, `/run`, `/tmp`, and critically `/usr/bin/ins` and
  `/usr/local/bin/ins` — the running binary; either `ETXTBSY` or schedule the
  overwrite for first boot).
- Bootloader: wipe `/boot`, `grub-install`, `grub-mkconfig` with a `40_custom` entry
  for the old root so the user can boot back into it.
- Drop a first-boot cleanup unit + a `/old-root` mount so the user can migrate `/home`
  data, then reclaim the space.
- `machine-id` regeneration becomes **mandatory** here, and so does regenerating SSH
  host keys (which Design 1 also needs).

#### Design 3 — deferred / staged install (no RAM-resident copy)

The ArchWiki's own "Replacing the existing system without an installation medium"
recipe is this: find ~700 MiB of free space (e.g. repurpose the swap partition),
`swapoff`, `mkfs`, `pacstrap` a minimal Arch there, install a bootloader, reboot into
it, and `rsync` it over the real root. Automated, that becomes:

- Write an `install` script into a systemd unit or a `mkinitcpio` hook that runs at
  next boot, before `/` is mounted (an initramfs hook has the ideal privilege and
  timing: nothing is holding files open yet).
- The hook does the real destructive work; the wizard just writes the hook and reboots.
- `InstallState` (`execution/state.rs:12-99`, keyed by config SHA, resumable) is
  already the right shape for a multi-boot install.

**This is the most "correct" of the three** and also the one where a failure is
catastrophic and untestable without real hardware. The ArchWiki itself suggests the
swap-partition route, which is a nice property: it does not touch `/` at all until the
machine is already running from the new system.

### 6.3 So: is it hacky?

**Yes, but in a well-understood way.** The honest framing:

- It is outside the kernel's supported model. Nothing in Linux is designed for
  "replace the filesystem under a running system", so every implementation is a
  sequence of `umount -l` / `pivot_root` / "don't reboot mid-way" that the authors all
  document as dangerous.
- It is **not** novel. `takeover.sh` is from 2006-ish; the ivarch HOWTO is from 2007;
  the ArchWiki page is old; `nixos-in-place` and `bootc` are current and shipping.
- The **specific** trick everyone uses is one of: (a) get off the root into RAM
  (Design 1), (b) don't erase, install alongside and clean up later (Design 2), or
  (c) do the destructive part after a reboot from a context where nothing is open
  (Design 3).
- Design 2 is the only one that has ever been shipped by a mainstream distro, and it
  is the one that is *not* destructive.

**Recommendation: implement Design 2 (bootc's `alongside`) and Design 3 (deferred
resize/replace) and do not implement Design 1 inside `ins`.** Design 1's session
continuity problem fundamentally conflicts with `ins arch install` being a
foreground TUI wizard. If instantOS wants a `takeover` capability, it belongs in a
separate `ins dev takeover` (or a standalone script) that says "this will cut your
session; open a second terminal first", and reuses the loop-mounted ISO as the rescue
env. `src/dev/` is already the home for exactly this kind of power-user tool
(`src/dev/chroot.rs` is a close structural sibling: scan disks → open LUKS → mount →
verify `/etc/os-release` → `arch-chroot`).

---

## 7. Implementation plan

### 7.0 Phase 0 — the test harness (do this first, blocks everything)

The repo currently has **no way to test a disk-level install at all**:
- `MockRunner` (`execution/mod.rs:791-848`) records command strings, which is good for
  asserting the *shape* of the new modes' command sequences.
- `--dry-run` and `/etc/instant/installdryrun` cover the non-destructive path.
- But there is **no VM, no QEMU, no loopback harness**. `tests/run_all.sh` runs shell
  E2E against `INS_BIN`; `test_install_script.sh` tests the *shell bootstrap*, not the
  Rust installer. Nothing exercises `ins arch install` or `ins arch exec` end to end.

Before writing any of the above, build:

1. **Loopback smoke test.** In a container or VM: create a 6 GiB sparse file,
   `losetup -P`, run `ins arch install` non-interactively with a hand-written
   `questions.toml` (the format is already stable and `ins arch ask -o` emits it),
   then assert the target boots. This is *dramatically* cheaper than a VM-per-run and
   exercises every real command. It needs `--target`-style device selection, which
   `DiskPath::parse` already accepts for `/dev/loop0`.
2. **A QEMU smoke test** for the boot assertion (a `scripts/` helper + a
   `tests/test_*.sh` that boots the loop image with `-nographic` and greps for a login
   prompt). QEMU with `-kernel`/`-initrd` extracted from the target avoids needing BIOS
   setup entirely.
3. **Coverage for the currently untested destructive code**: `disk/dualboot.rs`,
   `disk/encryption.rs`, `bootloader.rs` command emission, and `setup_chroot` have
   **zero** tests today. Any new mode lands on top of that.

Everything below is unsafe to ship without this.

### 7.1 Domain model

`CONTEXT.md` pins four terms: *Wizard state*, *Install plan*, *Answer value*,
*Validated value*. Two new validated values fit cleanly and should be added there:

- **`HostProfile`** (Validated value): what we are running on —
  `LiveIso | RunningArch | ForeignDistro`, with the resolved toolchain
  (`pacstrap`/`arch-chroot`/`genfstab` availability and how they were obtained).
- **`TargetRelation`** (Validated value): how the target relates to the running system —
  `OtherDisk | SameDiskFreeSpace | SameDiskAlongside | RunningRoot`, with the
  invariants each one implies (e.g. `RunningRoot` ⇒ no `umount` of `/`, no `mkfs` of
  the source device, `machine-id` regeneration required).

Both are computed from `SystemInfo::detect()` + `disks::get_root_device()` +
`disks::get_boot_disk()` and should be *validated values* (constructor-checked),
consumed by `InstallPlan::try_from`, and serialised into `questions.toml` so the
`exec` phase and the in-chroot re-entry (`arch exec <step>`) see the same decision.
The `install_flow_hooks_only_read_declared_dependencies` test
(`cli/commands/mod.rs:266-316`) means you must declare these as `required_data_keys` /
`depends_on` rather than reading globals.

### 7.2 Phases

| Phase | Scope | Size | Notes |
|-------|-------|------|-------|
| **0** | Loopback + QEMU e2e harness; unit-test the untested destructive paths | L | Blocks everything. Independent of the feature. |
| **1** | Ephemeral state dir; non-live dependency preflight; drop the live-only fzf/gum gate; regenerate `machine-id` | S | Prerequisite for 2–4. Touches `execution/paths.rs`, `offline.rs`, new `arch/host.rs` |
| **2** | **Feature A**: `OtherDisk` from a running Arch. Relax the disk guard to be `TargetRelation`-aware; refuse to unmount the source root; expose the menu item; fix the finish menu | S–M | Highest value per line changed |
| **3** | **Feature B**: `ForeignDistro`. Bootstrap preflight (apt → pacman → bootstrap tarball), Apt dep mappings, `/run/shm` + canonical `pacman.conf` + genfstab sanitising | M | Verified feasible on Ubuntu 24.04 |
| **4** | **Feature C(a)+(b)**: `SameDiskFreeSpace`, plus per-partition (not per-disk) guard relaxation | M | Nearly free once 2 lands — `DualBoot` already handles free space |
| **5** | **Feature C(c)**: online btrfs shrink in `auto_resize_partition` | M | Add a `btrfs` arm; honour the existing `resize/btrfs.rs` min-size parse; skip the `umount`; keep shrink-FS-before-shrink-partition ordering |
| **6** | **Feature C(d)**: deferred ext4 shrink via initramfs/systemd, with resume through `InstallState` | L | A design project; needs its own state machine |
| **7** | **Feature D, Design 2**: `RunningRoot` alongside, first-boot cleanup unit, `/old-root` migration mount, mandatory `machine-id` + host-key regen, custom GRUB entry for the old system | XL | Model on `bootc install to-existing-root`; explicit `--replace=alongside|wipe` where **`wipe` is not implemented** |
| **8** | (optional) `ins dev takeover`: loop-mount the ISO, `pivot_root`, do feature D from RAM | XL | Separate tool, explicit session-detach warning, VM-only support |

### 7.3 Things that must not regress

- `install_flow_hooks_only_read_declared_dependencies` and
  `install_question_graph_is_valid` (`cli/commands/mod.rs:255-316`) — adding steps
  means updating both.
- The chroot hand-off's `completed_steps` / `configuration_sha256` contract: a
  multi-boot install (phase 6/7) must survive the machine rebooting mid-install, so
  `InstallState` needs to become the source of truth for "where were we", not just a
  log.
- `installation_identity` idempotency must be disabled or target-scoped for any
  same-disk mode, or it will read the *running* system and falsely report
  `AlreadyInstalled`.
- The offline bundle path is a live-ISO feature; for a non-live install either make
  `BUNDLE_ROOT` overridable (so a loop-mounted ISO's bundle can be used — a genuinely
  nice capability, see Design 1) or leave `mode() == Online` and require a network.

---

## 8. Recommendation

1. **Build Phase 0 first.** A disk-level e2e harness is worth more than any of these
   features and is currently missing entirely. Without it, phases 6–8 are unfalsifiable.
2. **Ship A and B.** Together they cover the overwhelming majority of real demand —
   "I have a spare SSD and don't want to flash a USB stick" and "I'm on Ubuntu, install
   Arch/instantOS onto a second drive" — and they are *not destructive to the running
   system*, so they carry none of the risk that makes feature D hairy. Verified
   feasible on Ubuntu 24.04 with the toolchain already in `universe`.
3. **Ship C(a)+(b) next**, then C(c) for btrfs. The dual-boot code already exists; you
   are mostly un-blocking it. Refuse loudly for XFS.
4. **Treat D as a separate, opt-in, advanced feature**, modelled on
   `bootc install to-existing-root --replace=alongside`, with `wipe` deliberately
   unimplemented. Ship it only after the Phase 0 harness exists. It should be
   discoverable only via an explicit flag, and the confirmation should name the
   physical disk and print what will and will not be erased.
5. **Do not put `pivot_root` inside `ins arch install`.** The session-continuity
   problem makes it incompatible with a foreground TUI. If instantOS wants that
   capability, put it in `ins dev takeover` reusing the loop-mounted ISO.

---

## 9. Sources

**instantOS / instantCLI (this repo)**
- `src/arch/cli/commands/install.rs`, `src/arch/cli/commands/mod.rs`,
  `src/arch/cli/commands/ask.rs`, `src/arch/cli/commands/setup.rs`
- `src/arch/execution/mod.rs`, `step.rs`, `paths.rs`, `state.rs`, `fstab.rs`, `base.rs`,
  `bootloader.rs`, `disk/{mod,automatic,filesystem,encryption,dualboot,mount}.rs`
- `src/arch/questions/disk.rs`, `src/arch/disks.rs`, `src/arch/offline.rs`,
  `src/arch/installation_identity.rs`, `src/arch/dualboot/resize/*`
- `src/common/distro.rs`, `src/common/deps.rs`, `src/common/package/*`
- `src/welcome/ui.rs`, `src/menu_utils/fzf/utils.rs`, `src/dev/chroot.rs`
- `scripts/install-src/environment.sh`, `Cargo.toml`, `utils/build_deb.sh`,
  `CONTEXT.md`, `README.md`

**Arch / general**
- ArchWiki, *Install from existing Linux* —
  https://wiki.archlinux.org/title/Install_from_Existing_Linux
  (includes "Replacing the existing system without an installation medium", the
  bootstrap-tarball route, and the Debian `/run/shm` / Fedora `seclabel` caveats)
- ArchWiki, *Installation guide* — https://wiki.archlinux.org/title/Installation_guide
- ArchWiki, *chroot* / *arch-chroot* — https://wiki.archlinux.org/title/Change_root
- ArchWiki, *Migrate installation to new hardware* — notes that disk cloning
  **requires** a live system
- ArchWiki, *Installing Arch Linux on a USB key* — "It is possible to install Arch on
  the same USB drive that you are trying to install it from. However, you cannot shut
  down or reboot in the middle of the installation process."
- Arch Linux bootstrap tarball (verified present, 2026-09-01, 121 MiB) —
  https://geo.mirror.pkgbuild.com/iso/latest/archlinux-bootstrap-x86_64.tar.zst
- `archinstall` README: "The installer also doubles as a python library … *(Usually
  from a live medium **or from an existing installation**)*" —
  https://github.com/archlinux/archinstall
- `pacstrap(8)` man page (Debian) — https://manpages.debian.org/unstable/arch-install-scripts/pacstrap.8
- Ubuntu `arch-install-scripts` 28-1 in `noble/universe` (verified via
  `apt-cache policy` on Ubuntu 24.04.5) — https://packages.ubuntu.com/noble/all/arch-install-scripts
- Debian package tracker — https://tracker.debian.org/pkg/arch-install-scripts
- From Debian to Arch without ISO (walkthrough; `apt install arch-install-scripts
  pacman-package-manager archlinux-keyring makepkg`) —
  https://www.jamescherti.com/installing-arch-linux-from-debian-system-ubuntu-linux-mint/
- Arch from Arch onto an external drive (walkthrough) —
  https://revontulet.dev/p/2026-installing-arch-from-arch-onto-an-external-drive/
- `yannayl/arch-bootstrap` — https://github.com/tokland/arch-bootstrap
- Cross-distro/arch Arch bootstrapping without a prebuilt rootfs —
  https://7ji.github.io/cross/2023/09/18/cross-bootstrap-arch.html
- Archinstall on a machine with an existing Btrfs/ESP layout —
  https://www.lorenzobettini.it/2026/07/archinstall-dual-boot-with-another-linux-installation/

**In-place / onto-the-running-root designs**
- `bootc install to-existing-root` man page and design docs (**the reference design**) —
  https://bootc.dev/bootc/man/bootc-install-to-existing-root.8.html
  https://github.com/bootc-dev/bootc/blob/main/docs/src/bootc-install.md
  https://bootc.dev/bootc/bootc-install.html
  (`ReplaceMode::Wipe` vs `Alongside`; "This cannot be done if the target filesystem is
  the one the system is booted from"; old root at `/sysroot`;
  `systemd.mount-extra` kargs instead of migrating `/etc/fstab`;
  `system-reinstall-bootc` as the interactive wrapper)
- `jeaye/nixos-in-place` — https://github.com/jeaye/nixos-in-place/blob/master/README.md
  (install into `/nixos` on the existing root, GRUB on top of the existing bootloader,
  old root at `/old-root`)
- `peva3/takeover.sh` — https://github.com/peva3/takeover.sh
  ("log into an in-memory rescue environment, unmount the original root filesystem, and
  do anything you want, all without rebooting"; the `fakeinit` session-detach problem;
  "You should always test this in a VM first")
- `hugopoi/self-flash-image` — pivot_root recipe
  https://home.hugopoi.net/gitea/hugopoi/self-flash-image
- "Resize a live root FS — a HOWTO" (tmpfs + `pivot_root`, 2007) —
  https://ivarch.com/blogs/oss/2007/01/resize-a-live-root-fs-a-howto.shtml
- `pivot_root(2)` semantics and the shared-propagation / `mount --make-private`
  requirement — https://man7.org/linux/man-pages/man2/pivot_root.2.html
- Ubuntu Wiki, *install-ovewrite* (keep `/home`, wipe the rest) —
  https://wiki.ubuntu.com/install-ovewrite

**Resize capability matrix**
- Btrfs: online grow *and* shrink — https://btrfs.readthedocs.io/en/latest/Resize.html
- SLES resize matrix (btrfs online both ways; ext2/3/4 shrink offline only; XFS no
  shrink) — https://documentation.suse.com/en-us/sles/15-SP7/html/SLES-all/cha-resize-fs.html
- `resize2fs` "On-line shrinking not supported" —
  https://serverfault.com/questions/528075/is-it-possible-to-on-line-shrink-a-ext4-volume-with-lvm
- initramfs-based ext4 shrink on a remote host (no console) —
  https://serverfault.com/questions/528075/is-it-possible-to-on-line-shrink-a-ext4-volume-with-lvm#answer-151146
- chroot + `/proc /sys /dev /run` bind-mount discipline —
  https://wiki.archlinux.org/title/Change_root
