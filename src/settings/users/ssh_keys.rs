use std::ffi::CString;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};

use anyhow::{Context, Result, bail};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::models::UserInfo;
use crate::common::display_server::DisplayServer;
use crate::menu_utils::{FzfPreview, FzfSelectable, FzfWrapper};
use crate::settings::context::SettingsContext;
use crate::ui::catppuccin::{colors, format_icon, format_icon_colored};
use crate::ui::prelude::*;
use crate::ui::preview::PreviewBuilder;

#[derive(Clone, Debug, PartialEq, Eq)]
struct AuthorizedKey {
    line_index: usize,
    original_line: String,
    prefix: String,
    options: String,
    key_type: String,
    key_data: String,
    comment: String,
}

impl AuthorizedKey {
    fn label(&self) -> &str {
        if self.comment.is_empty() {
            "Unnamed key"
        } else {
            &self.comment
        }
    }

    fn fingerprint(&self) -> String {
        let Ok(blob) = base64::engine::general_purpose::STANDARD.decode(&self.key_data) else {
            return "Invalid key data".to_string();
        };
        let digest = Sha256::digest(blob);
        format!(
            "SHA256:{}",
            base64::engine::general_purpose::STANDARD_NO_PAD.encode(digest)
        )
    }

    fn serialized_with_comment(&self, comment: &str) -> String {
        if comment.trim().is_empty() {
            self.prefix.clone()
        } else {
            format!("{} {}", self.prefix, comment.trim())
        }
    }

    /// The shareable public-key line, without `authorized_keys` options that
    /// only apply to this machine.
    fn public_key_line(&self) -> String {
        if self.comment.is_empty() {
            format!("{} {}", self.key_type, self.key_data)
        } else {
            format!("{} {} {}", self.key_type, self.key_data, self.comment)
        }
    }
}

/// Where one account's `authorized_keys` file lives and who may touch it.
///
/// Files for the account running this process are read and written directly.
/// Another account can control its own home-directory paths, so following
/// them with elevated privileges would turn a key edit into an arbitrary
/// privileged write. Every foreign access therefore goes through a helper
/// that permanently drops to that account before resolving its home path.
/// The target account must be able to read the existing file and create and
/// rename files in its target directory.
struct AuthorizedKeysFile {
    /// Account name from the passwd database, the helper's target identity.
    username: String,
    path: PathBuf,
    owner_uid: u32,
}

impl AuthorizedKeysFile {
    fn for_user(info: &UserInfo) -> Result<Self> {
        if info.home.as_os_str().is_empty() {
            bail!("user has no home directory");
        }

        Ok(Self {
            username: info.username.clone(),
            path: info.home.join(".ssh/authorized_keys"),
            owner_uid: info.uid,
        })
    }

    /// True when this file belongs to the user running this process, so
    /// reads and writes already happen with the right ownership.
    fn is_current(&self) -> bool {
        self.owner_uid == current_uid()
    }

    fn read_keys(&self, ctx: &SettingsContext) -> Result<Vec<AuthorizedKey>> {
        Ok(self
            .read_lines(ctx)?
            .iter()
            .enumerate()
            .filter_map(|(index, line)| parse_authorized_key(line, index))
            .collect())
    }

    fn read_lines(&self, ctx: &SettingsContext) -> Result<Vec<String>> {
        let Some(contents) = self.contents(ctx)? else {
            return Ok(Vec::new());
        };

        Ok(contents.lines().map(str::to_string).collect())
    }

    /// Returns `None` when the file does not exist yet.
    fn contents(&self, ctx: &SettingsContext) -> Result<Option<String>> {
        if self.is_current() {
            read_contents_direct(&self.path)
        } else {
            read_contents_as_user(ctx, &self.username, &self.path)
        }
    }

    fn write_lines_if_unchanged(
        &self,
        ctx: &SettingsContext,
        expected: Option<&str>,
        lines: &[String],
    ) -> Result<()> {
        let replacement = serialize_lines(lines);
        if self.is_current() {
            replace_contents_atomically(&self.path, expected, &replacement)
        } else {
            write_contents_as_user(ctx, &self.username, &self.path, expected, &replacement)
        }
    }

    fn replace_key_line(
        &self,
        ctx: &SettingsContext,
        key: &AuthorizedKey,
        replacement: &str,
    ) -> Result<()> {
        let expected = self.contents(ctx)?;
        let mut lines = lines_from_contents(expected.as_deref());
        replace_line(&mut lines, key, replacement)?;
        self.write_lines_if_unchanged(ctx, expected.as_deref(), &lines)
    }

    fn remove_key_line(&self, ctx: &SettingsContext, key: &AuthorizedKey) -> Result<()> {
        let expected = self.contents(ctx)?;
        let mut lines = lines_from_contents(expected.as_deref());
        remove_line(&mut lines, key)?;
        self.write_lines_if_unchanged(ctx, expected.as_deref(), &lines)
    }
}

#[derive(Clone)]
enum KeyMenuItem {
    Key(AuthorizedKey),
    Add,
    Back,
}

impl FzfSelectable for KeyMenuItem {
    fn fzf_display_text(&self) -> String {
        match self {
            Self::Key(key) => format!("{} {}", format_icon(NerdFont::Key), key.label()),
            Self::Add => format!(
                "{} Add SSH key",
                format_icon_colored(NerdFont::Plus, colors::GREEN)
            ),
            Self::Back => format!(
                "{} Back",
                format_icon_colored(NerdFont::ArrowLeft, colors::OVERLAY0)
            ),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            Self::Key(key) => PreviewBuilder::new()
                .header(NerdFont::Key, key.label())
                .field("Type", &key.key_type)
                .field("Fingerprint", &key.fingerprint())
                .blank()
                .subtext("Select to manage this key.")
                .build(),
            Self::Add => PreviewBuilder::new()
                .header(NerdFont::Plus, "Add SSH Key")
                .text("Authorize a new SSH public key for this account.")
                .blank()
                .subtext("Paste one complete OpenSSH public-key line.")
                .build(),
            Self::Back => PreviewBuilder::new()
                .header(NerdFont::ArrowLeft, "Back")
                .text("Return to settings.")
                .build(),
        }
    }
}

#[derive(Clone)]
enum KeyActionItem {
    Copy(AuthorizedKey),
    EditComment,
    Remove,
    Back,
}

impl FzfSelectable for KeyActionItem {
    fn fzf_display_text(&self) -> String {
        match self {
            Self::Copy(_) => format!(
                "{} Copy public key",
                format_icon_colored(NerdFont::Clipboard, colors::GREEN)
            ),
            Self::EditComment => format!("{} Edit comment", format_icon(NerdFont::Edit)),
            Self::Remove => format!(
                "{} Remove key",
                format_icon_colored(NerdFont::Trash, colors::RED)
            ),
            Self::Back => format!("{} Back", format_icon(NerdFont::ArrowLeft)),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            Self::Copy(key) => PreviewBuilder::new()
                .header(NerdFont::Clipboard, "Copy Public Key")
                .text("Copy this key to the system clipboard.")
                .blank()
                .field("Key", &key.public_key_line())
                .build(),
            Self::EditComment => PreviewBuilder::new()
                .header(NerdFont::Edit, "Edit Comment")
                .text("Change the label at the end of this public key.")
                .blank()
                .subtext("The key itself and its access restrictions remain unchanged.")
                .build(),
            Self::Remove => PreviewBuilder::new()
                .header(NerdFont::Trash, "Remove SSH Key")
                .line(
                    colors::RED,
                    Some(NerdFont::Warning),
                    "This key will no longer be able to log in.",
                )
                .build(),
            Self::Back => FzfPreview::Text("Return to the SSH key list.".to_string()),
        }
    }
}

/// Manage an account's `authorized_keys`: edited directly when it belongs
/// to the current user, through sudo otherwise.
pub fn manage_ssh_keys(ctx: &mut SettingsContext, info: &UserInfo) -> Result<()> {
    let keys = AuthorizedKeysFile::for_user(info)?;
    loop {
        let mut items: Vec<_> = keys
            .read_keys(ctx)?
            .into_iter()
            .map(KeyMenuItem::Key)
            .collect();
        items.push(KeyMenuItem::Add);
        items.push(KeyMenuItem::Back);

        match FzfWrapper::menu().items(items).padded().select_one()? {
            crate::menu_utils::DialogOutcome::Submitted(KeyMenuItem::Key(key)) => {
                manage_key(ctx, &keys, &key)?
            }
            crate::menu_utils::DialogOutcome::Submitted(KeyMenuItem::Add) => add_key(ctx, &keys)?,
            _ => break,
        }
    }

    Ok(())
}

fn parse_authorized_key(line: &str, line_index: usize) -> Option<AuthorizedKey> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    let tokens = whitespace_tokens_with_offsets(trimmed)?;
    let type_index = tokens.iter().position(|(_, _, token)| is_key_type(token))?;
    let (type_start, _, key_type) = tokens[type_index];
    let (_, data_end, key_data) = *tokens.get(type_index + 1)?;
    validate_key_blob(key_type, key_data).ok()?;

    Some(AuthorizedKey {
        line_index,
        original_line: line.to_string(),
        prefix: trimmed[..data_end].to_string(),
        options: trimmed[..type_start].trim().to_string(),
        key_type: key_type.to_string(),
        key_data: key_data.to_string(),
        comment: trimmed[data_end..].trim().to_string(),
    })
}

fn whitespace_tokens_with_offsets(value: &str) -> Option<Vec<(usize, usize, &str)>> {
    let mut cursor = 0;
    let mut tokens = Vec::new();
    for token in value.split_whitespace() {
        let relative_start = value.get(cursor..)?.find(token)?;
        let start = cursor + relative_start;
        let end = start + token.len();
        cursor = end;
        tokens.push((start, end, token));
    }
    Some(tokens)
}

fn is_key_type(value: &str) -> bool {
    value.starts_with("ssh-")
        || value.starts_with("ecdsa-sha2-")
        || value.starts_with("sk-ssh-")
        || value.starts_with("sk-ecdsa-")
}

fn validate_key_blob(key_type: &str, key_data: &str) -> Result<()> {
    let blob = base64::engine::general_purpose::STANDARD
        .decode(key_data)
        .context("public key is not valid base64")?;
    let length_bytes: [u8; 4] = blob
        .get(..4)
        .context("public key data is truncated")?
        .try_into()?;
    let algorithm_length = u32::from_be_bytes(length_bytes) as usize;
    let algorithm = blob
        .get(4..4 + algorithm_length)
        .context("public key algorithm is truncated")?;
    if algorithm != key_type.as_bytes() {
        bail!("public key type does not match its encoded data");
    }
    Ok(())
}

fn manage_key(
    ctx: &mut SettingsContext,
    keys: &AuthorizedKeysFile,
    key: &AuthorizedKey,
) -> Result<()> {
    loop {
        match FzfWrapper::menu()
            .items(vec![
                KeyActionItem::Copy(key.clone()),
                KeyActionItem::EditComment,
                KeyActionItem::Remove,
                KeyActionItem::Back,
            ])
            .padded()
            .select_one()?
        {
            crate::menu_utils::DialogOutcome::Submitted(KeyActionItem::Copy(_)) => {
                copy_public_key(ctx, key);
            }
            crate::menu_utils::DialogOutcome::Submitted(KeyActionItem::EditComment) => {
                let comment = FzfWrapper::builder()
                    .prompt("SSH key comment")
                    .query(&key.comment)
                    .input()
                    .input_dialog()?;
                let crate::menu_utils::DialogOutcome::Submitted(comment) = comment else {
                    continue;
                };
                keys.replace_key_line(ctx, key, &key.serialized_with_comment(&comment))?;
                ctx.emit_success("settings.users.ssh_keys", "SSH key comment updated.");
                break;
            }
            crate::menu_utils::DialogOutcome::Submitted(KeyActionItem::Remove) => {
                let result = FzfWrapper::builder()
                    .confirm(format!("Remove SSH key {:?}?", key.label()))
                    .yes_text("Remove key")
                    .no_text("Cancel")
                    .confirm_dialog()?;
                if matches!(result, crate::menu_utils::ConfirmResult::Yes) {
                    keys.remove_key_line(ctx, key)?;
                    ctx.emit_success("settings.users.ssh_keys", "SSH key removed.");
                    break;
                }
            }
            _ => break,
        }
    }
    Ok(())
}

fn copy_public_key(ctx: &SettingsContext, key: &AuthorizedKey) {
    let display_server = DisplayServer::detect();
    match crate::assist::utils::copy_to_clipboard(key.public_key_line().as_bytes(), &display_server)
    {
        Ok(()) => ctx.emit_success("settings.users.ssh_keys", "Public key copied to clipboard."),
        Err(error) => ctx.emit_failure(
            "settings.users.ssh_keys",
            &format!("Failed to copy to clipboard: {error}. Ensure wl-copy (Wayland) or xclip (X11) is installed."),
        ),
    }
}

fn add_key(ctx: &mut SettingsContext, keys: &AuthorizedKeysFile) -> Result<()> {
    let input = match FzfWrapper::builder()
        .prompt("Paste SSH public key")
        .input()
        .input_dialog()?
    {
        crate::menu_utils::DialogOutcome::Submitted(input) => input,
        crate::menu_utils::DialogOutcome::Cancelled => return Ok(()),
    };
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    let Some(new_key) = parse_authorized_key(trimmed, 0) else {
        ctx.emit_failure(
            "settings.users.ssh_keys",
            "Invalid SSH public key. Paste one complete OpenSSH public-key line.",
        );
        return Ok(());
    };
    let expected = keys.contents(ctx)?;
    let mut lines = lines_from_contents(expected.as_deref());
    if lines.iter().enumerate().any(|(index, line)| {
        parse_authorized_key(line, index).is_some_and(|key| same_authorization(&key, &new_key))
    }) {
        ctx.emit_info(
            "settings.users.ssh_keys",
            "This SSH key is already authorized.",
        );
        return Ok(());
    }

    lines.push(trimmed.to_string());
    keys.write_lines_if_unchanged(ctx, expected.as_deref(), &lines)?;
    ctx.emit_success("settings.users.ssh_keys", "SSH key added.");
    Ok(())
}

fn same_authorization(left: &AuthorizedKey, right: &AuthorizedKey) -> bool {
    left.key_data == right.key_data && left.options == right.options
}

/// Replaces the line `key` was parsed from, refusing when the file changed
/// underneath the menu.
fn replace_line(lines: &mut [String], key: &AuthorizedKey, replacement: &str) -> Result<()> {
    let line = lines
        .get_mut(key.line_index)
        .context("SSH key changed while the menu was open")?;
    ensure_same_key(line, key)?;
    *line = replacement.to_string();
    Ok(())
}

/// Removes the line `key` was parsed from, refusing when the file changed
/// underneath the menu.
fn remove_line(lines: &mut Vec<String>, key: &AuthorizedKey) -> Result<()> {
    if key.line_index >= lines.len() {
        bail!("SSH key changed while the menu was open");
    }
    ensure_same_key(&lines[key.line_index], key)?;
    lines.remove(key.line_index);
    Ok(())
}

fn ensure_same_key(line: &str, expected: &AuthorizedKey) -> Result<()> {
    if line != expected.original_line {
        bail!("SSH key changed while the menu was open");
    }
    Ok(())
}

/// The effective uid of this process, which decides file access rights.
fn current_uid() -> u32 {
    nix::unistd::geteuid().as_raw()
}

/// Reads a file the current process is allowed to open; `None` if missing.
fn read_contents_direct(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("reading {}", path.display())),
    }
}

/// Starts the internal user helper through the normal root command boundary.
/// The helper permanently drops privileges before resolving any user path.
fn run_user_helper(
    ctx: &SettingsContext,
    username: &str,
    args: &[&std::ffi::OsStr],
) -> Result<std::process::Output> {
    let executable = crate::common::shell::resolve_current_binary();
    ctx.command_as_root(&executable, args.iter().copied())
        .output()
        .with_context(|| format!("starting the authorized_keys helper for user {username}"))
}

/// Fails when `output` reports failure, including the command's stderr.
fn require_user_command_success(username: &str, what: &str, output: &Output) -> Result<()> {
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim();
    if stderr.is_empty() {
        bail!(
            "{what} failed as user {username} with status {:?}",
            output.status.code()
        );
    }
    bail!(
        "{what} failed as user {username} with status {:?}: {stderr}",
        output.status.code()
    );
}

/// Decodes captured stdout as UTF-8, failing instead of replacing bytes:
/// a lossy decode would be written back and silently corrupt unrelated
/// lines on the next whole-file rewrite.
fn captured_user_stdout(username: &str, what: &str, output: Output) -> Result<String> {
    require_user_command_success(username, what, &output)?;
    String::from_utf8(output.stdout)
        .with_context(|| format!("{what} as user {username} did not produce UTF-8 output"))
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AuthorizedKeysSnapshot {
    version: u8,
    contents: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AuthorizedKeysUpdate {
    version: u8,
    expected: Option<String>,
    replacement: String,
}

const AUTHORIZED_KEYS_PROTOCOL_VERSION: u8 = 1;

/// Reads another user's `authorized_keys` through this binary running as
/// that user. Using a structured response distinguishes a missing file from
/// an empty one without assigning special meanings to process exit codes.
fn read_contents_as_user(
    ctx: &SettingsContext,
    username: &str,
    path: &Path,
) -> Result<Option<String>> {
    let args = [
        std::ffi::OsStr::new("settings"),
        std::ffi::OsStr::new("internal-read-authorized-keys"),
        std::ffi::OsStr::new("--username"),
        std::ffi::OsStr::new(username),
    ];
    let output = run_user_helper(ctx, username, &args)?;
    let stdout = captured_user_stdout(username, &format!("reading {}", path.display()), output)?;
    let snapshot: AuthorizedKeysSnapshot = serde_json::from_str(&stdout)
        .with_context(|| format!("decoding the authorized_keys response for user {username}"))?;
    if snapshot.version != AUTHORIZED_KEYS_PROTOCOL_VERSION {
        bail!(
            "unsupported authorized_keys helper response version {}",
            snapshot.version
        );
    }
    Ok(snapshot.contents)
}

fn lines_from_contents(contents: Option<&str>) -> Vec<String> {
    contents
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

fn serialize_lines(lines: &[String]) -> String {
    let mut contents = String::new();
    for line in lines {
        contents.push_str(line);
        contents.push('\n');
    }
    contents
}

/// Atomically replaces `path` only if its contents still match `expected`.
/// The second comparison happens after the replacement has been fully written
/// and synced, keeping the remaining race window immediately around rename.
fn replace_contents_atomically(
    path: &Path,
    expected: Option<&str>,
    replacement: &str,
) -> Result<()> {
    if read_contents_direct(path)?.as_deref() != expected {
        bail!("SSH keys changed while the edit was being saved");
    }

    let ssh_dir = path
        .parent()
        .context("authorized_keys has no parent directory")?;
    if !ssh_dir.exists() {
        // Plain create: the current user's home directory must already
        // exist, and nothing outside `.ssh` should spring into existence.
        fs::DirBuilder::new()
            .mode(0o700)
            .create(ssh_dir)
            .with_context(|| format!("creating {}", ssh_dir.display()))?;
    }

    let target = resolve_write_target(path)?;
    let target_dir = target
        .parent()
        .context("authorized_keys target has no parent directory")?;
    let mut temporary = tempfile::NamedTempFile::new_in(target_dir)
        .with_context(|| format!("creating temporary file in {}", target_dir.display()))?;
    temporary
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))?;
    temporary.write_all(replacement.as_bytes())?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;

    let current_target = resolve_write_target(path)?;
    let current_contents = read_contents_direct(path)?;
    if current_target != target || current_contents.as_deref() != expected {
        bail!("SSH keys changed while the edit was being saved");
    }

    let persisted = temporary
        .persist(&target)
        .map_err(|err| err.error)
        .with_context(|| format!("atomically replacing {}", target.display()))?;
    persisted.sync_all()?;

    fs::File::open(target_dir)?.sync_all()?;
    Ok(())
}

fn resolve_write_target(path: &Path) -> Result<PathBuf> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => fs::canonicalize(path)
            .with_context(|| format!("resolving authorized_keys symlink {}", path.display())),
        Ok(_) => Ok(path.to_path_buf()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(path.to_path_buf()),
        Err(err) => Err(err).with_context(|| format!("inspecting {}", path.display())),
    }
}

/// Requests one optimistic-concurrency-checked update from this binary running
/// as the target account. The helper owns directory creation, symlink
/// resolution, temporary file creation, syncing, and rename, so no privileged
/// process follows paths controlled by that account.
fn write_contents_as_user(
    ctx: &SettingsContext,
    username: &str,
    path: &Path,
    expected: Option<&str>,
    replacement: &str,
) -> Result<()> {
    let args = [
        std::ffi::OsStr::new("settings"),
        std::ffi::OsStr::new("internal-update-authorized-keys"),
        std::ffi::OsStr::new("--username"),
        std::ffi::OsStr::new(username),
    ];
    let update = AuthorizedKeysUpdate {
        version: AUTHORIZED_KEYS_PROTOCOL_VERSION,
        expected: expected.map(str::to_string),
        replacement: replacement.to_string(),
    };
    let payload = serde_json::to_vec(&update).context("encoding the authorized_keys update")?;
    let executable = crate::common::shell::resolve_current_binary();
    let mut child = ctx
        .command_as_root(&executable, args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("starting the write of {}", path.display()))?;
    let mut stdin = child.stdin.take().context("sudo child has no stdin")?;
    let write_error = stdin.write_all(&payload).err();
    drop(stdin);
    let output = child
        .wait_with_output()
        .context("waiting for the write to finish")?;
    require_user_command_success(username, &format!("writing {}", path.display()), &output)?;

    // A failed write only surfaces if the command itself succeeded, since
    // the exit status carries the better message.
    if let Some(err) = write_error {
        return Err(err).context("piping the new contents to sudo");
    }
    Ok(())
}

/// Internal helper endpoint. It drops every user and group identity from root
/// to `username` before deriving the account's fixed authorized_keys path.
pub(in crate::settings) fn read_authorized_keys_for_helper(username: &str) -> Result<()> {
    let path = drop_privileges_for_authorized_keys_helper(username)?;
    let snapshot = AuthorizedKeysSnapshot {
        version: AUTHORIZED_KEYS_PROTOCOL_VERSION,
        contents: read_contents_direct(&path)?,
    };
    serde_json::to_writer(std::io::stdout().lock(), &snapshot)
        .context("encoding the authorized_keys snapshot")?;
    Ok(())
}

/// Internal helper endpoint for a durable, optimistic-concurrency-checked
/// authorized_keys update. The request is read only from stdin so key material
/// never appears in argv or a staging file outside the target directory.
pub(in crate::settings) fn update_authorized_keys_for_helper(username: &str) -> Result<()> {
    let path = drop_privileges_for_authorized_keys_helper(username)?;
    let update: AuthorizedKeysUpdate = serde_json::from_reader(std::io::stdin().lock())
        .context("decoding authorized_keys update")?;
    if update.version != AUTHORIZED_KEYS_PROTOCOL_VERSION {
        bail!(
            "unsupported authorized_keys update version {}",
            update.version
        );
    }
    replace_contents_atomically(&path, update.expected.as_deref(), &update.replacement)
}

fn drop_privileges_for_authorized_keys_helper(username: &str) -> Result<PathBuf> {
    if !nix::unistd::Uid::effective().is_root() {
        bail!("authorized_keys user helper must start with root privileges");
    }

    let user = nix::unistd::User::from_name(username)
        .with_context(|| format!("looking up authorized_keys helper account {username}"))?
        .with_context(|| format!("no passwd entry for user {username}"))?;
    let path = authorized_keys_path_for_user(&user)?;
    let username = CString::new(user.name).context("username contains a null byte")?;

    nix::unistd::initgroups(&username, user.gid).with_context(|| {
        format!(
            "initializing groups for user {}",
            username.to_string_lossy()
        )
    })?;
    nix::unistd::setresgid(user.gid, user.gid, user.gid)
        .with_context(|| format!("dropping group privileges to gid {}", user.gid.as_raw()))?;
    nix::unistd::setresuid(user.uid, user.uid, user.uid)
        .with_context(|| format!("dropping user privileges to uid {}", user.uid.as_raw()))?;

    if nix::unistd::Uid::current() != user.uid || nix::unistd::Uid::effective() != user.uid {
        bail!("authorized_keys helper did not fully drop user privileges");
    }
    Ok(path)
}

fn authorized_keys_path_for_user(user: &nix::unistd::User) -> Result<PathBuf> {
    if user.dir.as_os_str().is_empty() {
        bail!("authorized_keys helper account has no home directory");
    }
    Ok(user.dir.join(".ssh/authorized_keys"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_line(key_type: &str, comment: &str) -> String {
        let mut blob = Vec::new();
        blob.extend_from_slice(&(key_type.len() as u32).to_be_bytes());
        blob.extend_from_slice(key_type.as_bytes());
        blob.extend_from_slice(b"key payload");
        let encoded = base64::engine::general_purpose::STANDARD.encode(blob);
        format!("{key_type} {encoded} {comment}")
    }

    fn parse_authorized_keys(contents: &str) -> Vec<AuthorizedKey> {
        contents
            .lines()
            .enumerate()
            .filter_map(|(line_index, line)| parse_authorized_key(line, line_index))
            .collect()
    }

    fn read_keys_from(path: &Path) -> Vec<AuthorizedKey> {
        parse_authorized_keys(&read_contents_direct(path).unwrap().unwrap_or_default())
    }

    fn read_lines_from(path: &Path) -> Vec<String> {
        read_contents_direct(path)
            .unwrap()
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn parses_key_and_comment() {
        let line = key_line("ssh-ed25519", "laptop key");
        let key = parse_authorized_key(&line, 4).unwrap();

        assert_eq!(key.line_index, 4);
        assert_eq!(key.key_type, "ssh-ed25519");
        assert_eq!(key.comment, "laptop key");
        assert!(key.fingerprint().starts_with("SHA256:"));
    }

    #[test]
    fn parses_key_with_authorized_keys_options() {
        let line = format!(
            "from=\"192.0.2.1\",no-agent-forwarding {}",
            key_line("ssh-ed25519", "restricted")
        );
        let key = parse_authorized_key(&line, 0).unwrap();

        assert!(key.prefix.starts_with("from="));
        assert_eq!(key.comment, "restricted");
    }

    #[test]
    fn preserves_spacing_inside_quoted_options() {
        let line = format!(
            "command=\"echo   hello world\",no-pty {}",
            key_line("ssh-ed25519", "restricted")
        );
        let key = parse_authorized_key(&line, 0).unwrap();

        assert_eq!(key.options, "command=\"echo   hello world\",no-pty");
        assert!(
            key.serialized_with_comment("renamed")
                .starts_with("command=\"echo   hello world\",no-pty ")
        );
    }

    #[test]
    fn public_key_line_omits_authorized_keys_options() {
        let restricted = parse_authorized_key(
            &format!(
                "from=\"192.0.2.1\",no-pty {}",
                key_line("ssh-ed25519", "laptop")
            ),
            0,
        )
        .unwrap();
        assert_eq!(
            restricted.public_key_line(),
            format!("ssh-ed25519 {} laptop", restricted.key_data)
        );

        let unnamed = parse_authorized_key(
            &format!("ssh-rsa {}", {
                let mut blob = Vec::new();
                blob.extend_from_slice(&("ssh-rsa".len() as u32).to_be_bytes());
                blob.extend_from_slice(b"ssh-rsa");
                blob.extend_from_slice(b"payload");
                base64::engine::general_purpose::STANDARD.encode(blob)
            }),
            1,
        )
        .unwrap();
        assert_eq!(
            unnamed.public_key_line(),
            format!("ssh-rsa {}", unnamed.key_data)
        );
    }

    #[test]
    fn duplicate_check_includes_authorization_options() {
        let plain = parse_authorized_key(&key_line("ssh-ed25519", "plain"), 0).unwrap();
        let restricted = parse_authorized_key(
            &format!(
                "from=\"192.0.2.0/24\" {}",
                key_line("ssh-ed25519", "restricted")
            ),
            1,
        )
        .unwrap();
        let renamed = parse_authorized_key(&key_line("ssh-ed25519", "renamed"), 2).unwrap();

        assert!(!same_authorization(&plain, &restricted));
        assert!(same_authorization(&plain, &renamed));
    }

    #[test]
    fn selected_line_must_match_exactly_before_editing() {
        let original = format!(
            "from=\"192.0.2.1\",no-pty {}",
            key_line("ssh-ed25519", "original")
        );
        let key = parse_authorized_key(&original, 0).unwrap();

        let changed_options = original.replace("no-pty", "restrict");
        let changed_comment = original.replace("original", "changed elsewhere");
        let changed_spacing = format!("  {original}");

        for changed in [changed_options, changed_comment, changed_spacing] {
            let mut lines = vec![changed];
            assert!(replace_line(&mut lines, &key, &key.serialized_with_comment("mine")).is_err());
        }
    }

    #[test]
    fn ignores_comments_and_invalid_key_data() {
        assert!(parse_authorized_key("# ssh-ed25519 disabled", 0).is_none());
        assert!(parse_authorized_key("ssh-ed25519 not-base64 label", 0).is_none());
    }

    #[test]
    fn edits_and_removes_only_selected_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("authorized_keys");
        let first = key_line("ssh-ed25519", "first");
        let second = key_line("ssh-rsa", "second");
        fs::write(&path, format!("# keep this\n{first}\n{second}\n")).unwrap();

        let keys = read_keys_from(&path);
        let mut lines = read_lines_from(&path);
        replace_line(
            &mut lines,
            &keys[0],
            &keys[0].serialized_with_comment("renamed"),
        )
        .unwrap();
        let expected = fs::read_to_string(&path).unwrap();
        replace_contents_atomically(&path, Some(&expected), &serialize_lines(&lines)).unwrap();

        let keys = read_keys_from(&path);
        assert_eq!(keys[0].comment, "renamed");
        assert_eq!(keys[1].comment, "second");

        let mut lines = read_lines_from(&path);
        remove_line(&mut lines, &keys[1]).unwrap();
        let expected = fs::read_to_string(&path).unwrap();
        replace_contents_atomically(&path, Some(&expected), &serialize_lines(&lines)).unwrap();

        let contents = fs::read_to_string(path).unwrap();
        assert!(contents.starts_with("# keep this\n"));
        assert!(contents.contains("renamed"));
        assert!(!contents.contains("second"));
    }

    #[test]
    fn foreign_store_targets_the_users_home() {
        let foreign_uid = current_uid().wrapping_add(1);
        let info = UserInfo {
            username: "alice".to_string(),
            shell: "/bin/bash".to_string(),
            primary_group: Some("alice".to_string()),
            groups: vec!["alice".to_string()],
            home: PathBuf::from("/home/alice"),
            uid: foreign_uid,
        };

        let keys = AuthorizedKeysFile::for_user(&info).unwrap();
        assert_eq!(keys.username, "alice");
        assert_eq!(keys.path, PathBuf::from("/home/alice/.ssh/authorized_keys"));
        assert_eq!(keys.owner_uid, foreign_uid);
        assert!(!keys.is_current());

        let mut homeless = info.clone();
        homeless.home = PathBuf::new();
        assert!(AuthorizedKeysFile::for_user(&homeless).is_err());
    }

    #[test]
    fn only_the_owning_user_gets_direct_access() {
        let mut info = UserInfo {
            username: "me".to_string(),
            shell: "/bin/bash".to_string(),
            primary_group: Some("me".to_string()),
            groups: vec!["me".to_string()],
            home: PathBuf::from("/home/me"),
            uid: current_uid(),
        };
        assert!(AuthorizedKeysFile::for_user(&info).unwrap().is_current());

        info.uid += 1;
        assert!(!AuthorizedKeysFile::for_user(&info).unwrap().is_current());
    }

    #[test]
    fn atomically_updates_symlink_target_without_replacing_symlink() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let target_dir = tempfile::tempdir().unwrap();
        let target = target_dir.path().join("authorized_keys");
        let link = dir.path().join("authorized_keys");
        fs::write(&target, "old contents\n").unwrap();
        symlink(&target, &link).unwrap();

        replace_contents_atomically(&link, Some("old contents\n"), "new contents\n").unwrap();

        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "new contents\n");
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn stale_atomic_update_preserves_current_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("authorized_keys");
        fs::write(&path, "newer contents\n").unwrap();

        let result =
            replace_contents_atomically(&path, Some("stale contents\n"), "replacement contents\n");

        assert!(result.is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "newer contents\n");
    }

    #[test]
    fn atomic_update_creates_secure_ssh_storage() {
        let home = tempfile::tempdir().unwrap();
        let ssh_dir = home.path().join(".ssh");
        let path = ssh_dir.join("authorized_keys");

        replace_contents_atomically(&path, None, "key contents\n").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "key contents\n");
        assert_eq!(
            fs::metadata(&ssh_dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn helper_protocol_round_trips_and_rejects_other_versions() {
        let update = AuthorizedKeysUpdate {
            version: AUTHORIZED_KEYS_PROTOCOL_VERSION,
            expected: Some("old\n".to_string()),
            replacement: "new\n".to_string(),
        };
        let encoded = serde_json::to_vec(&update).unwrap();
        let decoded: AuthorizedKeysUpdate = serde_json::from_slice(&encoded).unwrap();

        assert_eq!(decoded.version, AUTHORIZED_KEYS_PROTOCOL_VERSION);
        assert_eq!(decoded.expected.as_deref(), Some("old\n"));
        assert_eq!(decoded.replacement, "new\n");

        let unknown_field = br#"{"version":1,"expected":null,"replacement":"","extra":true}"#;
        assert!(serde_json::from_slice::<AuthorizedKeysUpdate>(unknown_field).is_err());
    }

    #[test]
    fn helper_path_is_restricted_to_the_account_home() {
        let uid = nix::unistd::Uid::effective();
        let user = nix::unistd::User::from_uid(uid).unwrap().unwrap();

        assert_eq!(
            authorized_keys_path_for_user(&user).unwrap(),
            user.dir.join(".ssh/authorized_keys")
        );
    }
}
