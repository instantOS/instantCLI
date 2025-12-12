#!/bin/bash

# VERY experimental new Rust based installer for instantOS

echo "installing instantOS"

mkdir -p /etc/instantos
touch /etc/instantos/uploadlogs

# Initialize pacman keyring if needed
if [ ! -d "/etc/pacman.d/gnupg" ] || [ -z "$(ls -A /etc/pacman.d/gnupg 2>/dev/null)" ]; then
    echo "Initializing pacman keyring..."
    pacman-key --init
    echo "Populating Arch Linux keys..."
    pacman-key --populate archlinux
fi

# Update keyring package first to avoid signature issues (without full system upgrade)
echo "Updating archlinux-keyring package..."
pacman -Sy --needed archlinux-keyring --noconfirm
echo "Refreshing keys..."
pacman-key --refresh-keys

# Now install required packages
echo "Installing dependencies..."
pacman -S --needed libgit2 --noconfirm
curl https://stuff.paperbenni.xyz/ins > ins
chmod +x ins
./ins arch install

