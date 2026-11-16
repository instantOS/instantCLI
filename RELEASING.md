# Releasing Instant CLI

Releases are distributed through GitHub artifacts, Arch packaging, and npm.
New versions are not published to crates.io. Git dependencies stay in place;
the shared crates do not need separate registry releases.

## Prepare and publish

The **Version bump PR** workflow opens or updates `release/version-bump` after
main changes and on its weekly schedule. Merge that PR to publish a release.
It updates Cargo.toml, the root package in Cargo.lock, CHANGELOG.md, and the
Arch PKGBUILD and .SRCINFO together. Shared dependency revisions stay unchanged.

Automatic version policy:

- `feat:` / `feat(scope):` advances the minor version.
- Breaking changes advance the minor version below 1.0, and major from 1.0 onward.
- Fixes, performance changes, refactors, and unprefixed commits advance patch.
- `chore:`, `ci:`, `docs:`, `style:`, and `test:` alone do not prepare a release.

Use Actions → **Version bump PR** → **Run workflow** on main to request an
explicit patch, minor, or major bump. Major releases below 1.0 are deliberate.
The script bases bumps on the latest reachable release tag, so repeated runs
before publication keep the same version. It rebuilds pending changelog entries
from Git, replacing duplicate untagged release-plz entries during migration.

The PR workflow reuses the existing `RELEASE_PLZ_TOKEN` secret when present so
its PR updates continue to trigger CI. It falls back to `GITHUB_TOKEN`; GitHub
must allow Actions to create PRs, and that fallback does not trigger PR CI.
Keep the existing token configured if release PR checks are required.

After the PR merges, **Release** recognizes the generated release commit
(including squash, rebase, and ordinary merge commits), creates the version tag,
and builds all existing binary and package formats. It publishes GitHub release
notes from CHANGELOG.md, then publishes npm packages using the same tag.
The tag creation and artifact publication run in the same workflow, so they do
not depend on a second workflow being triggered by a bot-created tag.

## Retry and manual releases

Rerun failed jobs to recover a failed build or publication. A complete rerun on
the same release commit also works when its tag already exists. The workflow
refuses to reuse a tag pointing at a different commit. npm skips versions already
published; published npm versions cannot be replaced.

Pushing a matching `vX.Y.Z` tag or manually running **Release** also publishes
that version. For a manual branch run, Cargo.toml must already contain the
intended release version. A manual run publishes GitHub and npm artifacts;
it is not a preview. Run release preparation locally to inspect the bump:

```sh
python3 .github/scripts/prepare-release.py --dry-run
python3 .github/scripts/test-release.py
```

## Cargo installations

Install from Git using `cargo install --git https://github.com/instantOS/instantCLI --locked ins`.
To build a specific release, add `--tag vX.Y.Z`. Cargo-installed copies of the
new CLI self-update from the latest stable GitHub release tag with `--locked`.
Existing older binaries retain their old crates.io updater until reinstalled.

See [npm/PUBLISHING.md](npm/PUBLISHING.md) for npm credentials and trusted publishing.
