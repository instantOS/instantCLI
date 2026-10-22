# Game save ownership and synchronization

Implemented in the working tree. Private saves remain inline; shared storage has
one named owner regardless of how many games use it. The default conflict policy
is **newest saves win, preserve alternatives, notify**. Steam shortcuts never need
to open a terminal or the TUI to resolve an ordinary save conflict.

## Ownership and human-readable configuration

A private game owns its save path, history, and local sync state. A shared save
group owns those once for all participating games. Each owner covers one file or
directory and uses the same backup, restore, and synchronization implementation.

Portable `games.toml` declares group identities and game membership:

```toml
[[games]]
name = "Native Game"
launch_command = "/games/native-game"

[[games]]
name = "Final Fantasy X"
save_group = "ps2"
launch_command = "pcsx2-qt -- '/games/Final Fantasy X.iso'"

[[games]]
name = "Kingdom Hearts"
save_group = "ps2"

[[save_groups]]
id = "ps2"
name = "PS2 memory cards"
```

Device-specific `installations.toml` binds paths:

```toml
[[installations]]
game_name = "Native Game"
save_path = "~/Games/Native Game/Saves"
save_path_type = "directory"

[[save_groups]]
id = "ps2"
save_path = "~/.config/PCSX2/memcards"
save_path_type = "directory"
```

Repository settings are omitted from these examples. Checkpoints, pending restore
markers, and content baselines are generated automatically in the device config.
They are stored only on the owner, never copied to every group member.

Group members need no installation record unless they have local launch overrides
or dependencies. Such records must omit all private save fields. Device-specific
launch commands can override portable commands; they cannot override membership.

Group IDs are readable lowercase slugs such as `ps2`. Display names can change
without changing history. Changing an ID creates a different owner. Game and group
history have separate namespaces: a game named `ps2` cannot select a group named
`ps2` accidentally. Existing private game snapshot identities remain unchanged.

Unbound groups are allowed on devices that do not use them. Operations on an
unbound group report the missing binding and never fall back to a private history.
Reject duplicate IDs, undefined references, mixed private/shared ownership, and
equal or nested paths across all local owners. Resolve symlinks, existing prefixes
of missing paths, and hard-linked files. Filesystem bind-mount aliases and external
programs changing paths during an operation remain outside these checks.

## Automatic synchronization and conflict policy

Each device records the local content digest and repository head from its last
successful synchronization. This is a baseline, not a device registry or a way to
query which other devices are online.

- Only local content changed: back it up.
- Only the repository changed: restore the remote version.
- Neither changed: skip.
- Both changed, or remote history has competing heads: automatically choose the
  newest save contents, preserve the alternatives, and report that choice.
- Matching local and remote content can establish a baseline without replacing
  files. A device with no local saves can download the remote version directly.
  A fresh empty directory is treated as having no saves; its creation time cannot
  override existing remote progress. Empty data after an established baseline can
  still represent intentional deletions.

The digest includes hidden files, empty directories, symlink targets, and missing
entries. Internal symlinks must be relative and stay inside the owner; uncovered
external or absolute links are rejected. Restic is forced to read file bytes too, so changed contents with preserved
modification times cannot be skipped by its metadata cache.

New snapshots record save modification time separately from backup creation time.
Conflict selection compares the save times; a late upload of an old save does not
make its contents newest. Equal remote save times are ordered deterministically
by snapshot ID; a remote candidate wins a local/remote timestamp tie. Old private
snapshots without save-time metadata use their backup creation times.

“Newest” means filesystem modification time, not furthest game progress. Clock
skew, manually touched files, and games rewriting old progress can affect that
choice. A shared memory card is selected as one unit: progress from two different
games inside a binary card cannot be merged automatically.

Before replacing unuploaded local changes, create a recovery snapshot. When local
saves win, retain the competing remote versions. Preserved alternatives are exempt
from automatic pruning; deliberate removal remains available through restic.
Graphical shortcut launches send a notification when they resolve a conflict;
terminal commands print the selected version. Optional notification failure does
not turn a successful sync into a failure.

## Concurrent writes and durable state

New owner snapshots declare the heads they supersede. Concurrent uploads can
therefore create distinct branches rather than silently pretending one upload
included another. Automatic conflict resolution publishes the chosen contents as
a descendant of every observed branch. The next device sees one resolved head.
Private and shared owners use this same mechanism; existing private histories
without parent metadata retain their previous linear ordering as a legacy anchor.
No snapshot-format conversion or second sync engine is needed.

This uses append-only restic snapshots rather than a separate distributed lock
service. A concurrent upload arriving during resolution can create another branch;
a subsequent sync detects and resolves it. Under continuing concurrent writes an
operation can fail and be retried, while the uploaded versions remain available.

Local owner locks cover pre-sync, the tracked process lifetime, and post-sync.
Manual backup, restore, sync, pruning, and binding changes use the same locks.
Launch sync visits only the selected owner; explicit sync-all visits each locally
bound owner once and propagates failures to all its member games.

A restore persists its intent before changing live files. Completion saves the
content baseline, checkpoint, acknowledged remote head, and cleared retry marker
in one atomic replacement. Interrupted restores retry the selected snapshot before
automatic play; partial local data cannot win just because its timestamp is newer.
An unavailable retry snapshot requires explicit recovery.

Config writes use file locks, atomic replacement, and stale-write checks. Owner
state transitions reload and update under the config lock, so simultaneous sessions
for different owners cannot erase each other's checkpoints. Pruning keeps active
heads, local recovery checkpoints, preserved alternatives, and the links needed to
avoid mistaking a retained ancestor for a new branch. It refuses unresolved branches
or incomplete restores.

## PCSX2 integration

PCSX2 discovery offers its memory-card directory as one shared unit instead of
inventing a game per card. This covers both slots, folder cards, ordinary cards,
and cards created later. No database of which games have progress in each card is
needed. Restoring a group displays its name, destination, and participating games,
and states that all contained saves roll back together.

Before automatic launch sync, ask the selected PCSX2 executable to resolve its
actual data directory using `-testconfig -earlyconsolelog`. This handles native,
Flatpak, portable/AppImage, and custom `-datapath` resolution without duplicating
the emulator's directory-selection rules. The bounded probe starts no game; it
can initialize PCSX2's own settings. Versions that cannot report an unambiguous
data root are rejected before any save restore.

Read the resolved base configuration, enabled card slots, configured card directory,
and relevant override settings before restoring. `-datapath` and `-gamecfg` are
recognized structured launch arguments. For an explicit game-config file, inspect
that file. Otherwise conservatively inspect all possible per-game INIs rather
than guessing the ROM's serial/CRC. This may reject an override belonging to another
game; it cannot silently approve an uncovered card.

Every enabled card must fall within the declared owner. An individual card is
supported only when all enabled slots are covered by that same owner. Disjoint
save areas are unsupported; use the common card directory or reconfigure slots.
Unrecognized save-setting encodings fail inspection instead of being guessed.

PCSX2 launches run in batch mode so its process exits when the VM stops. Inspect
configuration again before post-exit synchronization. Shared automatic launch
verification currently supports structured PCSX2 commands. Arbitrary scripts and
other emulators can use explicitly declared shared ownership and manual group
synchronization, but cannot claim verified automatic shared launch synchronization.
The arbitrary Steam `exec` wrapper continues syncing private owners and leaves
unrelated shared groups alone.

Programs launched outside ins do not participate in its owner locks. Configuration
inspection cannot prevent users changing emulator settings during play, and it
cannot prove arbitrary scripts' writes. There is no general emulator-profile or
save-mapping framework.

## Commands and migration

```sh
ins game group create ps2 --name "PS2 memory cards" --path ~/.config/PCSX2/memcards
ins game add --name "Final Fantasy X" --save-group ps2 \
  --launch-command "pcsx2-qt -- '/games/Final Fantasy X.iso'"
ins game group bind ps2 /another/device/memcards
ins game group sync ps2
ins game group snapshots ps2
ins game group restore ps2 SNAPSHOT_ID
```

For a missing single-file binding, pass `--file` to `group create` or `group bind`.
Ordinary private games still use `game add --name ... --save-path ...`.

To migrate existing duplicated private mappings, declare a group without a path,
then explicitly remove private bindings while assigning members, and finally bind
the group. Existing snapshots and save files remain untouched:

```sh
ins game group create ps2 --name "PS2 memory cards"
ins game group join ps2 "Final Fantasy X" --discard-private-binding
ins game group join ps2 "Kingdom Hearts" --discard-private-binding
ins game group bind ps2 ~/.config/PCSX2/memcards
```

Use `group reconcile ps2 --local` to deliberately publish current local data, or
`group import ps2 "Old Game Name" SNAPSHOT_ID` to import a selected private history
version as a new group baseline. Import retains the original private snapshots,
stages the chosen files, creates a group snapshot, and restores it through the
same managed restore implementation. Storage units must match; converting an
individual card history into a different multi-card layout is explicit work.

`game reconcile GAME` and `group reconcile GROUP` also offer interactive recovery
when deliberately invoked in a terminal. Their `--local` and `--snapshot` flags
support explicit noninteractive choices. Normal synchronization and Steam shortcut
launches use the automatic policy above and never invoke those terminal menus.

Removing a game retains its group and other members. Removing a group is a separate
operation that rejects remaining references and retains snapshots.

## Restic request budget

Each automatic sync pass starts with a fresh repository snapshot listing, shared
across all save owners. It does not reuse the five-minute display cache. A no-op
sync therefore makes one repository request regardless of the number of owners.
A normal upload adds one backup and one fresh listing to detect concurrent
branches. A directory download adds one restore; single-file downloads also
inspect the snapshot contents to resolve legacy paths safely.

Nested conflict handling reuses the same observation. Every recovery or resolution
upload refreshes it, so branch detection still sees competing versions. Restores
reuse the selected snapshot metadata and do not invalidate the repository view.
Pre-launch and post-game sync are separate passes, each with fresh observations.
Version checks are local subprocesses, not repository requests. Save hashing is
local; ordinary automatic sync never backs up unchanged owners. Explicit forced
backups and conflict resolution can publish unchanged contents to record lineage.

## References

- Implementation: `src/game/{config,ownership,save_runtime,save_groups}.rs`,
  `src/game/operations/{launch,sync}.rs`, `src/game/platforms/pcsx2.rs`, and
  `src/game/restic/{backup,cache,prune,tags}.rs`.
- [PCSX2 card slots and directory resolution](https://github.com/PCSX2/pcsx2/blob/master/pcsx2/Pcsx2Config.cpp).
- [PCSX2 per-game configuration](https://github.com/PCSX2/pcsx2/blob/master/pcsx2/VMManager.cpp).
- [PCSX2 command-line arguments and configuration probe](https://github.com/PCSX2/pcsx2/blob/master/pcsx2-qt/QtHost.cpp).
- [Steam Cloud conflict behavior](https://help.steampowered.com/en/faqs/view/68D2-35AB-09A9-7678).
