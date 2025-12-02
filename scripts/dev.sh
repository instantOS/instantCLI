#!/bin/bash

# ins dev setup

echo "installing instantOS"

mkdir -p /etc/instantos
touch /etc/instantos/uploadlogs

sudo pacman -Sy --needed libgit2 --noconfirm
curl https://stuff.paperbenni.xyz/ins > ins
chmod +x ins
./ins dev setup
zsh

