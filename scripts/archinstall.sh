#!/bin/bash

# VERY experimental new Rust based installer for instantOS

echo "installing instantOS"

mkdir -p /etc/instantos
touch /etc/instantos/uploadlogs

# Always initialize pacman keyring (required in live ISO environment)
echo "Initializing pacman keyring..."
pacman-key --init
echo "Populating Arch Linux keys..."
pacman-key --populate archlinux

# Update keyring package first to avoid signature issues
echo "Updating archlinux-keyring package..."
if ! pacman -Sy --needed archlinux-keyring --noconfirm; then
    echo "Error: Failed to update archlinux-keyring. Cannot continue."
    exit 1
fi

# Now install required packages
echo "Installing dependencies..."
pacman -S --needed libgit2 --noconfirm
curl https://stuff.paperbenni.xyz/ins > ins
chmod +x ins
./ins arch install

