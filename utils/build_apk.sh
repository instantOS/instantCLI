#!/bin/sh
set -eu

ROOT_DIR=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT_DIR"

if [ "$#" -ne 2 ]; then
	printf 'Usage: %s <static-musl-binary> <x86_64|aarch64>\n' "$0" >&2
	exit 1
fi
INS_BIN=$1
ARCH=$2
case "$ARCH" in
x86_64 | aarch64) ;;
*)
	printf 'unsupported apk architecture: %s\n' "$ARCH" >&2
	exit 1
	;;
esac
for command in abuild readelf tar sha256sum; do
	command -v "$command" >/dev/null 2>&1 || {
		printf 'missing dependency: %s (run inside Alpine with abuild and binutils)\n' "$command" >&2
		exit 1
	}
done
[ -x "$INS_BIN" ] || {
	printf 'binary not found or not executable: %s\n' "$INS_BIN" >&2
	exit 1
}

# These packages reuse vendored, static musl builds. Reject dynamic or incorrectly
# labelled inputs rather than publishing packages with undeclared library needs.
machine=$(LC_ALL=C readelf -h "$INS_BIN" | awk -F: '/Machine:/ {sub(/^[[:space:]]+/, "", $2); print $2}')
case "$ARCH:$machine" in
x86_64:*X86-64 | aarch64:AArch64) ;;
*)
	printf 'binary architecture does not match %s: %s\n' "$ARCH" "$machine" >&2
	exit 1
	;;
esac
if LC_ALL=C readelf -l "$INS_BIN" | grep -q INTERP; then
	printf 'apk packaging requires a statically linked musl binary\n' >&2
	exit 1
fi
VERSION=$(awk -F '"' '/^version =/ {print $2; exit}' Cargo.toml)
[ -n "$VERSION" ] || {
	printf 'failed to determine version from Cargo.toml\n' >&2
	exit 1
}

WORK_DIR=$(mktemp -d)
trap 'rm -rf "$WORK_DIR"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'exit 129' HUP
PACKAGE_DIR="$WORK_DIR/instantcli/ins"
PAYLOAD_DIR="$WORK_DIR/payload"
mkdir -p "$PACKAGE_DIR" "$PAYLOAD_DIR/desktop" "$ROOT_DIR/target/apk"
install -m755 "$INS_BIN" "$PAYLOAD_DIR/ins"
install -m644 LICENSE README.md "$PAYLOAD_DIR/"
install -m644 src/assist/actions/emoji/LICENSE "$PAYLOAD_DIR/emoji-catalog-LICENSE"
install -m644 desktop/*.desktop "$PAYLOAD_DIR/desktop/"
install -m644 systemd/ins-notify.service "$PAYLOAD_DIR/ins-notify.service"
tar -czf "$PACKAGE_DIR/payload.tar.gz" -C "$PAYLOAD_DIR" .
sed "s/@VERSION@/$VERSION/g" apk/APKBUILD.in >"$PACKAGE_DIR/APKBUILD"
(
	cd "$PACKAGE_DIR"
	# Only metadata and files are packaged: no target binaries are executed, so
	# both architectures can be packaged on an x86_64 runner without emulation.
	export CARCH="$ARCH"
	abuild -F -d checksum
	abuild -F -d -P "$WORK_DIR/packages"
)
OUTPUT="$ROOT_DIR/target/apk/ins-${VERSION}-r0-${ARCH}.apk"
install -m644 "$WORK_DIR/packages/instantcli/$ARCH/ins-${VERSION}-r0.apk" "$OUTPUT"
(cd "$ROOT_DIR/target/apk" && sha256sum "${OUTPUT##*/}" >"${OUTPUT##*/}.sha256")
printf 'apk package created at %s\n' "$OUTPUT"
