# Executed only inside the authenticated, disposable Arch bootstrap.
printf 'Server = https://geo.mirror.pkgbuild.com/$repo/os/$arch\n' >/etc/pacman.d/mirrorlist
pacman-key --init
pacman-key --populate archlinux
# Upgrade the bootstrap as a unit. Never perform a partial Arch upgrade.
pacman -Syu --needed --noconfirm arch-install-scripts sqlite fzf gum git \
	gptfdisk parted btrfs-progs ntfs-3g ntfsprogs dosfstools e2fsprogs cryptsetup lvm2 \
	pciutils usbutils kbd acpi kmod
# Fail before the wizard can partition anything if this build's dynamic
# dependencies cannot be satisfied by the Arch environment.
/usr/bin/ins --version
