# Ins game dependencies plan

I want to introduce a concept called dependencies to the `ins game` command.
These are read-only files which are needed to run the game. These might be
provided for by the user already, so it's not necessary to use `ins game` for
them, but they can be very useful. Keep this in mind. 

Dependencies are read-only, they don't change, so for each game dependency there
only needs to be a single snapshot in restic. Game dependencies should be stored
in the same restic repo, but tagged differently so that they can be
distinguished from game saves. Each dependency has an ID, same as with the save
directories. The games toml should declare which dependencies a game has. 
The installations toml should declare which dependencies are installed where
already. 
Since the dependencies do not change, all the sync logic does not apply to them. 

It does not need to be tracked when they were last modified, or when the last
dependency snaphot was taken. 

A dependency can be either a directory or a file. 

The CLI should be 

`ins game deps`

First command
```
ins game deps add [gamename] [dependencyid] [path]
```

If any of these arguments are missing, they should be asked for interactively
just like with `ins game restore` and `ins game add` and `ins game setup`. 

```
ins game deps install [gamename] [dependencyid] [path]
ins game deps uninstall [gamename] [dependencyid]
ins game deps list [gamename]
```

With `install`, if the path is not given, give the choice between the path
inside the dependency snapshot or let the user choose a path (just like with
`inst game setup`, and just like with `setup`, warn when the user chooses an
install path which is not empty.)

These are just suggestions on how the CLI should work. Again, if any arguments
are omited, they should be asked for interactively, look at the other parts of
the codebase for reference. 


# Coding instructions

Ensure code is not duplicated between the different `ins game` commands, they
share a lot of logic and utils, so the logic should be extracted into general
functions where appropriate. Make sure files do not grow too long and the file
structure is easy to navigate. Refactoring encouraged when files grow too long. 


