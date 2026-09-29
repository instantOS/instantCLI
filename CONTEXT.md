# instantCLI

instantCLI manages interactive system configuration and turns a completed installer configuration into changes on the target system.

## Installer

**Wizard state**:
The possibly incomplete set of answers and navigation metadata produced while a person moves through the installer.
_Avoid_: Install plan, configuration

**Install plan**:
A complete, internally consistent description of an installation that is safe for the execution layer to consume.
_Avoid_: Wizard state, answers

**Answer value**:
The stable machine-readable value selected for a wizard step, distinct from the label presented to a person.
_Avoid_: Display label

**Validated value**:
A domain-specific value whose constructor has established the invariants required by execution, such as a safe username, a timezone, or a device path below `/dev`.
_Avoid_: Raw answer, string

**Host profile**:
A validated value naming what the installer is running on — the instantOS live ISO, a running Arch Linux or instantOS system, or an unsupported distribution. It decides what the installer may write: only a live ISO has a throwaway `/etc`, so only a live ISO may reconfigure the running system's own pacman and installer state.
_Avoid_: Wizard state, live session flag

**Target relation**:
A validated value naming how the install target relates to the running system — a different disk, the disk the system booted from, or the running root device. It decides what the installer may do to the target without endangering the system it is executing from.
_Avoid_: Wizard state, disk answer
