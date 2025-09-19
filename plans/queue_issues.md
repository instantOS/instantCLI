The following causes issues:

I send a menu request to the menu server, it shows the menu scratchpad with the
menu in it. I then hide the menu scratchpad, and the menu is still there, just
not visible. The server does not take any new requests until the menu choice is
made or the menu is cancelled, but because the terminal containing the menu is
currently hidden, that is impossible. 

Idea for solution:

Periodically check if the scratchpad is hidden, then kill the menu and report
'cancelled' to the client if the scratchpad is hidden. IMPORTANT: Hyprctl
commands are blocking, so do this only while a menu is active, if there is no
menu active or about to be active, then do not touch hyprctl in the background. 

