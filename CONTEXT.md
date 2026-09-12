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
A domain-specific value whose constructor has established the invariants required by execution, such as a safe username, timezone, or device path.
_Avoid_: Raw answer, string
