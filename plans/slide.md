
Reproduce the islide utility as a generic TUI in the menu utils. Look at how
keychords are implemented for reference. Keep in mind the menu server must not
capture input while no prompt is active in order to preserve that input for fzf
or other programs which might be spawned later.

Also add an `ins menu slide` command to call the new TUI slide utility, designed
similarly to the original islide cli. 

The islide source code is quite old and long, so here is an overview of what it
does: It provides a slider going from 0 to 100 by default, with other values
being configurable. The user can also provide a command which executes each time
the value of the slider changes, and the command receives the slider value as an
argument. An example for this is changing the system volume or brightness.

The slider can be changed to either n/10 of its max value by pressing the number
n (0-9 on the keyboard) or with vim keys (h/j/k/l) with j and k being bigger jumps
than h and l, by blicking on it with the mouse or by using the arrow keys. 0 is
the max, 1 the minimum, as 0 is on the far right of the keyboard. 

The slider can take up the entire screen in the TUI version as there will not be
anything else running on the terminal while it is active, and that also provides
a bigger click target and more visual clarity. 

Look up how to render that nicely with a TUI. Again, also look at the other
TUIs, particularly key chords, for reference. 

Mouse input is optional for now, only do it if it is easy to implement.


This is not trivial, think hard about making this maintainable and what provides
good DX. 
