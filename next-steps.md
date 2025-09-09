# Next Steps

## Fix Path Resolution for the CLI

The handling of dotfile cli arguments should be similar to how git does it. 
If I am in /home/benjamin/.config/kitty and I run instant dot add kitty.conf, it
should be treated as if I ran instant dot add `~/.config/kitty/kitty.conf`.
Internally, work with absolute paths. Accept absolute paths, paths relative to
the current working directory, paths with `~/...`
All dotfiles should be in the home directory. Other files like
/mnt/mydrive/myfile.txt should not be accepted, so args to invalid files should
be rejected. 

