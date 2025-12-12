A new check for `ins doctor`:

Skip if no desktop environment (X11 or Wayland compositor) is detected to be
running. Web search what the best practices for detecting this are in 2025. 

If a desktop environment is detected to be running, detect if there is a working
polkit agent

Fix: install and enable a polkit agent (maybe systemd-user service, look into
what best practices for this are)
