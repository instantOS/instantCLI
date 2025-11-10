
I want to be able to use the GUI menu system implemented by the menu server (and
used by the `ins launch` command as a gui application launcher) on 
other desktop environments which might not support or have a notion of a scratchpad. In that
case, I cannot spawn a terminal and reuse it, I need to open a new terminal
(focussed on kitty right now, do not account for others) with the menu command
in it which then also closes after I make a choice or cancel. 

Implement a graceful fallback mechanism for that case. Ensure good DX, the menu
system should still be nice and generic to use. Also add a command line flag to
force that behavior. Keep in mind right now the menu server takes a small while
to start, which is acceptable if we only start it once and reuse the terminal,
but doing that each time we open a terminal isn't nice UX. Maybe we can do it
without the Menu server or find a faster way for the fallback? Research and do
what seems best. 

