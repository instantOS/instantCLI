# npm release setup

The `npm` job in `.github/workflows/release.yml` reuses the two musl archives from
`cross-compile`, waits for the GitHub release, and publishes these public packages:

- `@instantos/cli-linux-x64`
- `@instantos/cli-linux-arm64`
- `@instantos/cli` (provides the `ins` executable)

Versions come from Cargo.toml and must match the checked-out `vVERSION` tag.
Binary packages publish first with exact version dependencies in the launcher.
Stable versions use `latest`; prereleases use `next`. Rerunning a failed job skips
versions already published. npm versions are immutable: corrections to a
published package require a new release version.

## First release: credentials

1. Create or obtain ownership of the `instantos` organization/scope on npm.
   GitHub organization ownership does not grant npm scope ownership. Ensure the
   publishing account can create and publish all three packages above.
2. Create an npm **granular access token** with read/write access to the
   `instantos` scope and permission to create these packages. Enable **Bypass
   2FA** for unattended CI publishing, use a short expiry, and avoid IP
   restrictions incompatible with GitHub-hosted runners.
3. Add the token as the repository Actions secret **`NPM_TOKEN`**. No npm
   username/password or new GitHub token is needed. Existing release-plz and
   GitHub release credentials remain in use.
4. After these changes are merged, create the next release normally. For an
   existing tag containing these workflow files, Actions → Release → Run
   workflow → choose that tag also works. Running against a branch does not
   publish npm packages. The existing release workflow's version check requires
   a tag; this change does not alter that behavior.

All three names must be available or already owned by the publishing account.
If `@instantos/cli` is unavailable, update the scope/names in `build-packages.py`,
`bin/ins.cjs`, `publish.cjs`, and documentation together.

## Subsequent releases: trusted publishing (recommended)

After the packages exist, configure a GitHub Actions trusted publisher in the
npm settings of **each of the three packages**:

- GitHub organization/user: `instantOS`
- Repository: `instantCLI`
- Workflow filename: `release.yml`
- Environment: leave empty (the job does not use a GitHub environment)
- Allow direct publishing with `npm publish`

Then remove/revoke `NPM_TOKEN`. The job already grants `id-token: write` and uses
Node 24 with a compatible npm CLI, so it can authenticate using GitHub OIDC.
Provenance is requested for every publish. npm scope membership and trusted
publisher settings must be configured on npm, not as GitHub secrets.

See [npm trusted publishing](https://docs.npmjs.com/trusted-publishers/) and
[npm granular tokens](https://docs.npmjs.com/creating-and-viewing-access-tokens/).

## Local validation

```sh
python3 npm/test-packaging.py
cargo check --locked
```

To inspect real release archives without publishing:

```sh
python3 npm/build-packages.py --artifacts /path/to/archives --output target/npm --tag vVERSION
npm pack ./target/npm/cli --dry-run
```

Staging verifies archive checksums and extracts only the expected regular binary
file. Installation does not run a postinstall downloader or require a Rust compiler.
