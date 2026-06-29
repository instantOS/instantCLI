use std::fs;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use base64::Engine;
use sha2::{Digest, Sha256};

use crate::menu_utils::{FzfPreview, FzfResult, FzfSelectable, FzfWrapper, select_one_with_style};
use crate::settings::context::SettingsContext;
use crate::ui::catppuccin::{colors, format_icon, format_icon_colored};
use crate::ui::prelude::*;
use crate::ui::preview::PreviewBuilder;

#[derive(Clone, Debug, PartialEq, Eq)]
struct AuthorizedKey {
    line_index: usize,
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
    EditComment,
    Remove,
    Back,
}

impl FzfSelectable for KeyActionItem {
    fn fzf_display_text(&self) -> String {
        match self {
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

pub fn manage_ssh_keys(ctx: &mut SettingsContext) -> Result<()> {
    let path = authorized_keys_path()?;

    loop {
        let keys = read_authorized_keys(&path)?;
        let mut items: Vec<_> = keys.into_iter().map(KeyMenuItem::Key).collect();
        items.push(KeyMenuItem::Add);
        items.push(KeyMenuItem::Back);

        match select_one_with_style(items)? {
            Some(KeyMenuItem::Key(key)) => manage_key(ctx, &path, &key)?,
            Some(KeyMenuItem::Add) => add_key(ctx, &path)?,
            _ => break,
        }
    }

    Ok(())
}

fn authorized_keys_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("determining home directory for SSH keys")?;
    Ok(home.join(".ssh/authorized_keys"))
}

fn read_authorized_keys(path: &Path) -> Result<Vec<AuthorizedKey>> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err).with_context(|| format!("reading {}", path.display())),
    };

    Ok(contents
        .lines()
        .enumerate()
        .filter_map(|(line_index, line)| parse_authorized_key(line, line_index))
        .collect())
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

fn manage_key(ctx: &mut SettingsContext, path: &Path, key: &AuthorizedKey) -> Result<()> {
    loop {
        match select_one_with_style(vec![
            KeyActionItem::EditComment,
            KeyActionItem::Remove,
            KeyActionItem::Back,
        ])? {
            Some(KeyActionItem::EditComment) => {
                let comment = FzfWrapper::builder()
                    .prompt("SSH key comment")
                    .query(&key.comment)
                    .input()
                    .input_result()?;
                let FzfResult::Selected(comment) = comment else {
                    continue;
                };
                replace_key_line(path, key, &key.serialized_with_comment(&comment))?;
                ctx.emit_success("settings.users.ssh_keys", "SSH key comment updated.");
                break;
            }
            Some(KeyActionItem::Remove) => {
                let result = FzfWrapper::builder()
                    .confirm(format!("Remove SSH key {:?}?", key.label()))
                    .yes_text("Remove key")
                    .no_text("Cancel")
                    .confirm_dialog()?;
                if matches!(result, crate::menu_utils::ConfirmResult::Yes) {
                    remove_key_line(path, key)?;
                    ctx.emit_success("settings.users.ssh_keys", "SSH key removed.");
                    break;
                }
            }
            _ => break,
        }
    }
    Ok(())
}

fn add_key(ctx: &mut SettingsContext, path: &Path) -> Result<()> {
    let input = FzfWrapper::builder()
        .prompt("Paste SSH public key")
        .input()
        .input_dialog()?;
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
    if read_authorized_keys(path)?
        .iter()
        .any(|key| same_authorization(key, &new_key))
    {
        ctx.emit_info(
            "settings.users.ssh_keys",
            "This SSH key is already authorized.",
        );
        return Ok(());
    }

    let mut lines = read_lines(path)?;
    lines.push(trimmed.to_string());
    write_lines(path, &lines)?;
    ctx.emit_success("settings.users.ssh_keys", "SSH key added.");
    Ok(())
}

fn same_authorization(left: &AuthorizedKey, right: &AuthorizedKey) -> bool {
    left.key_data == right.key_data && left.options == right.options
}

fn replace_key_line(path: &Path, key: &AuthorizedKey, replacement: &str) -> Result<()> {
    let mut lines = read_lines(path)?;
    let line = lines
        .get_mut(key.line_index)
        .context("SSH key changed while the menu was open")?;
    ensure_same_key(line, key)?;
    *line = replacement.to_string();
    write_lines(path, &lines)
}

fn remove_key_line(path: &Path, key: &AuthorizedKey) -> Result<()> {
    let mut lines = read_lines(path)?;
    if key.line_index >= lines.len() {
        bail!("SSH key changed while the menu was open");
    }
    ensure_same_key(&lines[key.line_index], key)?;
    lines.remove(key.line_index);
    write_lines(path, &lines)
}

fn ensure_same_key(line: &str, expected: &AuthorizedKey) -> Result<()> {
    let current = parse_authorized_key(line, expected.line_index)
        .context("SSH key changed while the menu was open")?;
    if current.key_data != expected.key_data {
        bail!("SSH key changed while the menu was open");
    }
    Ok(())
}

fn read_lines(path: &Path) -> Result<Vec<String>> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(contents.lines().map(str::to_string).collect()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(err) => Err(err).with_context(|| format!("reading {}", path.display())),
    }
}

fn write_lines(path: &Path, lines: &[String]) -> Result<()> {
    let ssh_dir = path
        .parent()
        .context("authorized_keys has no parent directory")?;
    if !ssh_dir.exists() {
        fs::DirBuilder::new()
            .recursive(true)
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
    for line in lines {
        writeln!(temporary, "{line}")?;
    }
    temporary.flush()?;
    temporary.as_file().sync_all()?;

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

        let keys = read_authorized_keys(&path).unwrap();
        replace_key_line(&path, &keys[0], &keys[0].serialized_with_comment("renamed")).unwrap();
        let keys = read_authorized_keys(&path).unwrap();
        assert_eq!(keys[0].comment, "renamed");
        assert_eq!(keys[1].comment, "second");

        remove_key_line(&path, &keys[1]).unwrap();
        let contents = fs::read_to_string(path).unwrap();
        assert!(contents.starts_with("# keep this\n"));
        assert!(contents.contains("renamed"));
        assert!(!contents.contains("second"));
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

        write_lines(&link, &["new contents".to_string()]).unwrap();

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
}
