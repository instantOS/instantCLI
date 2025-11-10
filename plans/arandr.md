I want to recreate the 'Windows + P' menu on sway using swaymsg. 

I want to be able to choose between different display modes:
- Mirror displays
- Extend displays
- Specific screen only (allow choosing from connected screens)

The tool should automatically detect connected displays and show only the relevant options.

This should be a separate module, and is accessed through a 'Display' entry in
'ins settings'. See if the existing sway utilities in this project can be used
or extended for this and if they need to be moved. You are allowed refactors, do
not keep around legacy compatibility code. 


