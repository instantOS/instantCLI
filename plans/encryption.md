# Encrypted Dotfiles for `ins dot`

Investigation of how to introduce encryption support for dotfiles that
contain credentials (API tokens, SSH configs with secrets, `.netrc`,
mail passwords, etc.) without reinventing crypto and without breaking
the existing modified-tracking model.

## Current model (the constraint everything else has to fit)

1. **Discovery** ([scan_directory_for_dotfiles](../src/dot/utils.rs)): walk every
   active `dots_dirs` subdir; every file becomes a
   `Dotfile { source_path, target_path = home/<relpath> }`. Target path equals
   source path under `~` byte-for-byte (or under `/` for root dotfiles).
2. **Apply** ([Dotfile::apply](../src/dot/dotfile.rs)): plain
   `fs::copy(source → target)`, then record `sha256(source)` in the SQLite DB as
   the target's hash.
3. **Fetch** (reverse direction): plain `fs::copy(target → source)`.
4. **Modified-tracking** ([is_target_unmodified](../src/dot/dotfile.rs)): a
   target is "safe to overwrite" iff its sha256 equals *some* sha256 ever
   recorded for a source (in the DB at `database_dir`). `is_outdated` compares
   source hash vs target hash, with mtime as fallback.

The whole "did the user touch this file" invariant relies on
**source hash and target hash being directly comparable**. That is exactly
what naive encryption (different bytes on each side) breaks.

## Options considered

| Option | What it is | Fits `ins dot` model? | Notes |
|---|---|---|---|
| **age** (`rage` Rust crate) | Modern file encryption, X25519 + SSH key recipients, single-shot per file. Used by chezmoi, sops-nix. | ★★★ Best fit | Public recipients in repo, identities local. Pure-Rust crate, no system deps. |
| **GPG** | Classic, ubiquitous, agent integration. | ★★ OK | Big toolchain, painful keyring UX, web-of-trust baggage. Wider familiarity though. |
| **SOPS** | Encrypts *values* inside YAML/JSON/TOML/env files. | ★ partial | Only works for structured files. Useless for `~/.netrc`, `~/.ssh/config`, raw secret files. |
| **git-crypt / transcrypt** | Transparent encryption via git smudge/clean filters. | ✗ Bad fit | Encryption is tied to git checkout, not to apply. After `git pull` the source on disk is plaintext, so we lose the on-disk-encrypted invariant. Also fights hash-based modified detection. |
| **Templating** (Tera / MiniJinja / handlebars) + secret store (`pass`, keyring, 1Password CLI) | Source is a template with `{{ secret "foo" }}` placeholders; secrets stay outside the repo entirely. | ★★ Complementary | Pure one-way (render only). Fetch becomes impossible without manual template edits. Modified detection has to hash the *rendered* output, not the template. |

**Recommendation:** **age** with an extension-based convention, optionally
templating as a separate later feature. `age` is what chezmoi settled on for
the same problem; recipients are public so encryption never needs unlocked
keys, and SSH keys can be reused as identities.

## Recommended design

### File naming convention (chezmoi-style)

- `<dots_dir>/.config/foo/secrets.toml.age` — ciphertext, committed to git.
- Target path is `~/.config/foo/secrets.toml` — plaintext, suffix dropped.
- Discovery in `scan_directory_for_dotfiles` strips the `.age` suffix when
  computing `target_path` and tags the resulting `Dotfile` as encrypted.

### Configuration

Per-repo metadata in `instantdots.toml` ([RepoMetaData](../src/dot/types.rs)):

```toml
age_recipients = [
  "age1qrk...",                      # X25519 pubkey
  "ssh-ed25519 AAAA... user@host",   # SSH pubkey also works
]
```

Recipients are public; safe to commit.

Future identity configuration in `dots.toml` (user-local, not committed):

```toml
age_identity_files = [
  "~/.config/instant/age/identity",
  "~/.ssh/id_ed25519",
]
```

Current identity discovery is file-based: `$AGE_IDENTITY`, then
`<instant_config_dir>/age/identity`, then files under
`<instant_config_dir>/age/identities/`. `dots.toml.age_identity_files` and SSH
agent loading are still future work.

### Hash tracking — the key insight

Keep one hash space. Make that hash **always the plaintext sha256**, never
the ciphertext.

```diagram
╭─────────────────╮    ╭───────────────────╮
│ source          │    │ target            │
│ secrets.toml.age│    │ secrets.toml      │
│ ciphertext: Hc  │    │ plaintext: Hp     │
╰────────┬────────╯    ╰─────────┬─────────╯
         │ decrypt               │ read
         ▼                       ▼
    plaintext Hp  ◄── compare ──►  Hp
```

Add a small DB mapping `cipher_hash → plain_hash`. **Do not** try to add
a third variant to [DotFileType](../src/dot/db.rs) — it is serialized to a
SQLite bool (`SourceFile=true`, `TargetFile=false`) via the
`From<bool>`/`Into<bool>` impls. Instead, bump `CURRENT_SCHEMA_VERSION`
from 3 to 4 (the migration system already exists in
[db.rs](../src/dot/db.rs)) and add a dedicated side table:

```sql
CREATE TABLE encrypted_sources (
  cipher_hash TEXT NOT NULL PRIMARY KEY,
  plain_hash  TEXT NOT NULL,
  created     TEXT NOT NULL
);
CREATE INDEX idx_encrypted_sources_plain ON encrypted_sources(plain_hash);
```

The existing `hashes` table continues to store plaintext hashes for both
source and target rows (`SourceFile` row for the `.age` source still gets
the *plaintext* hash, so all existing comparison logic keeps working
unchanged). Then `Dotfile::get_file_hash` for an encrypted source becomes:

1. Hash the ciphertext file (cheap, existing buffered reader).
2. Look up `cipher_hash → plain_hash`.
3. On miss: decrypt once, sha256 the plaintext, record both.

Consequence: `ins dot status`, `ins dot diff`, conflict UI, and background
`apply` do **not** need the identity unlocked every time — only when the
ciphertext actually changes upstream. Critical because `apply` runs in the
autostart path.

### Per-operation behaviour

| Operation | Plain source | Age source |
|---|---|---|
| `apply` | `fs::copy(src, tgt)` | `age::decrypt(src) → tgt`, record plaintext hash for target |
| `fetch` | `fs::copy(tgt, src)` | If `Hp(tgt) == Hp(src_cached)` → no-op (avoids churning repo with new nonces). Else `age::encrypt(tgt, recipients) → src`, update cache. |
| `encrypt` | Convert tracked plaintext source to `<src>.age`, remove plaintext source, record plaintext hashes, stage both git changes. | Already encrypted; no-op/error. |
| `is_target_unmodified` | unchanged | unchanged — `Hp` on both sides |
| `is_outdated` | unchanged | unchanged — compares `Hp` to `Hp` |
| `diff` ([git/diff.rs](../src/dot/git/diff.rs)) | text diff | decrypt source for diff; if no identity available, show `[encrypted: identity required]` |

The crucial property: **age encryption is non-deterministic** (random nonce
per encryption), so re-encrypting unchanged plaintext would create spurious
git diffs on every fetch. The "skip re-encrypt if plaintext-hash unchanged"
rule in `fetch` is the only genuinely new piece of logic.

## Things that need extra care

1. **`add` command** ([operations/add.rs](../src/dot/operations/add.rs)) still
   needs an `--encrypt` flag for new files. `ins dot encrypt <path>` handles
   conversion of already tracked plaintext sources, but not first-time add.
2. **Background apply / autostart**: when no identity is unlockable (no
   agent, no passphrase available), encrypted entries should be **skipped
   with a warning**, not error out. Plain dotfiles must still apply.
3. **Identity discovery**: support `dots.toml.age_identity_files`,
   `$AGE_IDENTITY` env, SSH agent. Never prompt for passphrases in
   background contexts.
4. **`ignored_paths` and `units`** must match against the *target* path
   (post-suffix-stripping), not the source.
5. **`.insignore` / git history**: converting an existing plaintext dotfile
   to encrypted does **not** remove the secret from git history. Document
   that this requires `git filter-repo` if the secret was already pushed.
6. **Tests**: the in-memory `HASH_CACHE` in `dotfile.rs` and the DB hash
   cache both need correct invalidation when ciphertext changes. The
   `external_metadata_tests.rs` style covers this pattern.
7. **Alternative-source picking** in
   [src/dot/operations/alternative/](../src/dot/operations/alternative/)
   (the UI that lets the user choose which repo provides a given target)
   must handle the case where displaying a source requires decryption —
   show a placeholder when no identity is available rather than failing.
8. **Cross-repo precedence**: if the same target has both a plain source in
   one repo and an `.age` source in another, the existing
   [default_source_for](../src/dot/sources.rs) priority logic applies — but
   `list_sources_for_target` and `list_sources_by_target_in_dir` in
   [sources.rs](../src/dot/sources.rs) must strip `.age` when computing the
   relative-to-target match, and `merge_dotfiles` in
   [utils.rs](../src/dot/utils.rs) keys by target path so that already
   works once discovery strips the suffix.

## Templating (deferred, complementary)

A separate later feature: source file like `~/.config/foo.tmpl` with
`{{ secret "github_token" }}` placeholders. Renderer pulls from `pass`,
`keyring`, `1password`, env at apply time. Modified-tracking would have to
hash the rendered output, and `fetch` is essentially impossible
(plaintext → template diff is not derivable). Use **MiniJinja** (Rust,
small, well-maintained) if/when this is built. Recommend not mixing the
two features in v1.

## Current implementation status

The first vertical slice plus tracked-source conversion is implemented, but this
is still not full encryption support.

Implemented:

- `age = { version = "0.11", features = ["ssh", "armor"] }` dependency.
- `.age` source detection with `SourceKind::{Plain, Age}`.
- Discovery strips `.age` when computing target paths.
- Schema version 4 with `encrypted_sources(cipher_hash, plain_hash, created)`.
- File-based identity discovery:
  - `$AGE_IDENTITY` as colon-separated paths.
  - `age_identity_files` in `dots.toml` (global user config).
  - `<instant_config_dir>/age/identity`.
  - `<instant_config_dir>/age/identities/*`.
- `instantdots.toml` `age_recipients = [...]` metadata for public X25519 and
  SSH recipients.
- Recipient parsing and ASCII-armored age encryption helpers.
- `apply` / `reset` decrypt encrypted sources into plaintext targets.
- `status` exposes an explicit `identity_required` state for encrypted files
  that need a local identity.
- Background `apply` skips encrypted files that cannot be decrypted, while
  continuing to process plain dotfiles.
- `diff` decrypts encrypted sources to a temp plaintext file before invoking
  `delta`; when no identity works, it prints an identity-required placeholder.
- Alternative source lookup considers both plain and `.age` files, and persisted
  overrides re-resolve `.age` source paths.
- `ins dot encrypt <path>` converts an already tracked plaintext source into a
  `.age` source, records plaintext hashes, stages the source deletion and new
  ciphertext, and warns that git history is unchanged.
- `ins dot add --encrypt` adds new files as encrypted `.age` sources, and
  updates tracked encrypted files via encrypted `fetch` (nonce-avoiding
  no-op when target is unchanged).
- Encrypted `fetch` / update-back-to-repo for tracked encrypted files, with
  hash-comparison no-op to avoid nonce-only git churn.
- `age_identity_files` config in `dots.toml` for specifying local identity
  paths (tilde-expanded, loaded after `$AGE_IDENTITY`, before default paths).
- README and changelog documentation for the current CLI boundary.

Still missing:

- SSH agent identity loading.
- TUI flows for encryption setup and encrypted add/update operations.
- Broader end-to-end tests for the CLI surface, especially multi-source
  selection and missing recipient errors.

## Recommended next steps

### Phase 1: Stabilize the vertical slice

Mostly done for the core manual `.age` workflow. Keep these as regression
coverage and add CLI-level tests around the new conversion command.

- Existing coverage now includes the core `.age` paths. Remaining useful
  integration/CLI coverage:
  - scanning `foo.age` as source and `foo` as target;
  - status reporting `identity_required` when no matching identity is present;
  - apply skipping encrypted files without aborting plain files;
  - repo clone/add hash seeding recording plaintext hashes for encrypted files;
  - persisted alternatives resolving `target` to `target.age`;
  - diff displaying a placeholder when no identity can decrypt.
- README documentation now covers the current workflow:
  - place encrypted files as `<target>.age` in a dots dir;
  - configure identities via `$AGE_IDENTITY` or
    `~/.config/instant/age/identity`;
  - `apply`, `reset`, `status`, and `diff` behavior;
  - `add` and `fetch` are not yet supported for encrypted sources.
- Changelog names this as a manual `.age` apply/status/diff slice plus tracked
  source conversion, not full encryption support.

### Phase 2: Recipient configuration

Implemented for repository metadata and the conversion command. Follow-up:
surface it in repo info/status views enough for debugging, but avoid noisy
output in normal `ins dot status`.

- `RepoMetaData` / `instantdots.toml` now supports:

  ```toml
  age_recipients = [
    "age1...",
    "ssh-ed25519 AAAA... user@host",
  ]
  ```

- Parsing/validation helpers now exist in `dot::encryption`:
  - read recipients for the repo that owns a source path;
  - parse X25519 and SSH recipients using the `age` crate;
  - return a clear error when no recipients are configured.

### Phase 3: `ins dot encrypt <path>`

Implemented as the first creation/conversion workflow for already tracked
plaintext dotfiles.

- Requires `age_recipients` in the owning repo's `instantdots.toml`.
- Converts the tracked source from `source` to `source.age`.
- Encrypts current target bytes when the target exists; otherwise encrypts the
  existing source bytes.
- Records plaintext hashes for encrypted source/target tracking.
- Stages the plaintext source deletion and encrypted source addition.
- Supports `--repo`, `--subdir`, and `--dry-run`.
- Warns that converting does not remove plaintext from git history.

Additional hardening still worth doing:

- Add CLI-level tests for `--dry-run`, missing recipients, already encrypted
  sources, read-only repos, and ambiguous multi-source targets.
- Consider a dedicated recipient management command only if editing
  `instantdots.toml` manually proves too error-prone.

### Phase 4: `ins dot add --encrypt`

Implemented.

- `--encrypt` flag on `ins dot add` (`src/dot/commands.rs:61-63`).
- New files are written as `<target>.age`, encrypted to repo recipients.
- Tracked encrypted files updated via `dotfile.fetch()` in
  `update_single_dotfile`.
- Repeated fetch of unchanged target plaintext is a no-op (hash comparison
  avoids nonce-only git churn).
- Missing recipients produce a clear error message.
- Existing plaintext files warn that git history is not cleaned.

### Phase 5: Encrypted `fetch`

Implemented.

- `Dotfile::fetch()` handles `SourceKind::Age` end-to-end:
  - loads recipients from repo metadata;
  - compares target plaintext hash to cached source plaintext hash;
  - if equal → no-op (avoids nonce-only diffs);
  - if different → encrypts target → `.age` source, updates DB mapping.
- Tested by `test_fetch_age_encrypted_source` in `src/dot/dotfile.rs`.

### Phase 6: TUI support

Keep the TUI as a wrapper around the same CLI semantics.

- Display encrypted sources with an encrypted/identity-required marker in dot
  menus and previews.
- Add an "Encrypt tracked file" action that maps to `ins dot encrypt <path>`.
- Add an "Add encrypted" action wherever `ins dot add` is exposed once
  `ins dot add --encrypt` exists.
- Add a repo details action for managing `age_recipients` in
  `instantdots.toml`.
- Add a local settings action for managing `age_identity_files` in
  `dots.toml` (currently must be edited manually).
- Never prompt for passphrases from autostart/background apply. Interactive TUI
  prompts can be added later only if the identity loader can clearly distinguish
  interactive from background contexts.

### Phase 7: Optional identity improvements

These are useful but should not block the add/fetch path.

- Add SSH agent identity support if the `age` crate exposes a reliable
  non-blocking path for it.
- Consider a small `ins dot encryption doctor` command if setup errors are
  common in practice.
