#!/bin/bash

# VERY experimental new Rust based installer for instantOS

echo "installing instantOS"
sudo pacman -Sy --needed libgit2 --noconfirm
curl https://stuff.paperbenni.xyz/ins > ins
chmod +x ins
./ins arch install

