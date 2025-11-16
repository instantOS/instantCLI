
I want to add an `ins self-update` command. Use the scripts/install.sh script as
reference for this. I want this to check if the current version is installed
inside a user writeable directory or /usr/local, when in `~/.cargo/bin` or
/usr/bin then it should print a message saying that the package manager should
instead be used to update the package. If the current version is installed in a
user writeable directory or /usr/local then it should download check the latest
release version on github, and if it's newer than what is currently installed,
download and replace the current binary with the new version. Ask for sudo
(check other places in this repo for how privilege escalation is handled in this
project) only if needed. Don't forget the file should be executable. Do not test
the command yet, just regularly run `cargo check` and `cargo build` to make sure
your changes are valid rust. 
