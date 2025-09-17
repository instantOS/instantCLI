# Sway

This approach works for spawning a terminal and toggling its visibility on sway
Note that it's pretty basic and sleep 0.3 is a hack. Also it's in bash

```sh

#!/bin/bash

# Check if scratchpad terminal exists
if swaymsg -t get_tree | grep -q '"app_id": "scratchpad_term"'; then
    # Terminal exists, toggle its visibility
    swaymsg '[app_id="scratchpad_term"] scratchpad show'
else
    # Terminal doesn't exist, create and configure it
    # Launch the terminal
    alacritty --class scratchpad_term &
    
    # Wait a moment for the window to appear
    sleep 0.3
    
    # Configure the new window
    swaymsg '[app_id="scratchpad_term"] floating enable'
    swaymsg '[app_id="scratchpad_term"] resize set width 50 ppt height 60 ppt'
    swaymsg '[app_id="scratchpad_term"] move position center'
    swaymsg '[app_id="scratchpad_term"] move to scratchpad'
    
    # Show it immediately (optional)
    swaymsg '[app_id="scratchpad_term"] scratchpad show'
fi

```

# Hyprland

TODO, but I'm pretty sure this should use special workspaces

# i3

Probably similar to sway

# Others

Not sure if possible, maybe spawn and kill terminals as needed


