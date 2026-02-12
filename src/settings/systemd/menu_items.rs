use std::process::Command;

use anyhow::Result;

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

        let enabled_color = match self.enabled.as_str() {
            "enabled" => colors::GREEN,
            "disabled" => colors::OVERLAY0,
            _ => colors::SUBTEXT0,
        };

        format!(
            "{} {}  {}{}{} {}",
            format_icon_colored(active_icon, active_color),
            self.name,
            format_icon_colored(NerdFont::ToggleOn, enabled_color),
            self.enabled,
            format_icon(NerdFont::Bullet),
            truncate(&self.description, 50)
        )
    }

    fn fzf_key(&self) -> String {
        self.name.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        let active_color = match self.active.as_str() {
            "active" => colors::GREEN,
            "failed" => colors::RED,
            "inactive" => colors::OVERLAY0,
            _ => colors::YELLOW,
        };

        let enabled_color = match self.enabled.as_str() {
            "enabled" => colors::GREEN,
            "disabled" => colors::OVERLAY0,
            _ => colors::SUBTEXT0,
        };

        let scope_label = match self.scope {
            ServiceScope::System => "System",
            ServiceScope::User => "User",
        };

        let mut builder = PreviewBuilder::new()
            .header(NerdFont::Server, &self.name)
            .field("Description", &self.description)
            .blank()
            .line(
                active_color,
                Some(NerdFont::PowerOff),
                &format!("Status: {}", self.active),
            )
            .line(
                enabled_color,
                Some(NerdFont::ToggleOn),
                &format!("Enabled: {}", self.enabled),
            )
            .field("Scope", scope_label)
            .blank()
            .separator()
            .blank()
            .text("Actions:")
            .bullet("Start/Stop/Restart the service")
            .bullet("Enable/Disable at boot")
            .bullet("View live logs (journalctl -f)");

        FzfPreview::Command(format!(
            r#"SERVICE="{}" SCOPE="{}"
echo "───────────────────────────────────"
echo -e "\033[1;38;2;203;164;213m$(systemctl show -p Description --value "$SERVICE" {} 2>/dev/null)\033[0m"
echo "───────────────────────────────────"
echo ""
ACTIVE=$(systemctl is-active "$SERVICE" {} 2>/dev/null)
ENABLED=$(systemctl is-enabled "$SERVICE" {} 2>/dev/null)

case "$ACTIVE" in
    active) ACTIVE_COLOR="\033[38;2;166;227;161m" ;;
    failed) ACTIVE_COLOR="\033[38;2;243;139;168m" ;;
    inactive) ACTIVE_COLOR="\033[38;2;137;180;183m" ;;
    *) ACTIVE_COLOR="\033[38;2;249;226;175m" ;;
esac

case "$ENABLED" in
    enabled) ENABLED_COLOR="\033[38;2;166;227;161m" ;;
    disabled) ENABLED_COLOR="\033[38;2;137;180;183m" ;;
    *) ENABLED_COLOR="\033[38;2;205;214;219m" ;;
esac

echo -e "Status:   $ACTIVE_COLOR$ACTIVE\033[0m"
echo -e "Enabled:  $ENABLED_COLOR$ENABLED\033[0m"
echo "Scope:    $SCOPE"
echo ""
echo "───────────────────────────────────"
echo "Actions:"
echo "Start/Stop/Restart: Control the service"
echo "Enable/Disable:     Boot behavior"
echo "Logs:              View live journalctl"
"#,
            self.name,
            if self.scope == ServiceScope::User {
                "--user"
            } else {
                ""
            },
            if self.scope == ServiceScope::User {
                "--user"
            } else {
                ""
            },
            if self.scope == ServiceScope::User {
                "--user"
            } else {
                ""
            },
            if self.scope == ServiceScope::User {
                "--user"
            } else {
                ""
            }
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

    let mut builder = FzfWrapper::builder()
        .header(Header::fancy("Systemd Manager"))
        .prompt("Select")
        .args(crate::ui::catppuccin::fzf_mocha_args())
        .responsive_layout();

    loop {
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

    let mut builder = FzfWrapper::builder()
        .header(Header::fancy(title))
        .prompt("Select service")
        .args(crate::ui::catppuccin::fzf_mocha_args())
        .responsive_layout();

    loop {
        let result = builder.select_menu_with_key(services.clone())?;

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
        let load = parts[1];
        let active = parts[2];
        let sub = parts[3];
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
