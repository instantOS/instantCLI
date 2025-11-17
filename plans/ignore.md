add an `ins dot ignore` command. The rationale behind this is the following:
If I want to not have certain dotfiles on my local machine, but they are present
in a dotfile repo, then `ins dot apply` will recreate them every time I delete
them. To avoid this, machines should keep a local list of files or directories
to ignore, and when applying dotfiles, these repos and files are skipped. Figure
out an ergonimoc way to implement this, read the code deeply first to make sure
you're not disrupting anything, reinventing the wheel or producing bad DX.
Refactors allowed. 



