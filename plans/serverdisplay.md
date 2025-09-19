# Pretty server TUI

The menu server just shows its log messages while no menu is running. 
Change this to be a pretty TUI which just shows 'waiting for menu requests' in
the center of the screen. Use the ratatui crate for this. Look things up if you
need to. 

Also add a way to have the server run without a scratchpad. This might be
because people would want it in a persistant window. It should be a command line
flag, and the server should then simply ignore anything having to do with the
scratchpad. It should not show a scratchpad when starting a menu and it should
not close one when the menu is done. It should not check for visibility and
should not close the menu when it is not visible. THIS IS ONLY IN THE
--no--scratchpad mode, when the flag is set. 
