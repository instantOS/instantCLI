
I want an `ins autostart` command which I can put in my autostart scripts (like
sway or hyprland or openbox startup) to set up things related to instantOS. 


There should be an option to disable the startup in a toml config file, if that
is set, then `ins autostart` should do nothing.

It should also check if an autostart instance is already running, and if so, do nothing.

For now it should do the following: Detect the current window manager or
compositor (the scratchpad feature already has utils for this, you might reuse
or move them)

If on sway, then run `ins assist setup`

after that, check if internet access is present, and then run `ins dot update`


