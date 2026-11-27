//! First-run setup for the `pass` password store.
//!
//! A new password store can be initialized in a terminal with a new or
//! existing GPG key. Existing encrypted entries are left to `pass` itself.

use std::env;
use std::ffi::OsStr;
use std::fs::{self, DirBuilder, File};
use std::io::{IsTerminal, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use walkdir::WalkDir;

use crate::common::commands::command_exists;
use crate::menu_utils::{DialogOutcome, FzfSelectable, FzfWrapper, HeaderBuilder};
use crate::ui::catppuccin::{colors, format_icon_colored, format_with_color};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::utils::{ensure_core_dependencies, password_store_dir, run_pass_status};

#[derive(Debug, PartialEq, Eq)]
enum StoreState {
    New,
    Configured(String),
    Existing,
}

/// Offer setup for a new store before continuing with the original command.
pub(super) fn onboard_if_new(debug: bool) -> Result<bool> {
    let store_dir = password_store_dir()?;
    if store_state(&store_dir)? != StoreState::New {
        return Ok(true);
    }

    if interactive_terminal() {
        return run_setup();
    }
    launch_in_terminal(debug)
}

/// Run setup in the current terminal. Used by `ins pass setup` and by the
/// terminal process that non-terminal callers launch for first-time setup.
///
/// Returns `false` when the user cancels before configuring the store.
pub(super) fn run_setup() -> Result<bool> {
    let result = run_setup_inner();
    if let Err(error) = &result
        && interactive_terminal()
    {
        let _ = message("INS Pass Setup Failed", &format!("{error:#}"));
    }
    result
}

fn run_setup_inner() -> Result<bool> {
    if !interactive_terminal() {
        bail!("`ins pass setup` needs an interactive terminal");
    }

    let store_dir = password_store_dir()?;
    match store_state(&store_dir)? {
        StoreState::Configured(recipient) => {
            message(
                "INS Pass Is Already Set Up",
                &format!("Store: {}\nRecipient: {recipient}", store_dir.display()),
            )?;
            Ok(true)
        }
        StoreState::Existing => Err(existing_store_error(&store_dir)),
        StoreState::New => {
            ensure_core_dependencies()?;
            run_onboarding(&store_dir)
        }
    }
}

fn launch_in_terminal(debug: bool) -> Result<bool> {
    let current_exe = env::current_exe().context("Failed to locate the ins executable")?;
    let mut args = Vec::new();
    if debug {
        args.push("--debug".to_string());
    }
    args.extend(["pass".to_string(), "setup".to_string()]);

    let status = instantos_terminal::Launcher::new(current_exe.as_os_str())
        .app_id("ins-pass")
        .title("INS Pass Setup")
        .args(&args)
        .status()
        .context("Failed to open the setup terminal")?;
    match status.code() {
        Some(0) => Ok(matches!(
            store_state(&password_store_dir()?)?,
            StoreState::Configured(_)
        )),
        Some(1) => Ok(false),
        _ => bail!("The setup terminal exited with {status}"),
    }
}

pub(super) fn interactive_terminal() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

fn store_state(store_dir: &Path) -> Result<StoreState> {
    let key_override = env::var_os("PASSWORD_STORE_KEY");
    inspect_store(
        store_dir,
        key_override
            .as_deref()
            .map(OsStr::to_string_lossy)
            .as_deref(),
    )
}

fn inspect_store(store_dir: &Path, key_override: Option<&str>) -> Result<StoreState> {
    if let Some(key) = key_override.filter(|key| !key.trim().is_empty()) {
        return Ok(StoreState::Configured(key.to_owned()));
    }

    match fs::symlink_metadata(store_dir) {
        Ok(_) if store_dir.is_dir() => {}
        Ok(_) => bail!("Password store is not a directory: {}", store_dir.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(StoreState::New),
        Err(error) => return Err(error).context("Could not inspect the password store"),
    }

    let gpg_id = store_dir.join(".gpg-id");
    if gpg_id.is_file() {
        let contents =
            fs::read(&gpg_id).with_context(|| format!("Could not read {}", gpg_id.display()))?;
        let contents = String::from_utf8_lossy(&contents);
        if contents
            .lines()
            .any(|line| !line.split('#').next().unwrap_or_default().trim().is_empty())
        {
            return Ok(StoreState::Configured(contents.trim().to_owned()));
        }
    }

    for entry in WalkDir::new(store_dir)
        .into_iter()
        .filter_entry(|entry| entry.depth() == 0 || entry.file_name() != ".git")
    {
        let entry = entry.context("Could not inspect existing password-store entries")?;
        let name = entry.file_name().as_bytes();
        let encrypted = name[name.len().saturating_sub(4)..].eq_ignore_ascii_case(b".gpg");
        if entry.depth() > 0 && (name == b".gpg-id" || encrypted) {
            return Ok(StoreState::Existing);
        }
    }
    Ok(StoreState::New)
}

fn existing_store_error(store_dir: &Path) -> anyhow::Error {
    anyhow::anyhow!(
        "Existing password store at {} has no usable root recipient configuration. \
         Use pass to manage its recipients, or choose a new store with PASSWORD_STORE_DIR.",
        store_dir.display()
    )
}

fn run_onboarding(store_dir: &Path) -> Result<bool> {
    message(
        "Welcome to INS Pass",
        &format!(
            "INS Pass uses the standard pass password store.\n\n\
             Store: {}\n\n\
             New entries must be encrypted with a GPG key. You can create a \
             new key or select an existing secret key from your GnuPG keyring.",
            store_dir.display()
        ),
    )?;

    loop {
        let options = vec![
            ChoiceOption::new(
                "create",
                "Create a new GPG key",
                "Generate a fresh encryption key for this machine. Recommended \
                 if you do not already use GnuPG.",
                NerdFont::Plus,
                colors::GREEN,
            ),
            ChoiceOption::new(
                "existing",
                "Use an existing GPG key",
                "Choose an encryption-capable secret key from your GnuPG keyring.",
                NerdFont::Key,
                colors::BLUE,
            ),
            ChoiceOption::new(
                "cancel",
                "Cancel setup",
                "Leave the password store unconfigured.",
                NerdFont::Cross,
                colors::OVERLAY1,
            ),
        ];

        let Some(choice) = choose(
            "Set Up INS Pass",
            "How should your passwords be encrypted?",
            &options,
        )?
        else {
            return Ok(false);
        };

        match choice.key.as_str() {
            "create" => {
                let Some(key) = create_key()? else {
                    continue;
                };
                initialize_store(store_dir, &key)?;
                return Ok(true);
            }
            "existing" => {
                let Some(key) = select_existing_key()? else {
                    continue;
                };
                initialize_store(store_dir, &key)?;
                return Ok(true);
            }
            "cancel" => return Ok(false),
            other => bail!("Unknown onboarding choice: {other}"),
        }
    }
}

#[derive(Clone)]
struct ChoiceOption {
    key: String,
    label: String,
    detail: String,
    icon: NerdFont,
    color: &'static str,
}

impl ChoiceOption {
    fn new(
        key: impl Into<String>,
        label: impl Into<String>,
        detail: impl Into<String>,
        icon: NerdFont,
        color: &'static str,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            detail: detail.into(),
            icon,
            color,
        }
    }
}

impl FzfSelectable for ChoiceOption {
    fn fzf_display_text(&self) -> String {
        let mut text = format!(
            "{} {}",
            format_icon_colored(self.icon, self.color),
            plain(&self.label)
        );
        if !self.detail.is_empty() {
            text.push_str(&format_with_color(
                &format!("  {}", plain(&self.detail)),
                colors::OVERLAY1,
            ));
        }
        text
    }

    fn fzf_preview(&self) -> crate::menu_utils::FzfPreview {
        PreviewBuilder::new()
            .header(self.icon, &self.label)
            .text(&self.detail)
            .build()
    }

    fn fzf_key(&self) -> String {
        self.key.clone()
    }
}

fn choose(title: &str, prompt: &str, options: &[ChoiceOption]) -> Result<Option<ChoiceOption>> {
    let header = HeaderBuilder::new(NerdFont::ClosedLockWithKey, title).build();
    let outcome = FzfWrapper::builder()
        .header(header)
        .prompt(prompt)
        .items(options.to_vec())
        .padded()
        .select_one()?;
    match outcome {
        DialogOutcome::Submitted(option) => Ok(Some(option)),
        DialogOutcome::Cancelled => Ok(None),
    }
}

fn message(title: &str, body: &str) -> Result<()> {
    FzfWrapper::builder()
        .message(body)
        .title(title)
        .message_dialog()
}

fn input(prompt: &str, placeholder: &str, initial: Option<&str>) -> Result<Option<String>> {
    let options = match initial.filter(|value| !value.is_empty()) {
        Some(value) => crate::menu::protocol::InputOptions::text_with_initial(prompt, value),
        None => crate::menu::protocol::InputOptions::text(prompt),
    }
    .with_placeholder(placeholder);
    match FzfWrapper::input_with_options(&options)? {
        DialogOutcome::Submitted(value) => Ok(Some(value)),
        DialogOutcome::Cancelled => Ok(None),
    }
}

fn password(prompt: &str, placeholder: &str) -> Result<Option<String>> {
    let options =
        crate::menu::protocol::InputOptions::password(prompt).with_placeholder(placeholder);
    match FzfWrapper::input_with_options(&options)? {
        DialogOutcome::Submitted(value) => Ok(Some(value)),
        DialogOutcome::Cancelled => Ok(None),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SecretKey {
    fingerprint: String,
    user_id: String,
}

impl SecretKey {
    fn label(&self) -> String {
        format!(
            "{} ({})",
            plain(&self.user_id),
            short_fingerprint(&self.fingerprint)
        )
    }
}

fn create_key() -> Result<Option<SecretKey>> {
    let Some(name) = prompt_name()? else {
        return Ok(None);
    };
    let Some(email) = prompt_email()? else {
        return Ok(None);
    };
    let Some(passphrase) = prompt_passphrase()? else {
        return Ok(None);
    };

    let identity = format!("{name} <{email}>");
    let fingerprint = generate_gpg_key(&identity, &passphrase)?;
    Ok(Some(SecretKey {
        fingerprint,
        user_id: identity,
    }))
}

fn prompt_name() -> Result<Option<String>> {
    let default = git_config("user.name");
    loop {
        let Some(value) = input(
            "Full name",
            "Name shown on your GPG key",
            default.as_deref(),
        )?
        else {
            return Ok(None);
        };
        let value = value.trim().to_string();
        if !value.is_empty() {
            return Ok(Some(value));
        }
        message("Name Required", "Enter a name for the new GPG key.")?;
    }
}

fn prompt_email() -> Result<Option<String>> {
    let default = git_config("user.email").or_else(|| env::var("EMAIL").ok());
    loop {
        let Some(value) = input(
            "Email address",
            "Address shown on your GPG key",
            default.as_deref(),
        )?
        else {
            return Ok(None);
        };
        let value = value.trim().to_string();
        if valid_email(&value) {
            return Ok(Some(value));
        }
        message(
            "Email Required",
            "Enter a valid email address, for example demo@example.invalid.",
        )?;
    }
}

fn valid_email(value: &str) -> bool {
    !value.is_empty()
        && !value.chars().any(char::is_whitespace)
        && value
            .split_once('@')
            .is_some_and(|(local, domain)| !local.is_empty() && domain.contains('.'))
}

fn prompt_passphrase() -> Result<Option<String>> {
    loop {
        let Some(passphrase) = password(
            "GPG key passphrase",
            "Leave empty to create a key without a passphrase",
        )?
        else {
            return Ok(None);
        };

        if passphrase.is_empty() {
            let options = vec![
                ChoiceOption::new(
                    "none",
                    "Create the key without a passphrase",
                    "Anyone who obtains your private key can decrypt your passwords \
                     without a passphrase.",
                    NerdFont::Warning,
                    colors::YELLOW,
                ),
                ChoiceOption::new(
                    "set",
                    "Set a passphrase",
                    "Return to the passphrase prompt.",
                    NerdFont::Key,
                    colors::BLUE,
                ),
                ChoiceOption::new(
                    "cancel",
                    "Cancel key creation",
                    "Keep the existing GPG keyring unchanged.",
                    NerdFont::Cross,
                    colors::OVERLAY1,
                ),
            ];
            let Some(choice) = choose(
                "No Passphrase",
                "Create a key without a passphrase?",
                &options,
            )?
            else {
                return Ok(None);
            };
            match choice.key.as_str() {
                "none" => return Ok(Some(String::new())),
                "set" => continue,
                _ => return Ok(None),
            }
        }

        let Some(confirmation) = password("Confirm passphrase", "")? else {
            return Ok(None);
        };
        if confirmation == passphrase {
            return Ok(Some(passphrase));
        }
        message(
            "Passphrase Mismatch",
            "The passphrases did not match. Please try again.",
        )?;
    }
}

fn select_existing_key() -> Result<Option<SecretKey>> {
    let keys = match list_secret_keys() {
        Ok(keys) => keys,
        Err(error) => {
            message("Could Not Read GPG Keys", &format!("{error:#}"))?;
            return Ok(None);
        }
    };
    if keys.is_empty() {
        message(
            "No Usable GPG Keys",
            "No encryption-capable secret keys were found in your GnuPG keyring.\n\n\
             Create a new key instead, or import or create a key with GnuPG first.",
        )?;
        return Ok(None);
    }

    let options = keys
        .iter()
        .map(|key| {
            ChoiceOption::new(
                key.fingerprint.clone(),
                key.label(),
                format!("Fingerprint: {}", key.fingerprint),
                NerdFont::Key,
                colors::BLUE,
            )
        })
        .collect::<Vec<_>>();

    let Some(choice) = choose(
        "Use an Existing GPG Key",
        "Choose the key that should encrypt your passwords",
        &options,
    )?
    else {
        return Ok(None);
    };
    keys.into_iter()
        .find(|key| key.fingerprint == choice.key)
        .map(Some)
        .context("Selected GPG key disappeared")
}

fn initialize_store(store_dir: &Path, key: &SecretKey) -> Result<()> {
    {
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(store_dir)
            .with_context(|| format!("Failed to create {}", store_dir.display()))?;
        // Serialize the final check and initialization across INS setup processes.
        let store = File::open(store_dir).context("Failed to open the password store")?;
        store
            .lock()
            .context("Failed to lock the password store for setup")?;
        match store_state(store_dir)? {
            StoreState::Configured(_) => bail!("The password store was configured during setup"),
            StoreState::Existing => return Err(existing_store_error(store_dir)),
            StoreState::New => {}
        }
        run_pass_status(["init", &key.fingerprint])?;
        if store_state(store_dir)? != StoreState::Configured(key.fingerprint.clone()) {
            bail!("`pass init` did not configure the selected recipient");
        }
    }

    message(
        "INS Pass Is Ready",
        &format!(
            "Your password store is initialized.\n\n\
             Store: {}\n\
             Recipient: {}\n\
             Fingerprint: {}\n\n\
             Add your first entry with `ins pass add`, or continue to the picker.",
            store_dir.display(),
            key.user_id,
            key.fingerprint
        ),
    )
}

fn gpg_command() -> Result<Command> {
    let mut gpg = Command::new(if command_exists("gpg2") {
        "gpg2"
    } else {
        "gpg"
    });
    if env::var_os("PASSWORD_STORE_GPG_OPTS").is_some_and(|opts| !opts.is_empty()) {
        // pass expands this value with Bash word splitting and pathname expansion.
        // Read the resulting arguments without interpreting the value as shell code.
        let output = Command::new("bash")
            .args([
                "-c",
                r#"for option in $PASSWORD_STORE_GPG_OPTS; do printf '%s\0' "$option"; done"#,
            ])
            .output()
            .context("Failed to read PASSWORD_STORE_GPG_OPTS")?;
        if !output.status.success() {
            bail!(
                "Could not read PASSWORD_STORE_GPG_OPTS: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        gpg.args(
            output
                .stdout
                .strip_suffix(&[0])
                .unwrap_or(&[])
                .split(|byte| *byte == 0)
                .filter(|option| !option.is_empty())
                .map(OsStr::from_bytes),
        );
    }
    Ok(gpg)
}

fn generate_gpg_key(identity: &str, passphrase: &str) -> Result<String> {
    let mut child = gpg_command()?
        .args([
            "--batch",
            "--pinentry-mode",
            "loopback",
            "--passphrase-fd",
            "0",
            "--status-fd",
            "1",
            "--quick-generate-key",
            identity,
            "default",
            "default",
            "never",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to start gpg")?;

    if let Some(mut stdin) = child.stdin.take() {
        writeln!(stdin, "{passphrase}").context("Failed to pass the passphrase to gpg")?;
    }

    let output = child.wait_with_output().context("Failed to wait for gpg")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        if stderr.is_empty() {
            bail!("gpg could not create the key ({})", output.status);
        }
        bail!("gpg could not create the key: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let fingerprint = parse_created_fingerprint(&stdout)
        .context("gpg created a key but did not report its fingerprint")?;
    if !list_secret_keys()?
        .iter()
        .any(|key| key.fingerprint == fingerprint)
    {
        bail!("The new GPG key {fingerprint} has no usable encryption secret key");
    }
    Ok(fingerprint)
}

fn parse_created_fingerprint(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        line.strip_prefix("[GNUPG:] KEY_CREATED ")
            .and_then(|rest| rest.split_whitespace().last())
            .map(ToString::to_string)
    })
}

fn list_secret_keys() -> Result<Vec<SecretKey>> {
    let output = gpg_command()?
        .args(["--batch", "--with-colons", "--list-secret-keys"])
        .output()
        .context("Failed to run `gpg --list-secret-keys`")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("gpg could not list secret keys: {}", stderr.trim());
    }

    let output = String::from_utf8_lossy(&output.stdout);
    let mut keys = parse_secret_keys(&output);
    keys.sort_by(|left, right| {
        left.user_id
            .to_lowercase()
            .cmp(&right.user_id.to_lowercase())
            .then_with(|| left.fingerprint.cmp(&right.fingerprint))
    });
    Ok(keys)
}

#[derive(Debug)]
struct PendingKey {
    fingerprint: Option<String>,
    user_id: Option<String>,
    has_encryption: bool,
    missing_encryption_secret: bool,
    unusable: bool,
}

impl PendingKey {
    fn record_encryption(&mut self, fields: &[&str]) {
        let validity = fields.get(1).copied().unwrap_or("");
        let capabilities = fields.get(11).copied().unwrap_or("");
        if !unusable_validity(validity) && capabilities.contains('e') && !capabilities.contains('D')
        {
            let secret = fields.get(14).copied().unwrap_or("");
            self.has_encryption = true;
            // GnuPG marks missing secret material with #. A card serial number
            // and + indicate that GnuPG knows where the secret key lives.
            self.missing_encryption_secret |= secret.is_empty() || secret == "#";
        }
    }
}

fn parse_secret_keys(output: &str) -> Vec<SecretKey> {
    let mut keys = Vec::new();
    let mut current: Option<PendingKey> = None;

    for line in output.lines() {
        let fields = line.split(':').collect::<Vec<_>>();
        match fields.first().copied() {
            Some("sec") => {
                if let Some(key) = current.take().and_then(finish_pending_key) {
                    keys.push(key);
                }
                let validity = fields.get(1).copied().unwrap_or("");
                let capabilities = fields.get(11).copied().unwrap_or("");
                let mut pending = PendingKey {
                    fingerprint: None,
                    user_id: None,
                    has_encryption: false,
                    missing_encryption_secret: false,
                    unusable: unusable_validity(validity) || capabilities.contains('D'),
                };
                pending.record_encryption(&fields);
                current = Some(pending);
            }
            Some("fpr") => {
                if let Some(pending) = current.as_mut()
                    && pending.fingerprint.is_none()
                {
                    pending.fingerprint = fields
                        .get(9)
                        .filter(|fingerprint| !fingerprint.is_empty())
                        .map(|fingerprint| (*fingerprint).to_string());
                }
            }
            Some("uid") => {
                if let Some(pending) = current.as_mut()
                    && pending.user_id.is_none()
                {
                    pending.user_id = fields.get(9).map(|uid| plain(&unescape_colon_field(uid)));
                }
            }
            Some("ssb") => {
                if let Some(pending) = current.as_mut() {
                    pending.record_encryption(&fields);
                }
            }
            _ => {}
        }
    }

    if let Some(key) = current.and_then(finish_pending_key) {
        keys.push(key);
    }
    keys
}

fn finish_pending_key(pending: PendingKey) -> Option<SecretKey> {
    // pass uses the primary fingerprint and lets GnuPG pick the encryption
    // subkey. Any eligible stub could be selected over a usable secret subkey.
    if pending.unusable || !pending.has_encryption || pending.missing_encryption_secret {
        return None;
    }
    let fingerprint = pending.fingerprint?;
    let user_id = pending
        .user_id
        .filter(|uid| !uid.is_empty())
        .unwrap_or_else(|| "GPG key".to_string());
    Some(SecretKey {
        fingerprint,
        user_id,
    })
}

fn unusable_validity(validity: &str) -> bool {
    matches!(validity, "r" | "e" | "d" | "i")
}

fn unescape_colon_field(value: &str) -> String {
    let mut result = String::new();
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            result.push(character);
            continue;
        }
        match chars.next() {
            Some('x') => {
                let hex = chars.by_ref().take(2).collect::<String>();
                if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                    result.push(char::from(byte));
                } else {
                    result.push_str("\\x");
                    result.push_str(&hex);
                }
            }
            Some('n') => result.push('\n'),
            Some('r') => result.push('\r'),
            Some('e') => result.push('\u{1b}'),
            Some(other) => result.push(other),
            None => result.push('\\'),
        }
    }
    result
}

fn short_fingerprint(fingerprint: &str) -> &str {
    let start = fingerprint.len().saturating_sub(16);
    &fingerprint[start..]
}

fn git_config(key: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["config", "--global", "--get", key])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn plain(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GPG_LIST_OUTPUT: &str = "\
sec:u:255:22:E670E0EF8F45A3CB:1795813985:::u:::scESC:::+::ed25519:::0:
fpr:::::::::6D0C46A4C1DD9511381C4418E670E0EF8F45A3CB:
grp:::::::::88AFE5CE7CA4ED9F5ADC3B8A8D91A025863A0D1F:
uid:u::::1795813985::153B56781844F5A77BA4651B89D5DCE453F148DE::Demo Test <demo-test@example.invalid>::::::::::0:
ssb:u:255:18:B0F7D9210CCE0DAB:1795813985::::::e:::+::cv25519::
fpr:::::::::9C40E85FF6A8D54A5245F062B0F7D9210CCE0DAB:
sec:r:2048:1:1111111111111111:1700000000:::u:::scESC:::+::rsa2048:::0:
fpr:::::::::AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA:
uid:u::::1700000000::AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA::Revoked <revoked@example.invalid>::::::::::0:
ssb:e:2048:1:2222222222222222:1700000000:::u:::e:::+::rsa2048::
fpr:::::::::BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB:
sec:u:255:22:3333333333333333:1795813985:::u:::scSC:::+::ed25519:::0:
fpr:::::::::CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC:
uid:u::::1795813985::CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC::Signing Only <sign@example.invalid>::::::::::0:
";

    #[test]
    fn parses_only_encryption_capable_unrevoked_secret_keys() {
        let keys = parse_secret_keys(GPG_LIST_OUTPUT);
        assert_eq!(keys.len(), 1);
        assert_eq!(
            keys[0].fingerprint,
            "6D0C46A4C1DD9511381C4418E670E0EF8F45A3CB"
        );
        assert_eq!(keys[0].user_id, "Demo Test <demo-test@example.invalid>");
    }

    #[test]
    fn short_fingerprint_keeps_the_last_sixteen_characters() {
        assert_eq!(
            short_fingerprint("6D0C46A4C1DD9511381C4418E670E0EF8F45A3CB"),
            "E670E0EF8F45A3CB"
        );
    }

    #[test]
    fn colon_fields_are_unescaped() {
        assert_eq!(
            unescape_colon_field("Demo\\x3a Test \\e"),
            "Demo: Test \u{1b}"
        );
    }

    #[test]
    fn created_fingerprint_is_read_from_status_output() {
        assert_eq!(
            parse_created_fingerprint(
                "[GNUPG:] KEY_CREATED B B33788FD2BD9B0DD2E9B5736D6E6013C1914D0FD\n"
            )
            .as_deref(),
            Some("B33788FD2BD9B0DD2E9B5736D6E6013C1914D0FD")
        );
        assert!(parse_created_fingerprint("gpg: key created").is_none());
    }

    #[test]
    fn email_validation_rejects_obvious_mistakes() {
        assert!(valid_email("demo@example.invalid"));
        assert!(!valid_email(""));
        assert!(!valid_email("demo"));
        assert!(!valid_email("demo@localhost"));
        assert!(!valid_email("demo @example.invalid"));
    }

    #[test]
    fn encryption_subkeys_need_secret_material() {
        let stub = GPG_LIST_OUTPUT.replace("::::::e:::+::cv25519", "::::::e:::#::cv25519");
        assert!(parse_secret_keys(&stub).is_empty());

        let card =
            GPG_LIST_OUTPUT.replace("::::::e:::+::cv25519", "::::::e:::D27600012401::cv25519");
        assert_eq!(parse_secret_keys(&card).len(), 1);
    }

    #[test]
    fn available_subkey_does_not_make_an_eligible_stub_safe() {
        let mixed = GPG_LIST_OUTPUT.replace(
            "sec:r:",
            "ssb:u:255:18:1111111111111111:1795814000::::::e:::#::cv25519::\nsec:r:",
        );
        assert!(parse_secret_keys(&mixed).is_empty());
    }

    #[test]
    fn only_new_stores_are_eligible_for_onboarding() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = temp.path().join("store");
        assert_eq!(inspect_store(&store, None)?, StoreState::New);
        fs::create_dir_all(store.join(".git"))?;
        fs::write(store.join(".git/fixture.gpg"), [])?;
        assert_eq!(inspect_store(&store, None)?, StoreState::New);

        let subfolder = store.join("shared");
        fs::create_dir(&subfolder)?;
        let entry = subfolder.join(".GPG");
        fs::write(&entry, [])?;
        assert_eq!(inspect_store(&store, None)?, StoreState::Existing);
        fs::remove_file(entry)?;
        fs::write(subfolder.join(".gpg-id"), "shared@example.invalid\n")?;
        assert_eq!(inspect_store(&store, None)?, StoreState::Existing);

        let root_id = store.join(".gpg-id");
        fs::write(&root_id, "# placeholder\n")?;
        assert_eq!(inspect_store(&store, None)?, StoreState::Existing);
        fs::write(root_id, "demo@example.invalid\n")?;
        assert_eq!(
            inspect_store(&store, None)?,
            StoreState::Configured("demo@example.invalid".into())
        );
        assert_eq!(
            inspect_store(&store, Some("override@example.invalid"))?,
            StoreState::Configured("override@example.invalid".into())
        );
        Ok(())
    }
}
