use crate::common::deps::UMU_LAUNCHER;
use crate::common::package::Dependency;
use crate::game::launch_command::{LaunchCommand, LaunchCommandKind, WineRunner};

static UMU_RUN_DEPENDENCIES: &[&Dependency] = &[&UMU_LAUNCHER];
static NO_DEPENDENCIES: &[&Dependency] = &[];

pub(crate) fn dependencies_for_launch_command(
    command: &LaunchCommand,
) -> &'static [&'static Dependency] {
    match &command.kind {
        LaunchCommandKind::Wine(wine) => dependencies_for_wine_runner(wine.runner),
        _ => NO_DEPENDENCIES,
    }
}

pub(crate) fn dependencies_for_wine_runner(runner: WineRunner) -> &'static [&'static Dependency] {
    match runner {
        WineRunner::UmuRun => UMU_RUN_DEPENDENCIES,
        WineRunner::Wine => NO_DEPENDENCIES,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::game::launch_command::{LaunchWrappers, ProtonSelection, WineLaunchCommand};

    #[test]
    fn umu_run_launch_commands_require_umu_launcher() {
        let command = LaunchCommand {
            wrappers: LaunchWrappers::default(),
            kind: LaunchCommandKind::Wine(WineLaunchCommand {
                runner: WineRunner::UmuRun,
                prefix: Some(PathBuf::from("/games/prefix")),
                proton: ProtonSelection::UmuProtonLatest,
                executable: PathBuf::from("/games/Test.exe"),
            }),
        };

        assert_eq!(
            dependencies_for_launch_command(&command)
                .iter()
                .map(|dep| dep.name)
                .collect::<Vec<_>>(),
            vec!["umu-launcher"]
        );
    }

    #[test]
    fn plain_wine_launch_commands_do_not_require_umu_launcher() {
        let command = LaunchCommand {
            wrappers: LaunchWrappers::default(),
            kind: LaunchCommandKind::Wine(WineLaunchCommand {
                runner: WineRunner::Wine,
                prefix: Some(PathBuf::from("/games/prefix")),
                proton: ProtonSelection::UmuProtonLatest,
                executable: PathBuf::from("/games/Test.exe"),
            }),
        };

        assert!(dependencies_for_launch_command(&command).is_empty());
    }
}
