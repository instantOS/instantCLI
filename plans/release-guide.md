# Release workflow for InstantCLI

This guide covers the end-to-end process for publishing a new InstantCLI release, including version management, tagging, and packaging for GitHub releases and Arch Linux users.

## 1. Prepare the environment

1. Ensure the repository is clean and up-to-date: `git status` should show no pending changes.
2. Install the release tooling once on your workstation: `cargo install cargo-release`. The `cargo release` subcommand automates version bumps, tagging, and pushes for Rust crates.[^cargo-release]

## 2. Bump the version and tag the release

1. Decide the appropriate semantic version bump (`patch`, `minor`, or `major`).
2. Run a dry run to check the planned changes: `cargo release <level>`.
3. Execute the release when ready: `cargo release <level> --execute`.
   - This updates `Cargo.toml`, refreshes `Cargo.lock`, commits the changes, creates the `vX.Y.Z` tag, and pushes by default.[^cargo-release]

## 3. Refresh the Arch packaging metadata

1. Update the default `VERSION` value at the top of `packaging/PKGBUILD` to match the new crate version.
2. Regenerate hashes so the tarball download remains reproducible: `cd packaging && updpkgsums`.
3. Run the packaging checks locally if desired:
   ```bash
   cd packaging
   VERSION="<new-version>" makepkg --clean --cleanbuild --syncdeps --noconfirm
   ```
   This follows the Arch Rust packaging guidelines for fetching dependencies, building, testing, and installing binaries.[^arch-rust]
4. Commit the PKGBUILD updates alongside the release commit created by `cargo release`.

## 4. Push to GitHub

1. Push the release commit if `cargo release` did not do so automatically: `git push origin main`.
2. Push the annotated tag if needed: `git push origin vX.Y.Z`.
3. The `Release` GitHub Actions workflow triggers automatically on the new tag. It recompiles on an Arch Linux container, generates a stripped binary, builds the pacman package via `makepkg`, and uploads both artifacts with `softprops/action-gh-release`.[^gh-release]

## 5. Verify the published assets

1. Open the GitHub release page for the new tag.
2. Download the attached binary (`instant-<version>-x86_64-unknown-linux-gnu`) and pacman package (`instant-<version>-1-x86_64.pkg.tar.zst`) to confirm they install and run as expected.

## Useful GitHub Actions

- [`softprops/action-gh-release`](https://github.com/softprops/action-gh-release) publishes releases and uploads artifacts based on tag pushes.[^gh-release]
- [`FFY00/build-arch-package`](https://github.com/FFY00/build-arch-package) is an alternative action for PKGBUILD-based packaging inside Arch containers if a reusable composite action is preferred.[^build-arch]

[^cargo-release]: crate-ci, *cargo-release* README, <https://github.com/crate-ci/cargo-release>.
[^arch-rust]: Arch Linux Wiki, *Rust package guidelines*, <https://wiki.archlinux.org/title/Rust_package_guidelines>.
[^gh-release]: softprops, *action gh-release* README, <https://github.com/softprops/action-gh-release>.
[^build-arch]: FFY00, *build-arch-package* README, <https://github.com/FFY00/build-arch-package>.
