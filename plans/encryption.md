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

Identities (private keys) in `dots.toml` (user-local, not committed):

```toml
age_identity_files = [
  "~/.config/instant/age/identity",
  "~/.ssh/id_ed25519",
]
```

Fallbacks: `$AGE_IDENTITY` env, then SSH agent (rage supports this).

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
| `is_target_unmodified` | unchanged | unchanged — `Hp` on both sides |
| `is_outdated` | unchanged | unchanged — compares `Hp` to `Hp` |
| `diff` ([git/diff.rs](../src/dot/git/diff.rs)) | text diff | decrypt source for diff; if no identity available, show `[encrypted: identity required]` |

The crucial property: **age encryption is non-deterministic** (random nonce
per encryption), so re-encrypting unchanged plaintext would create spurious
git diffs on every fetch. The "skip re-encrypt if plaintext-hash unchanged"
rule in `fetch` is the only genuinely new piece of logic.

## Things that need extra care

1. **`add` command** ([operations/add.rs](../src/dot/operations/add.rs)) needs
   an `--encrypt` flag: encrypt the target into `<source>.age` instead of
   plain copy. Requires at least one recipient configured.
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

## Concrete next steps if proceeding

1. Add `age = { version = "0.11", features = ["ssh", "armor"] }` to
   `Cargo.toml`. (Pure-Rust, no system deps. The `ssh` feature enables
   `ssh-ed25519` / `ssh-rsa` recipients and identities; `armor` enables
   ASCII-armored output so the encrypted files diff cleanly in git.
   Crate is still marked BETA upstream — pin precisely and revisit
   on each bump.)
2. Extend `Dotfile` with `kind: SourceKind { Plain, Age }` resolved at scan
   time from the `.age` suffix.
3. Migrate the DB schema from version 3 → 4 in
   [db.rs](../src/dot/db.rs), adding the `encrypted_sources(cipher_hash,
   plain_hash, created)` side table described above. Do **not** modify
   `DotFileType` — it's a bool on disk.
4. Implement an `Identity` resolver: read `dots.toml.age_identity_files`,
   fall back to `$AGE_IDENTITY`, then SSH agent. Cache the parsed
   `age::Identity` per process.
5. Branch `apply` / `fetch` / `get_file_hash` on `kind`. Plain path stays
   exactly as it is today.
6. Add `add --encrypt` and surface "encrypted / skipped due to missing
   identity" states in [git/status.rs](../src/dot/git/status.rs).
7. Document the convention (`.age` suffix, recipients in
   `instantdots.toml`, identities in `dots.toml`) in `README.md` and
   `CHANGELOG.md`.

### Suggested first vertical slice

Minimal end-to-end slice to validate the design before committing:

- Extension detection in `scan_directory_for_dotfiles`.
- Identity loader (file-based only, skip SSH agent for v1).
- `decrypt-on-apply` path.
- Plaintext-hash tracking via the new cipher→plain DB mapping.
- Skip `fetch` and `add --encrypt` (second pass) — manual `age` CLI use is
  fine for seeding repos initially.

That slice exercises the entire hash-tracking change without committing
us to recipient management or interactive re-encryption flows.
