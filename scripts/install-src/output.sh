log() {
	printf '%s\n' "$1"
}

warn() {
	printf 'warning: %s\n' "$1" >&2
}

fatal() {
	printf 'error: %s\n' "$1" >&2
	exit 1
}
