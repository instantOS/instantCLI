I want to introduce a concept called dependencies to the `ins game` command.
These are read-only files which are needed to run the game. These might be
provided for by the user already, so it's not necessary to use `ins game` for
them, but they can be very useful. Keep this in mind. 

Dependencies are read-only, they don't change, so for each game dependency there
only needs to be a single snapshot in restic. Game dependencies should be stored
in the same restic repo, but tagged differently so that they can be
distinguished from game saves. 


