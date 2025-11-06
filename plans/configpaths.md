I want to move some of the data paths. `~/.local/share/instantos` should become `~/.local/share/instant`

That way things are more unified, maybe more code can be shared. 

Also the `ins dot` command should not be configured in `instant.toml` instead it
should be `dots.toml`, meaning the global config file is not full of data only
related to the dots command. 

This is a breaking change, do not add any compatibility or legacy keeping code.
Internal and external APIs can be changed. 


