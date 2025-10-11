the interactive `ins game setup` flow has an issue and should work better.

If I set up a game which has no checkpoints yet, the behavior should be
selecting a path, from which the first checkpoint will be created. Right now,
the prompt warns that the path will be overwritten. This is not correct, and it
also should not happen in case that is correct. If a game has no checkpoints,
but an installation path (or gets set up interactively), then the first
checkpoint should be created from that path. 

Make sure the architecture is clean and exhibits this behavior. 

