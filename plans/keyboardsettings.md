I want to implement a keyboard layout setting into `ins settings`

Setting a keyboard layout is different between different window managers and
desktop environments, so it should behave differently depending on the
environment (there are existing utils for handling different behaviors for
different desktop environments)

The settings persistence should use different setting keys for different desktop
environments so it can restore the appropriate one whenever that desktop
environment is used. 

I initially want to support sway, here is a workin proof of concept script. It
works great. 

I want idiomatic rust, so do not just recreate it line by line. Pay attention to
how the settings storage and restoration is done. 
Also look at how other settings are using the menu utils and fzf wrapper present
in this repository. 

Find an appropriate place to put the setting in the menu as well. 

```bash
#!/bin/bash

# Dependency Check
if ! command -v fzf &> /dev/null; then
    echo "Error: fzf is not installed."
    exit 1
fi

if ! command -v swaymsg &> /dev/null; then
    echo "Error: swaymsg is not installed. Are you running Sway?"
    exit 1
fi

# The path to the xkeyboard-config rules file
RULES_FILE="/usr/share/X11/xkb/rules/evdev.lst"

if [ ! -f "$RULES_FILE" ]; then
    echo "Error: Cannot find xkb rules file at $RULES_FILE"
    exit 1
fi

# 1. Parse the layout list
# We look for the '! layout' section and stop at the '! variant' section.
# We remove lines starting with '!' (headers) to get clean "code description" lines.
LAYOUT_LIST=$(sed -n '/! layout/,/! variant/p' "$RULES_FILE" | grep -v '^!')

# 2. Pipe to fzf for selection
# We use --reverse for top-down list and --header for UX
SELECTED=$(echo "$LAYOUT_LIST" | fzf --reverse --header="Select Keyboard Layout" --prompt="Layout > ")

# 3. Handle exit (if user presses ESC)
if [ -z "$SELECTED" ]; then
    exit 0
fi

# 4. Extract the layout code (first column)
LAYOUT_CODE=$(echo "$SELECTED" | awk '{print $1}')
LAYOUT_NAME=$(echo "$SELECTED" | cut -d ' ' -f 2-)

# 5. Apply the layout using swaymsg
# We target 'type:keyboard' to apply this to all connected keyboards.
swaymsg input "type:keyboard" xkb_layout "$LAYOUT_CODE" > /dev/null

# 6. Optional: Send a notification (requires libnotify)
if command -v notify-send &> /dev/null; then
    notify-send "Keyboard Layout Changed" "Set to: $LAYOUT_NAME ($LAYOUT_CODE)" --icon=input-keyboard
fi
```
