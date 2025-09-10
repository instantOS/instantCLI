# Technical debt

Search this repository for technical debt and come up with a plan to fix it.
Document that plan in issues.md
Do not execute the plan or edit code yet. Just come up with a report and a plan. 

# Reset command issues

Make sure that the reset command prints all files which it reset. 
In case there are no files to reset, print a message that no files were reset. 

# status shows 'no dotfiles found'

```
$ instant dot status
~/.config/kitty/kitty.conf -> clean (dotfiles)
~/.config/alacritty/alacritty.toml -> clean (dotfiles)
~/tester.txt -> clean (dotfiles)
~/.config/kitty/current-theme.conf -> clean (dotfiles)
No dotfiles found.
```

this clearly shows there are dotfiles, there might be something wrong with the
logic of this command. 
