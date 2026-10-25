# Alpine release packages

The release workflow packages the existing `x86_64-unknown-linux-musl` and
`aarch64-unknown-linux-musl` artifacts with Alpine 3.22's `abuild`. It uploads
`ins-<version>-r0-<arch>.apk` and a sibling `.sha256` file to the GitHub release,
alongside the Debian package. Both architectures can be packaged on an x86_64
runner: packaging never executes the target binary.

The package includes the CLI, desktop entries, documentation, licenses (including
the emoji catalog's license), and the optional systemd user notification service.
It depends on Bash, fzf, and Git. The executable is static, with SQLite and TLS
libraries bundled by the release build's `cross-compile` feature. The build helper
rejects dynamic executables and binaries whose ELF architecture does not match
the requested package architecture.

The x86_64 release job installs its package in a clean Alpine container and checks
its checksum, dependencies, CLI startup, and packaged integration files.

## Build locally

Extract a matching musl release archive into `dist/`, then run from the repository
root (change the version and architecture to match your input):

```sh
docker run --rm -v "$PWD:/work" -w /work alpine:3.22 sh -ec '
    apk add --no-cache abuild binutils
    SUDO= abuild-keygen -a -n -i
    ./utils/build_apk.sh dist/ins-aarch64-unknown-linux-musl-v0.14.23/ins aarch64
'
```

Output is written to `target/apk/`. `utils/build_apk.sh` reads the package version
from `Cargo.toml`, so the input binary must come from the same version. A local
static musl debug build can also be supplied.

`abuild-keygen` creates a temporary signing key inside the disposable container;
CI needs no signing secret. These downloads are standalone packages, not a signed
apk repository. After checking the published checksum, install as root using
`apk add --no-cache --allow-untrusted ./ins-<version>-r0-<arch>.apk`.
