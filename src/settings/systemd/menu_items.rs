use std::process::Command;

use anyhow::Result;

use crate::common::shell::shell_quote;
use crate::common::systemd::{ServiceScope, SystemdManager};
use crate::menu_utils::{FzfPreview, FzfResult, FzfSelectable, FzfWrapper, Header};
use crate::ui::catppuccin::{colors, format_icon, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

#[derive(Clone)]
pub enum SystemdMenuEntry {
    SystemServices,
    UserServices,
    Back,
}

impl SystemdMenuEntry {
    pub fn run(&self) -> Result<()> {
        match self {
            SystemdMenuEntry::SystemServices => run_services_menu(ServiceScope::System),
            SystemdMenuEntry::UserServices => run_services_menu(ServiceScope::User),
            SystemdMenuEntry::Back => Ok(()),
        }
    }
}

impl FzfSelectable for SystemdMenuEntry {
    fn fzf_display_text(&self) -> String {
        match self {
            SystemdMenuEntry::SystemServices => format!(
                "{} System Services",
                format_icon_colored(NerdFont::Server, colors::PEACH)
            ),
            SystemdMenuEntry::UserServices => format!(
                "{} User Services",
                format_icon_colored(NerdFont::User, colors::TEAL)
            ),
            SystemdMenuEntry::Back => format!("{} Back", format_icon(NerdFont::ArrowLeft)),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            SystemdMenuEntry::SystemServices => "system-services".to_string(),
            SystemdMenuEntry::UserServices => "user-services".to_string(),
            SystemdMenuEntry::Back => "back".to_string(),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            SystemdMenuEntry::SystemServices => PreviewBuilder::new()
                .header(NerdFont::Server, "System Services")
                .text("Manage system-level systemd services.")
                .blank()
                .text("These services run at boot and are")
                .text("managed by the system administrator.")
                .build(),
            SystemdMenuEntry::UserServices => PreviewBuilder::new()
                .header(NerdFont::User, "User Services")
                .text("Manage user-level systemd services.")
                .blank()
                .text("These services run in your user session")
                .text("and don't require root privileges.")
                .build(),
            SystemdMenuEntry::Back => PreviewBuilder::new()
                .header(NerdFont::ArrowLeft, "Back")
                .text("Return to settings.")
                .build(),
        }
    }
}

#[derive(Clone)]
pub struct ServiceItem {
    pub name: String,
    pub description: String,
    pub active: String,
    pub enabled: String,
    pub scope: ServiceScope,
}

impl ServiceItem {
    fn new(
        name: String,
        description: String,
        active: String,
        enabled: String,
        scope: ServiceScope,
    ) -> Self {
        Self {
            name,
            description,
            active,
            enabled,
            scope,
        }
    }

    fn get_manager(&self) -> SystemdManager {
        match self.scope {
            ServiceScope::System => SystemdManager::system_with_sudo(),
            ServiceScope::User => SystemdManager::user(),
        }
    }
}

impl FzfSelectable for ServiceItem {
    fn fzf_display_text(&self) -> String {
        let (active_icon, active_color) = match self.active.as_str() {
            "active" => (NerdFont::CheckCircle, colors::GREEN),
            "failed" => (NerdFont::CrossCircle, colors::RED),
            "inactive" => (NerdFont::Circle, colors::OVERLAY0),
            _ => (NerdFont::Question, colors::YELLOW),
        };

        format!(
            "{} {} {}",
            format_icon_colored(active_icon, active_color),
            self.name,
            truncate(&self.description, 40)
        )
    }

    fn fzf_key(&self) -> String {
        format!(
            "{}:{}",
            self.name,
            match self.scope {
                ServiceScope::System => "system",
                ServiceScope::User => "user",
            }
        )
    }

    fn fzf_preview(&self) -> FzfPreview {
        let exe = std::env::current_exe()
            .ok()
            .and_then(|p| p.to_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "ins".to_string());

        let key = self.fzf_key();
        FzfPreview::Command(format!(
            "{exe} preview --id systemd-service --key {}",
            shell_quote(&key)
        ))
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}...", &s[..max_len - 3])
    } else {
        s.to_string()
    }
}

#[derive(Clone)]
pub enum ServiceAction {
    Start,
    Stop,
    Restart,
    Enable,
    Disable,
    Logs,
    Back,
}

impl FzfSelectable for ServiceAction {
    fn fzf_display_text(&self) -> String {
        match self {
            ServiceAction::Start => format!(
                "{} Start",
                format_icon_colored(NerdFont::Play, colors::GREEN)
            ),
            ServiceAction::Stop => {
                format!("{} Stop", format_icon_colored(NerdFont::Stop, colors::RED))
            }
            ServiceAction::Restart => format!(
                "{} Restart",
                format_icon_colored(NerdFont::Refresh, colors::YELLOW)
            ),
            ServiceAction::Enable => format!(
                "{} Enable",
                format_icon_colored(NerdFont::ToggleOn, colors::GREEN)
            ),
            ServiceAction::Disable => format!(
                "{} Disable",
                format_icon_colored(NerdFont::ToggleOff, colors::PEACH)
            ),
            ServiceAction::Logs => format!("{} View Logs", format_icon(NerdFont::Terminal)),
            ServiceAction::Back => format!("{} Back", format_icon(NerdFont::ArrowLeft)),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            ServiceAction::Start => "start".to_string(),
            ServiceAction::Stop => "stop".to_string(),
            ServiceAction::Restart => "restart".to_string(),
            ServiceAction::Enable => "enable".to_string(),
            ServiceAction::Disable => "disable".to_string(),
            ServiceAction::Logs => "logs".to_string(),
            ServiceAction::Back => "back".to_string(),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            ServiceAction::Start => PreviewBuilder::new()
                .header(NerdFont::Play, "Start Service")
                .text("Start the selected systemd service.")
                .blank()
                .subtext("The service will begin running immediately.")
                .build(),
            ServiceAction::Stop => PreviewBuilder::new()
                .header(NerdFont::Stop, "Stop Service")
                .text("Stop the selected systemd service.")
                .blank()
                .subtext("The service will stop running.")
                .build(),
            ServiceAction::Restart => PreviewBuilder::new()
                .header(NerdFont::Refresh, "Restart Service")
                .text("Restart the selected systemd service.")
                .blank()
                .subtext("The service will be stopped and started again.")
                .build(),
            ServiceAction::Enable => PreviewBuilder::new()
                .header(NerdFont::ToggleOn, "Enable Service")
                .text("Enable the service to start at boot.")
                .blank()
                .subtext("The service will start automatically when the system boots.")
                .build(),
            ServiceAction::Disable => PreviewBuilder::new()
                .header(NerdFont::ToggleOff, "Disable Service")
                .text("Disable the service from starting at boot.")
                .blank()
                .subtext("The service will not start automatically on boot.")
                .build(),
            ServiceAction::Logs => PreviewBuilder::new()
                .header(NerdFont::Terminal, "View Logs")
                .text("View live logs for this service.")
                .blank()
                .subtext("Press Ctrl+C to exit the log viewer.")
                .build(),
            ServiceAction::Back => PreviewBuilder::new()
                .header(NerdFont::ArrowLeft, "Back")
                .text("Return to service list.")
                .build(),
        }
    }
}

pub fn run_systemd_menu() -> Result<()> {
    let entries = vec![
        MenuItem::entry(SystemdMenuEntry::SystemServices),
        MenuItem::entry(SystemdMenuEntry::UserServices),
        MenuItem::line(),
        MenuItem::entry(SystemdMenuEntry::Back),
    ];

    loop {
        let builder = FzfWrapper::builder()
            .header(Header::fancy("Systemd Manager"))
            .prompt("Select")
            .args(crate::ui::catppuccin::fzf_mocha_args())
            .responsive_layout();

        let result = builder.select_menu(entries.clone())?;

        match result {
            FzfResult::Selected(entry) => {
                entry.run()?;
            }
            _ => break,
        }
    }

    Ok(())
}

fn run_services_menu(scope: ServiceScope) -> Result<()> {
    let services = list_services(scope)?;

    if services.is_empty() {
        let scope_name = match scope {
            ServiceScope::System => "system",
            ServiceScope::User => "user",
        };
        anyhow::bail!("No {} services found", scope_name);
    }

    let title = match scope {
        ServiceScope::System => "System Services",
        ServiceScope::User => "User Services",
    };

    let service_entries: Vec<MenuItem<ServiceItem>> = services
        .iter()
        .map(|s| MenuItem::entry(s.clone()))
        .collect();

    loop {
        let builder = FzfWrapper::builder()
            .header(Header::fancy(title))
            .prompt("Select service")
            .args(crate::ui::catppuccin::fzf_mocha_args())
            .responsive_layout();

        let result = builder.select_menu(service_entries.clone())?;

        match result {
            FzfResult::Selected(service) => {
                let action = select_service_action(&service)?;
                handle_service_action(&service, action)?;
            }
            _ => break,
        }
    }

    Ok(())
}

fn list_services(scope: ServiceScope) -> Result<Vec<ServiceItem>> {
    let scope_args: Vec<&str> = match scope {
        ServiceScope::System => vec![],
        ServiceScope::User => vec!["--user"],
    };

    let output = Command::new("systemctl")
        .args([
            "list-units",
            "--type=service",
            "--all",
            "--no-pager",
            "--plain",
            "--no-legend",
        ])
        .args(&scope_args)
        .output()?;

    if !output.status.success() {
        return Ok(vec![]);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut services = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }

        let name = parts[0].replace(".service", "");
        let _load = parts[1];
        let active = parts[2];
        let _sub = parts[3];
        let description = if parts.len() > 4 {
            parts[4..].join(" ")
        } else {
            String::new()
        };

        let enabled = get_service_enabled_state(&name, scope);

        services.push(ServiceItem::new(
            name,
            description,
            active.to_string(),
            enabled,
            scope,
        ));
    }

    services.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(services)
}

fn get_service_enabled_state(name: &str, scope: ServiceScope) -> String {
    let scope_args: Vec<&str> = match scope {
        ServiceScope::System => vec![],
        ServiceScope::User => vec!["--user"],
    };

    let output = Command::new("systemctl")
        .args(["is-enabled", name])
        .args(&scope_args)
        .output();

    match output {
        Ok(o) => {
            let state = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if o.status.success() {
                state
            } else {
                match state.as_str() {
                    "disabled" => "disabled".to_string(),
                    _ => state,
                }
            }
        }
        Err(_) => "unknown".to_string(),
    }
}

fn select_service_action(service: &ServiceItem) -> Result<ServiceAction> {
    let actions = vec![
        MenuItem::entry(ServiceAction::Start),
        MenuItem::entry(ServiceAction::Stop),
        MenuItem::entry(ServiceAction::Restart),
        MenuItem::separator("Boot"),
        MenuItem::entry(ServiceAction::Enable),
        MenuItem::entry(ServiceAction::Disable),
        MenuItem::line(),
        MenuItem::entry(ServiceAction::Logs),
        MenuItem::line(),
        MenuItem::entry(ServiceAction::Back),
    ];

    let result = FzfWrapper::builder()
        .header(Header::fancy(&service.name))
        .prompt("Action")
        .args(crate::ui::catppuccin::fzf_mocha_args())
        .select_menu(actions)?;

    match result {
        FzfResult::Selected(action) => Ok(action),
        _ => Err(anyhow::anyhow!("Cancelled")),
    }
}

fn handle_service_action(service: &ServiceItem, action: ServiceAction) -> Result<()> {
    match action {
        ServiceAction::Start => {
            service.get_manager().start(&service.name)?;
            println!("Service '{}' started.", service.name);
        }
        ServiceAction::Stop => {
            service.get_manager().stop(&service.name)?;
            println!("Service '{}' stopped.", service.name);
        }
        ServiceAction::Restart => {
            service.get_manager().restart(&service.name)?;
            println!("Service '{}' restarted.", service.name);
        }
        ServiceAction::Enable => {
            service.get_manager().enable(&service.name)?;
            println!("Service '{}' enabled.", service.name);
        }
        ServiceAction::Disable => {
            service.get_manager().disable(&service.name)?;
            println!("Service '{}' disabled.", service.name);
        }
        ServiceAction::Logs => {
            view_service_logs(service)?;
        }
        ServiceAction::Back => return Ok(()),
    }
    Ok(())
}

fn view_service_logs(service: &ServiceItem) -> Result<()> {
    let scope_args: Vec<&str> = match service.scope {
        ServiceScope::System => vec![],
        ServiceScope::User => vec!["--user"],
    };

    println!("Following logs for '{}' (Ctrl+C to exit)...", service.name);

    let mut cmd = Command::new("journalctl");
    cmd.args(["-u", &service.name, "-n", "50", "-f"]);
    cmd.args(&scope_args);

    cmd.status()?;

    Ok(())
}

use crate::menu_utils::MenuItem;

pub fn launch_cockpit() -> Result<()> {
    use crate::common::package::{ensure_all, InstallResult};
    use crate::common::systemd::SystemdManager;
    use crate::menu_utils::FzfWrapper;
    use crate::settings::deps::COCKPIT_DEPS;

    match ensure_all(COCKPIT_DEPS)? {
        InstallResult::Installed | InstallResult::AlreadyInstalled => {}
        _ => {
            println!("Cockpit launch cancelled.");
            return Ok(());
        }
    }

    let systemd = SystemdManager::system_with_sudo();
    const COCKPIT_SOCKET_NAME: &str = "cockpit.socket";

    if !systemd.is_enabled(COCKPIT_SOCKET_NAME) {
        systemd.enable_and_start(COCKPIT_SOCKET_NAME)?;

        std::thread::sleep(std::time::Duration::from_secs(2));

        let username = std::env::var("USER").unwrap_or_else(|_| "your username".to_string());
        FzfWrapper::builder()
            .message(format!(
                "Cockpit is starting...\n\nSign in with '{}' in the browser window.",
                username
            ))
            .title("Cockpit")
            .message_dialog()?;
    }

    std::process::Command::new("chromium")
        .arg("--app=http://localhost:9090")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;

    Ok(())
}
