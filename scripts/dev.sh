#!/bin/bash

# ins dev setup

echo "installing instantOS"

mkdir -p /etc/instantos
touch /etc/instantos/uploadlogs

curl https://stuff.paperbenni.xyz/ins > /usr/local/bin/ins
chmod +x /usr/local/bin/ins
ins dev setup
zsh
