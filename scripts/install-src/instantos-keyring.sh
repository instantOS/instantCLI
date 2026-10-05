# shellcheck shell=sh
# The public key is bundled so installation never depends on a keyserver.
# Keep this snapshot in sync with the vendored instantos-keyring public key.
bootstrap_instant_keyring() (
	instant_conf=${1:-/etc/pacman.conf}
	instant_master=E5C4740F883910B29694D6BE10DA645A82CF5206
	instant_signer=86D0589DC0EC55E4E5DFCE4436276C1C0A17CCD1
	instant_gpgdir=$(pacman-conf --config "$instant_conf" GPGDir) || return 1
	case "$instant_gpgdir" in
	/*) ;;
	*)
		echo "Invalid pacman GPGDir: $instant_gpgdir" >&2
		return 1
		;;
	esac
	instant_tmp=$(mktemp -d) || return 1
	trap 'rm -rf -- "$instant_tmp"' EXIT
	chmod 700 "$instant_tmp" || return 1
	cat >"$instant_tmp/public.asc" <<'INSTANTOS_PUBLIC_KEY'
-----BEGIN PGP PUBLIC KEY BLOCK-----

mDMEao8erhYJKwYBBAHaRw8BAQdAz6DdUv1yPniksezj/bp66MMNvwfnSJuH7fbL
RnQg9hK0J2luc3RhbnRPUyBQYWNrYWdlcyA8aGVsbG9AaW5zdGFudG9zLmlvPoiQ
BBMWCgA4FiEE5cR0D4g5ELKWlNa+ENpkWoLPUgYFAmqPHq4CGwEFCwkIBwIGFQoJ
CAsCBBYCAwECHgECF4AACgkQENpkWoLPUgYZbwD+O5dVIT+xahnPkWXeTy8iow0N
UADqYXAhigVm5wHjUCcA/RidbEHS6SbXeiqYHnsb7b4Yx+bjXmyRMNMRkshdffoN
uDMEao/85RYJKwYBBAHaRw8BAQdAwcKDRw77X2duANRsdCnx2IbfxQHtGLUw8uK3
3TI+PaqI9QQYFgoAJhYhBOXEdA+IORCylpTWvhDaZFqCz1IGBQJqj/zlAhsCBQkD
wmcAAIEJEBDaZFqCz1IGdiAEGRYKAB0WIQSG0FidwOxV5OXfzkQ2J2wcChfM0QUC
ao/85QAKCRA2J2wcChfM0ZytAP9kVeA/bnKTBrRY/yj+XEwRtmbY3QZUSGfvA27Y
6qYlTQEA6XMO9HVQu3Nfe1gFjRHVSH5UTSXNpdFtXucaYctXeAmVnAEA9noFaRWg
9fh4u37WMAciVTRD3JKP0TvfMfTiX6/9OlEBAN7Y44EQmYTTD/KSKmm4A8hCYy71
KAhi4hmd3fp0FMoO
=+5Jk
-----END PGP PUBLIC KEY BLOCK-----
INSTANTOS_PUBLIC_KEY
	# Validate both fingerprints before changing pacman's keyring. A key file
	# containing any additional primary key is rejected.
	gpg --homedir "$instant_tmp" --batch --with-colons --with-subkey-fingerprint \
		--show-keys "$instant_tmp/public.asc" >"$instant_tmp/listing" || return 1
	awk -F: -v master="$instant_master" -v signer="$instant_signer" '
        $1 == "pub" { kind = "pub"; count++ }
        $1 == "sub" { kind = "sub" }
        $1 == "fpr" && kind == "pub" && $10 != master { bad = 1 }
        $1 == "fpr" && kind == "sub" && $10 == signer { found = 1 }
        END { exit !(count == 1 && !bad && found) }
    ' "$instant_tmp/listing" || {
		echo "Bundled instantOS key does not match the pinned fingerprints" >&2
		return 1
	}
	instant_pacman_key() {
		if [ "$(id -u)" -eq 0 ]; then
			pacman-key --gpgdir "$instant_gpgdir" "$@"
		else
			sudo pacman-key --gpgdir "$instant_gpgdir" "$@"
		fi
	}
	instant_pacman_key --init || return 1
	instant_pacman_key --add "$instant_tmp/public.asc" || return 1
	instant_pacman_key --lsign-key "$instant_master" || return 1
	instant_pacman_key --updatedb || return 1
	# Fail rather than reporting success for a revoked or expired installed key.
	if [ "$(id -u)" -eq 0 ]; then
		gpg --homedir "$instant_gpgdir" --batch --with-colons --list-keys "$instant_master" \
			>"$instant_tmp/installed" || return 1
	else
		# shellcheck disable=SC2024 # The caller owns this temporary output file.
		sudo gpg --homedir "$instant_gpgdir" --batch --with-colons --list-keys "$instant_master" \
			>"$instant_tmp/installed" || return 1
	fi
	awk -F: '
        $1 == "pub" { trusted = ($2 == "f" || $2 == "u") }
        $1 == "sub" && trusted && $2 !~ /^[redi]$/ && $12 ~ /s/ { usable = 1 }
        END { exit !usable }
    ' "$instant_tmp/installed" || {
		echo "instantOS signing key is missing, unusable, or untrusted after import" >&2
		return 1
	}
)
