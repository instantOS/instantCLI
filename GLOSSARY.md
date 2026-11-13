# Installer

The installer builds an Arch-based system on a target disk or configures an existing system as instantOS.

## Language

**Arch Linux variant**:
A supported combination of base distribution and init system: Arch Linux with systemd, or Artix Linux with OpenRC, runit, s6, or dinit. It is independent of CPU architecture and the choice of the instantOS or minimal installation profile.
_Avoid_: Platform

**Installation source**:
The live installation medium or running system from which an installation is started.
_Avoid_: Target, installed system

**Target system**:
The system being installed on the selected disk. After entering its chroot, the target becomes the installer's current filesystem root.
_Avoid_: Host

**Installation profile**:
The choice between the full instantOS system and a minimal system based on the selected Arch Linux variant.
_Avoid_: Distribution, variant

**Init service package**:
An Artix package that supplies a daemon's integration with a particular init system. It is separate from the package containing the daemon itself.
_Avoid_: Daemon package, init package

**Package source**:
The repository configuration and mirrors used to obtain packages during bootstrap, either from the installation source or from an isolated installer configuration.
_Avoid_: Installation source

**Arch repository support**:
The compatibility packages, signing keys, and additional Arch repositories needed to use Arch packages on Artix.
_Avoid_: Bridge
