# Plans for settings


## Systems

### Settings invoking external commands

Some settings do not hold values, they just open external programs which handle
the actual settings. In this case, the `ins` cli does not need to care about the value
or state of the actual setting, it just needs to open the program in the
terminal the settings are currently running in. 
Adjust the architecture to make this kind of settings entry work, and implement
the wiremix setting mentioned at the bottom of this file. 
TUI commands should open in the same terminal as the settings are running in,
GUI commands should open non-blocking. Make sure the DX for adding these kinds
of entries is ergonomic. 

### Required Packages

Some settings may require specific programs to be installed, but I do not want
to have each of them as a hard dependency. There should be a RequiredPackage
struct, and a setting can provide an optional list of these for packages which
are required for the settings. Other parts of the program (not just settings) might also benefit from this system, so make it generic and in a utility module

```
RequiredPackage {
    name: String,
    arch_package_name: String,
    ubuntu_package_name: String,
    // if any of these completes, the package is considered installed
    installedTests: vec<InstallTest>
}

impl RequiredPackage {
    fn ensure() -> bool {
        // package managers tend to be slow, so only fall back to check that if the other tests fail
        // prompt for installation using the appropriate package name
        // use the fzf wrapper to prompt for installation, if yes, actually
call the package manager to install the package
        ...
    }
}

enum InstallTest {
    WhichSucceeds(String),
    FileExists(String),
    CommandSucceeds(String),
}

impl InstallTest {
    fn run(&self) -> bool {
        ...
    }
}

```


## Settings Entries

audio settings should require and open this
https://github.com/tsowell/wiremix
(it is a TUI, so it can just be opened in the terminal the settings are running
in)

## Remove Entries

This will list redundant entries which I do not like
