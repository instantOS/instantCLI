I identified a menu server issue on sway. If I open the menu using `instant
launch` and then while the launcher is open in the server I manually hide the
scratchpad, and then try to open the server again, it crashes. 

Ideas for debugging

Add logging to the menu server. If the --debug option is set, then spwan the
inside server in the terminal with a bash command which logs its output to a
file. 

