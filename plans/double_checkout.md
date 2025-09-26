
Add a field to the Installation struct with a checkpoint ID This should be the
checkpoint which is nearest to the current save state in the save folder for the
game. This means, if saves are restored from a checkpoint, then the installation
toml should be updated with the ID of that checkpoint. If a checkpoint is
created from the save data, then the installation toml should be updated with
the ID of the newly created checkpoint. 

When restoring a save from a restic checkpoint, first check if the checkpoint
being used is the same as the one in the installation toml. If it is, then skip
the actual restore, as likely nothing would change. 

Also keep in mind the output of the `instant game restore` and `instant game
sync` commands, which should reflect a skipped restore in their output. Also add a --force flag
which restores from the checkpoint even if the installation toml claims it is
the nearest checkpoint. 

The field in the toml (for each installation) should be named `nearest_checkpoint` and it should be a string.

