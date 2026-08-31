REPO="instantOS/instantCLI"
API_URL="https://api.github.com/repos/$REPO/releases"
BIN_NAME="ins"
SOURCE_BIN_NAME="ins"

INSTALL_DIR=${INSTALL_DIR:-}
CLI_ONLY=0
OS_INSTALL=0
ONLY_ANIMATION=0
NO_ANIMATION=0

# @include output.sh

# @include animation.sh

# @include environment.sh

# @include releases.sh

# @include install_binary.sh

# @include summary.sh

# @include main.sh

main "$@"
