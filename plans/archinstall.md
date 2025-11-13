# asking questions



ping archlinux.org to ensure interent connectivity, if not found, cancel installation

```
localectl list-keymaps
```

list keymaps, allow user to choose one

after that, do this

```
loadkeys de-latin1 # or whatever keymap user chose
```


ask if high-res screen is used, if yes, do this
```
setfont ter-132b
```


check boot mode
```
cat /sys/firmware/efi/fw_platform_size
```

the command returns 64, the system is booted in UEFI mode and has a 64-bit x64 UEFI.
If the command returns 32, the system is booted in UEFI mode and has a 32-bit IA32 UEFI. While this is supported, it will limit the boot loader choice to those that support mixed mode booting.
If it returns No such file or directory, the system may be booted in BIOS (or CSM) mode.

keep track of this state in an enum


fdisk -l

allow choosing disk to install to, for now take the whole disk


ask mirror regions

1. scrape https://archlinux.org/mirrorlist/ to get the list of available countries/regions
2. Parsing the HTML: filter for 'option value=".."' and do some regex to extract country codes and names from the HTML options
3. Let user choose one

prompt for timezone

1. list /usr/share/zoneinfo
2. Auto-detection (optional): If tzupdate is available, it suggests the current timezone by reading /etc/localtime and formatting it
3. List all timezones: Uses find . -type f | sort -u to get all timezone files from the zoneinfo directory (do this with walkdir)
4. Format for display: 
   - Removes leading ./ with sed 's/\.\///g' (do this in a Rustier way)
   - Filters out short entries with grep -Eo '.{2,}' (do this in a Rustier way)
   - Replaces slashes with arrow symbols  for better UI display: sed 's/\//   /g'
5. Present to user: Make user choose one
6. Clean up selection: After selection, the arrow symbols are converted back to slashes and any auto-detect prefix is removed


Prompt for system language (locale)

Do this in a way similar to how it is dont in `ins settings`, maybe some utils
need to be extracted or made more general.

ask for hostname (call this "Name of the computer" in the UI)

ask for username
ask for password (with confirmation) 
the password will be for both root and the user account

# installation

use the following partition layout for UEFI

/boot1	/dev/efi_system_partition	EFI system partition	1 GiB
[SWAP]	/dev/swap_partition	Linux swap	At least 4 GiB
/	/dev/root_partition	Linux x86-64 root (/)	Remainder of the device. At least 23–32 GiB.

Use this for BIOS
[SWAP]	/dev/swap_partition	Linux swap	At least 4 GiB
/	/dev/root_partition	Linux	Remainder of the device. At least 23–32 GiB.


Check the RAM size to calculate a sensible swap size

Format the partitions

mkfs.ext4 /dev/root_partition

mkswap /dev/swap_partition

Mount the file systems

mount /dev/root_partition /mnt

If UEFI is detected, do this
mount --mkdir /dev/efi_system_partition /mnt/boot

swapon /dev/swap_partition



Set up actual mirrors Country-specific fetching: When a country is selected, depend/mirrors.sh:18 fetches mirrors specifically for that country code from https://archlinux.org/mirrorlist/?country=$COUNTRYCODE

COUNTRYCODE was selected earlier by the user

Build a list of essential packages to install:

- Check if AMD or Intel CPU i s present, if yes, add amd-ucode or intel-ucode to the package list
- add linux-firmware
- check if nvidia gpu is present, if yes, add nvidia package to the list
- add basem linux, linux-headers


run this outside the chroot

genfstab -U /mnt >> /mnt/etc/fstab

some of the next steps will use arch-chroot /mnt as a wrapper

in chroot:
ln -sf /usr/share/zoneinfo/Region/City /etc/localtime

user selected Region/City earlier


locales

edit locale.gen and uncomment the user selected locales
run locale-gen

/etc/locale.conf
LANG=en_US.UTF-8

use localectl to set the locale

localectl set-locale LANG="$SETLOCALE"



/etc/vconsole.conf
KEYMAP=whatever keymap user chose



set the hostname in /etc/hostname

yourhostname

hostnamectl set-hostname "youthostname"


set the root password

create the user
- username from user choice
- user password fom user choice
- with home directory
- with zsh as default shell

install grub

if UEFI
then

inside the chroot
grub-install --target=x86_64-efi --efi-directory=/efi --bootloader-id=GRUB

else
grub-install "${DISK}"

fi

