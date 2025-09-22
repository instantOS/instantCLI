The TUI swallows keyboard input. This is not ideal, as the current flow is the
following:

1. Show scratchpad
2. Load data and start menu

This causes the TUI to be briefly visible and swallow the first keystroke if the
menu does not load fast enough. Ideally the input should be captured before the
menu is even there, and then when the menu is there, it should be passed to it. 

Research if this is possible
