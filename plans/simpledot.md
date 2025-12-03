Allow supporting dotfile repos without metadata in `ins dot`. The repo root is
used as the dotfile directory, readme.md is ignored, and warnings are dislpayed
if the repo is missing common files like `.config/` or `.bashrc`.

This will make `ins dot` compatible with a lot of existing dotfile repos that do
not contain `ins dot` specific metadata. 
