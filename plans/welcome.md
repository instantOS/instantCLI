Add an `ins welcome` command, which is intended as a welcome app when installing
the linux distribution instantOS. 

It should use the same styling as the `ins settings` command, meaning icons and
minimal fzf appearance with colors and previews etc. 

You might have to exctract some of the code from `ins settings` to be more
general, the style of `ins settings` looks really nice and will probably be
reused in lots of places. 

It should also have a 'close' option at the bottom to exit the welcome app.

It should also have a `--gui` option like the settings. 

`ins autostart` should start this app with the gui option if the setting to do
so is enabled. The welcome app should have an entry to disable the autostart
setting, and `ins settings` should contain a setting to enable or disable the
welcome app under the 'System' category. 

The welcome app for now (apart from the disabling option) should contain an
entry to open the instantOS website instantos.io, an entry to open the settings
(`ins settings --gui`)




