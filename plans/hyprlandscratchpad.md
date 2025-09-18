This is a basic idea for how the scratchpad might work on Hyprland.
Look at how the scratchpad works on Sway and at the basic bash implementation
and add scratchpad support to Hyprland. 

```sh
#!/bin/bash
WSNAME="instantscratchpad"

create_scratchpad() {
    hyprctl keyword windowrulev2 "workspace special:$WSNAME,class:^(instantmenu)$"
    hyprctl keyword windowrulev2 "center,class:^(instantmenu)$"
    hyprctl dispatch exec -- kitty --class=instantmenu -e /bin/sh
}

CLIENTS="$(hyprctl clients)"
if ! [[ $CLIENTS =~ "instantmenu" ]]; then
    create_scratchpad
else
    # show the workspace
    hyprctl dispatch togglespecialworkspace "$WSNAME"
fi
```
