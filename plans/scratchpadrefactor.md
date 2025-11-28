A lot of the scratchpad code is violating SOLID principles, come up with a
better architecture, breaking refactorings allowed. 

Ideas to get started: A ScratchpadProvider trait, which the different compositors can
implement. 

That way I cannot accidentally implement partial support. 

Also keep in mind that a fallback for desktop environments which do not support
scratchpads should be usable by commands like `ins assist` and `ins launch` and
possibly more. This should work by spawning a terminal with the `ins launch`
menu within it, which then also closes when the menu within it quits. 

Keep in mind how state is managed, I do not want to query the compositor/window
-anager more than absolutely necessary because this is expensive IO
