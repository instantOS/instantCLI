
There are dotfile directories which contain multiple files which are highly
dependent on each other, like the neovim config folder. In that case, if a file
is changed, it is not enough to stop updates for that single file. Updating
other files without taking into account the changes in the modified file may
lead to undesired behavior.
For that reason, I want to introduce the concept of "dotfile units"

This is a list of directories in the toml file in a dotfile repo for folders
which should be treated as a single unit. If a single (or multiple) tracked file within that
folder are modified, then so is the entire folder, every tracked file within it
should be treated the same as a mofidied file, meaning it will not get updated. 

There are edge cases however, like the `ins dot merge` or `ins dot add` command,
which should behave cleanly. 

