use anyhow::{Context, Result};
use std::process::Command;

/// Represents the scope of a systemd service (system or user)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceScope {
    System,
    User,
}

impl ServiceScope {
    /// Get the systemctl command arguments for this scope
    pub fn systemctl_args(&self) -> Vec<&'static str> {
        match self {
            ServiceScope::System => vec![],
            ServiceScope::User => vec!["--user"],
        }
    }
}

/// Represents the state of a systemd service
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceState {
    Active,
    Inactive,
    Failed,
    Unknown(String),
}

/// Represents the enablement state of a systemd service
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceEnablement {
    Enabled,
    Disabled,
    Static,
    Unknown(String),
}

/// Configuration for creating a user service
#[derive(Debug, Clone)]
pub struct UserServiceConfig {
    pub name: String,
    pub description: String,
    pub exec_start: String,
    pub restart: Option<String>,
    pub restart_sec: Option<u32>,
    pub wanted_by: Option<String>,
}

impl UserServiceConfig {
    /// Create a new user service configuration
    pub fn new(name: impl Into<String>, description: impl Into<String>, exec_start: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            exec_start: exec_start.into(),
            restart: Some("always".to_string()),
            restart_sec: Some(5),
            wanted_by: Some("default.target".to_string()),
        }
    }

    /// Set the restart policy
    pub fn with_restart(mut self, restart: impl Into<String>) -> Self {
        self.restart = Some(restart.into());
        self
    }

    /// Set the restart delay in seconds
    pub fn with_restart_sec(mut self, sec: u32) -> Self {
        self.restart_sec = Some(sec);
        self
    }

    /// Set the target that wants this service
    pub fn with_wanted_by(mut self, target: impl Into<String>) -> Self {
        self.wanted_by = Some(target.into());
        self
    }

    /// Generate the service file content
    pub fn to_service_content(&self) -> String {
        let mut content = format!(
            "[Unit]\nDescription={}\n\n[Service]\nExecStart={}\n",
            self.description, self.exec_start
        );

        if let Some(restart) = &self.restart {
            content.push_str(&format!("Restart={}\n", restart));
        }

        if let Some(restart_sec) = self.restart_sec {
            content.push_str(&format!("RestartSec={}\n", restart_sec));
        }

        content.push_str("\n[Install]\n");
        if let Some(wanted_by) = &self.wanted_by {
            content.push_str(&format!("WantedBy={}\n", wanted_by));
        }

        content
    }
}

/// Command executor function type for privileged operations
pub type CommandExecutor = Box<dyn Fn(&str, &[&str]) -> Result<std::process::ExitStatus> + Send + Sync>;

/// Systemd service manager for common operations
pub struct SystemdManager {
    scope: ServiceScope,
    command_executor: Option<CommandExecutor>,
}

impl SystemdManager {
    /// Create a new systemd manager for the given scope
    pub fn new(scope: ServiceScope) -> Self {
        Self { 
            scope,
            command_executor: None,
        }
    }

    /// Create a new systemd manager with a custom command executor for privileged operations
    pub fn new_with_executor(scope: ServiceScope, executor: CommandExecutor) -> Self {
        Self {
            scope,
            command_executor: Some(executor),
        }
    }

    /// Create a systemd manager for system services
    pub fn system() -> Self {
        Self::new(ServiceScope::System)
    }

    /// Create a systemd manager for system services with privileged command execution
    pub fn system_privileged(executor: CommandExecutor) -> Self {
        Self::new_with_executor(ServiceScope::System, executor)
    }

    /// Create a systemd manager for user services
    pub fn user() -> Self {
        Self::new(ServiceScope::User)
    }

    /// Check if a service is currently active
    pub fn is_active(&self, service_name: &str) -> bool {
        self.run_systemctl(&["is-active", "--quiet", service_name])
            .map(|status| status.success())
            .unwrap_or(false)
    }

    /// Check if a service is enabled
    pub fn is_enabled(&self, service_name: &str) -> bool {
        self.run_systemctl(&["is-enabled", "--quiet", service_name])
            .map(|status| status.success())
            .unwrap_or(false)
    }

    /// Get the detailed state of a service
    pub fn get_state(&self, service_name: &str) -> ServiceState {
        let output = self.run_systemctl(&["is-active", service_name]);
        
        match output {
            Ok(status) if status.success() => ServiceState::Active,
            Ok(status) => {
                let exit_code = status.code().unwrap_or(1);
                match exit_code {
                    3 => ServiceState::Inactive,
                    4 => ServiceState::Failed,
                    _ => ServiceState::Unknown(format!("Exit code: {}", exit_code)),
                }
            }
            Err(_) => ServiceState::Unknown("Command failed".to_string()),
        }
    }

    /// Get the enablement state of a service
    pub fn get_enablement(&self, service_name: &str) -> ServiceEnablement {
        let output = self.run_systemctl(&["is-enabled", service_name]);
        
        match output {
            Ok(status) if status.success() => ServiceEnablement::Enabled,
            Ok(status) => {
                let exit_code = status.code().unwrap_or(1);
                match exit_code {
                    1 => ServiceEnablement::Disabled,
                    2 => ServiceEnablement::Static,
                    _ => ServiceEnablement::Unknown(format!("Exit code: {}", exit_code)),
                }
            }
            Err(_) => ServiceEnablement::Unknown("Command failed".to_string()),
        }
    }

    /// Start a service
    pub fn start(&self, service_name: &str) -> Result<()> {
        let status = self.run_systemctl(&["start", service_name])
            .with_context(|| format!("Failed to start service '{}'", service_name))?;
        
        if !status.success() {
            anyhow::bail!("Failed to start service '{}'", service_name);
        }
        Ok(())
    }

    /// Stop a service
    pub fn stop(&self, service_name: &str) -> Result<()> {
        let status = self.run_systemctl(&["stop", service_name])
            .with_context(|| format!("Failed to stop service '{}'", service_name))?;
        
        if !status.success() {
            anyhow::bail!("Failed to stop service '{}'", service_name);
        }
        Ok(())
    }

    /// Enable a service
    pub fn enable(&self, service_name: &str) -> Result<()> {
        let status = self.run_systemctl(&["enable", service_name])
            .with_context(|| format!("Failed to enable service '{}'", service_name))?;
        
        if !status.success() {
            anyhow::bail!("Failed to enable service '{}'", service_name);
        }
        Ok(())
    }

    /// Disable a service
    pub fn disable(&self, service_name: &str) -> Result<()> {
        let status = self.run_systemctl(&["disable", service_name])
            .with_context(|| format!("Failed to disable service '{}'", service_name))?;
        
        if !status.success() {
            anyhow::bail!("Failed to disable service '{}'", service_name);
        }
        Ok(())
    }

    /// Enable and start a service
    pub fn enable_and_start(&self, service_name: &str) -> Result<()> {
        let status = self.run_systemctl(&["enable", "--now", service_name])
            .with_context(|| format!("Failed to enable and start service '{}'", service_name))?;
        
        if !status.success() {
            anyhow::bail!("Failed to enable and start service '{}'", service_name);
        }
        Ok(())
    }

    /// Disable and stop a service
    pub fn disable_and_stop(&self, service_name: &str) -> Result<()> {
        let status = self.run_systemctl(&["disable", "--now", service_name])
            .with_context(|| format!("Failed to disable and stop service '{}'", service_name))?;
        
        if !status.success() {
            anyhow::bail!("Failed to disable and stop service '{}'", service_name);
        }
        Ok(())
    }

    /// Reload systemd daemon
    pub fn daemon_reload(&self) -> Result<()> {
        let status = self.run_systemctl(&["daemon-reload"])
            .context("Failed to reload systemd daemon")?;
        
        if !status.success() {
            anyhow::bail!("Failed to reload systemd daemon");
        }
        Ok(())
    }

    /// Create a user service file from configuration
    pub fn create_user_service(&self, config: &UserServiceConfig) -> Result<()> {
        if self.scope != ServiceScope::User {
            anyhow::bail!("create_user_service can only be used with user scope");
        }

        let service_dir = self.get_user_service_dir()?;
        std::fs::create_dir_all(&service_dir)
            .with_context(|| format!("Failed to create service directory {}", service_dir.display()))?;

        let service_path = service_dir.join(format!("{}.service", config.name));
        let service_content = config.to_service_content();
        
        std::fs::write(&service_path, service_content)
            .with_context(|| format!("Failed to write service file {}", service_path.display()))?;

        // Reload daemon to pick up the new service file
        self.daemon_reload()?;

        Ok(())
    }

    /// Create a user service file with custom content
    pub fn create_user_service_file(
        &self,
        service_name: &str,
        service_content: &str,
    ) -> Result<()> {
        if self.scope != ServiceScope::User {
            anyhow::bail!("create_user_service_file can only be used with user scope");
        }

        let service_dir = self.get_user_service_dir()?;
        std::fs::create_dir_all(&service_dir)
            .with_context(|| format!("Failed to create service directory {}", service_dir.display()))?;

        let service_path = service_dir.join(format!("{}.service", service_name));
        std::fs::write(&service_path, service_content)
            .with_context(|| format!("Failed to write service file {}", service_path.display()))?;

        // Reload daemon to pick up the new service file
        self.daemon_reload()?;

        Ok(())
    }

    /// Remove a user service file
    pub fn remove_user_service_file(&self, service_name: &str) -> Result<()> {
        if self.scope != ServiceScope::User {
            anyhow::bail!("remove_user_service_file can only be used with user scope");
        }

        let service_dir = self.get_user_service_dir()?;
        let service_path = service_dir.join(format!("{}.service", service_name));

        if service_path.exists() {
            std::fs::remove_file(&service_path)
                .with_context(|| format!("Failed to remove service file {}", service_path.display()))?;
            // Reload daemon to reflect the removal
            self.daemon_reload()?;
        }

        Ok(())
    }

    /// Get the user service directory path
    fn get_user_service_dir(&self) -> Result<std::path::PathBuf> {
        let config_dir = dirs::config_dir()
            .context("unable to determine user config directory")?;
        Ok(config_dir.join("systemd").join("user"))
    }

    /// Run systemctl with the appropriate scope arguments
    fn run_systemctl(&self, args: &[&str]) -> Result<std::process::ExitStatus> {
        let mut full_args = self.scope.systemctl_args();
        full_args.extend_from_slice(args);

        if let Some(ref executor) = self.command_executor {
            executor("systemctl", &full_args)
        } else {
            let mut cmd = Command::new("systemctl");
            cmd.args(&full_args);

            let status = cmd.status()
                .with_context(|| format!("Failed to run systemctl with args: {:?}", full_args))?;
            Ok(status)
        }
    }
}

/// Convenience functions for common systemd operations
pub mod utils {
    use super::*;

    /// Check if a system service is active
    pub fn system_service_is_active(service_name: &str) -> bool {
        SystemdManager::system().is_active(service_name)
    }

    /// Check if a system service is enabled
    pub fn system_service_is_enabled(service_name: &str) -> bool {
        SystemdManager::system().is_enabled(service_name)
    }

    /// Check if a user service is active
    pub fn user_service_is_active(service_name: &str) -> bool {
        SystemdManager::user().is_active(service_name)
    }

    /// Check if a user service is enabled
    pub fn user_service_is_enabled(service_name: &str) -> bool {
        SystemdManager::user().is_enabled(service_name)
    }

    /// Create a udiskie service configuration
    pub fn create_udiskie_service_config() -> UserServiceConfig {
        UserServiceConfig::new(
            "udiskie",
            "udiskie removable media automounter",
            "/usr/bin/udiskie"
        )
    }

    /// Create a standard user service file content for udiskie (legacy)
    pub fn create_udiskie_service_content() -> String {
        create_udiskie_service_config().to_service_content()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_scope_args() {
        assert_eq!(ServiceScope::System.systemctl_args(), vec![] as Vec<&'static str>);
        assert_eq!(ServiceScope::User.systemctl_args(), vec!["--user"]);
    }

    #[test]
    fn test_user_service_config() {
        let config = UserServiceConfig::new("test-service", "Test Service", "/usr/bin/test");
        let content = config.to_service_content();
        
        assert!(content.contains("Description=Test Service"));
        assert!(content.contains("ExecStart=/usr/bin/test"));
        assert!(content.contains("Restart=always"));
        assert!(content.contains("RestartSec=5"));
        assert!(content.contains("WantedBy=default.target"));
    }

    #[test]
    fn test_user_service_config_custom() {
        let config = UserServiceConfig::new("custom-service", "Custom Service", "/usr/bin/custom")
            .with_restart("on-failure")
            .with_restart_sec(10)
            .with_wanted_by("graphical.target");
        
        let content = config.to_service_content();
        
        assert!(content.contains("Description=Custom Service"));
        assert!(content.contains("ExecStart=/usr/bin/custom"));
        assert!(content.contains("Restart=on-failure"));
        assert!(content.contains("RestartSec=10"));
        assert!(content.contains("WantedBy=graphical.target"));
    }

    #[test]
    fn test_udiskie_service_config() {
        let config = utils::create_udiskie_service_config();
        assert_eq!(config.name, "udiskie");
        assert_eq!(config.description, "udiskie removable media automounter");
        assert_eq!(config.exec_start, "/usr/bin/udiskie");
    }

    #[test]
    fn test_udiskie_service_content() {
        let content = utils::create_udiskie_service_content();
        assert!(content.contains("Description=udiskie removable media automounter"));
        assert!(content.contains("ExecStart=/usr/bin/udiskie"));
        assert!(content.contains("WantedBy=default.target"));
    }
}