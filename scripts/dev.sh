#!/bin/bash

# ins dev setup

echo "installing instantOS"

mkdir -p /etc/instantos
touch /etc/instantos/uploadlogs

sudo pacman -Sy --needed libgit2 --noconfirm
curl https://stuff.paperbenni.xyz/ins > /usr/local/bin/ins
chmod +x /usr/local/bin/ins
ins dev setup
zsh

