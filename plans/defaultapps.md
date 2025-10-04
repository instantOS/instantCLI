# Set default app using settings menu

I want to add an entry to the settings menu which allows me to set default
applications for different mime types. It lists all mime types and when I choose
one it lists all applications for that mime type. When I choose an application
it should be set as default for that mime type.

The state of this is not tracked by the settings system toml, the single source of
truth is the actual mimeinfo files. 

If needed, adjust the architecture for that. 

The implementation should be basic for now. 
You likely do not have all the knowledge required to implement this, so do some
of your own research into xdg, gio and if there are rust crates for this. 

As reference here's a bash implementation. Use this as inspiration. 

```sh
#!/bin/bash

# Get unique MIME types from mimeinfo.cache files
mime_types=$(
    grep -h -o '^[^=]*' \
    /usr/share/applications/mimeinfo.cache \
    ~/.local/share/applications/mimeinfo.cache \
    2>/dev/null | sort -u
)

# Select MIME type with fzf, preview shows current default
selected_mime=$(echo "$mime_types" | fzf --prompt="Select MIME type: " --preview="echo Current default: \$(xdg-mime query default {})")

if [ -z "$selected_mime" ]; then
    echo "No MIME type selected. Exiting."
    exit 0
fi

# Get list of available applications for the selected MIME type from mimeinfo.cache
apps=$(
    grep -h "^$selected_mime=" \
    /usr/share/applications/mimeinfo.cache \
    ~/.local/share/applications/mimeinfo.cache \
    2>/dev/null | sed 's/^[^=]*=//' | tr ';' '\n' | sed '/^$/d' | sort -u
)

if [ -z "$apps" ]; then
    echo "No applications found for $selected_mime."
    exit 1
fi

# Select application with fzf (apps include .desktop)
selected_app=$(echo "$apps" | fzf --prompt="Select application for $selected_mime: ")

if [ -z "$selected_app" ]; then
    echo "No application selected. Exiting."
    exit 0
fi

# Set the default application
xdg-mime default "$selected_app" "$selected_mime"

echo "Set $selected_app as default for $selected_mime."
```
