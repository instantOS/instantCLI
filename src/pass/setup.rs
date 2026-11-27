//! First-run setup for the `pass` password store.
//!
//! Interactive pass commands call [`ensure_store`] before opening a menu. When
//! the store has no recipient configuration, setup opens in a normal terminal
//! where the user can create a GPG key or choose an existing secret key. The
//! selected key is then written to `.gpg-id` by `pass init`.

use std::env;
use std::fs;
use std::io::{IsTerminal, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::menu_utils::{DialogOutcome, FzfSelectable, FzfWrapper, HeaderBuilder};
use crate::ui::catppuccin::{colors, format_icon_colored, format_with_color};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::utils::{ensure_core_dependencies, password_store_dir, run_pass_status};

/// Whether the password store still needs a recipient/GPG setup.
pub(super) fn needs_setup() -> Result<bool> {
    let store_dir = password_store_dir()?;
    Ok(!store_is_configured(&store_dir))
}

/// Ensure the store is configured. When called without a terminal (for example
/// from `--instantmenu`), setup is opened in the configured terminal window.
pub(super) fn ensure_store(debug: bool) -> Result<bool> {
    let result = ensure_store_inner(debug);
    if let Err(error) = &result
        && interactive_terminal()
    {
        let _ = message("INS Pass Setup Failed", &format!("{error:#}"));
    }
    result
}

fn ensure_store_inner(debug: bool) -> Result<bool> {
    let store_dir = password_store_dir()?;
    if store_is_configured(&store_dir) {
        if !store_dir.exists() {
            fs::create_dir_all(&store_dir)
                .with_context(|| format!("Failed to create {}", store_dir.display()))?;
        }
        return Ok(true);
    }

    if !interactive_terminal() {
        launch_in_terminal(debug)?;
        let store_dir = password_store_dir()?;
        return Ok(store_is_configured(&store_dir));
    }

    ensure_core_dependencies()?;
    run_onboarding(&store_dir)
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
    if store_is_configured(&store_dir) {
        let recipient = configured_recipient(&store_dir);
        message(
            "INS Pass Is Already Set Up",
            &format!(
                "Store: {}\nRecipient: {}\n\n\
                 To use a different store or key, set PASSWORD_STORE_DIR or run \
                 `pass init` with another key.",
                store_dir.display(),
                recipient
            ),
        )?;
        return Ok(true);
    }

    ensure_core_dependencies()?;
    run_onboarding(&store_dir)
}

fn launch_in_terminal(debug: bool) -> Result<()> {
    let current_exe = env::current_exe().context("Failed to locate the ins executable")?;
    let mut args = Vec::new();
    if debug {
        args.push("--debug".to_string());
    }
    args.extend(["pass".to_string(), "setup".to_string()]);

    instantos_terminal::Launcher::new(current_exe.as_os_str())
        .app_id("ins-pass")
        .title("INS Pass Setup")
        .args(&args)
        .status()
        .context("Failed to open the setup terminal")?;
    Ok(())
}

fn interactive_terminal() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

fn store_is_configured(store_dir: &Path) -> bool {
    let key_override = env::var("PASSWORD_STORE_KEY")
        .ok()
        .is_some_and(|value| !value.trim().is_empty());
    key_override || store_dir.join(".gpg-id").is_file()
}

fn configured_recipient(store_dir: &Path) -> String {
    if let Ok(key) = env::var("PASSWORD_STORE_KEY")
        && !key.trim().is_empty()
    {
        return key;
    }
    fs::read_to_string(store_dir.join(".gpg-id"))
        .ok()
        .map(|contents| contents.trim().to_string())
        .filter(|contents| !contents.is_empty())
        .unwrap_or_else(|| "(unknown)".to_string())
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
                    "Anyone who can read your password store files can decrypt \
                     it. Only use this for a disposable demo or test environment.",
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
    run_pass_status(["init", &key.fingerprint])?;
    let gpg_id = store_dir.join(".gpg-id");
    if !gpg_id.is_file() {
        bail!(
            "`pass init` succeeded but did not create {}",
            gpg_id.display()
        );
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

fn generate_gpg_key(identity: &str, passphrase: &str) -> Result<String> {
    let mut child = Command::new("gpg")
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
    parse_created_fingerprint(&stdout)
        .context("gpg created a key but did not report its fingerprint")
}

fn parse_created_fingerprint(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        line.strip_prefix("[GNUPG:] KEY_CREATED ")
            .and_then(|rest| rest.split_whitespace().last())
            .map(ToString::to_string)
    })
}

fn list_secret_keys() -> Result<Vec<SecretKey>> {
    let output = Command::new("gpg")
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
    unusable: bool,
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
                current = Some(PendingKey {
                    fingerprint: None,
                    user_id: None,
                    has_encryption: capabilities.contains('e'),
                    unusable: unusable_validity(validity),
                });
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
                    let validity = fields.get(1).copied().unwrap_or("");
                    let capabilities = fields.get(11).copied().unwrap_or("");
                    if !unusable_validity(validity) && capabilities.contains('e') {
                        pending.has_encryption = true;
                    }
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
    if pending.unusable || !pending.has_encryption {
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
    fn store_configuration_requires_a_gpg_id_file() {
        let store = tempfile::tempdir().unwrap();
        assert!(!store_is_configured(store.path()));
        fs::write(store.path().join(".gpg-id"), "demo@example.invalid\n").unwrap();
        assert!(store_is_configured(store.path()));
    }
}
