# Bug

Situation:
I clone a dotfile repo
The dotfiles are already present in my home directory, in the state the dotfile
repo expects. This means they should register as unmodified, since their content
is the same as in the dotfile repo. 
In this example, tester.txt etc already exist in home, and they also exist in
the dotfile repo, and they have the same content there. 
Yet they show as modified. Create a test in tests/scripts (using bash, look at
existing tests and how to add new ones) which tests this scenario (with made up
application names) and then try fixing the bug in the code

```
instant dot clone git@github.com:instantos/dotfiles.git --branch instantcli
⠚ Cloned git@github.com:instantos/dotfiles.git                                                                                                                                                                      Added repo git@github.com:instantos/dotfiles.git -> /home/benjamin/.local/share/instantos/dots/dotfiles
~ took 2s  instant dot status                                                         
git@github.com:instantos/dotfiles.git -> clean
    /home/benjamin/.config/kitty/kitty.conf -> modified
    /home/benjamin/.config/kitty/current-theme.conf -> modified
    /home/benjamin/tester.txt -> modified
~ 
```
