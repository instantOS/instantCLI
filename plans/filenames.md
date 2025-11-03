# ins game path niceties

## Wine prefixes

Some game saves are made from wine prefixes. Wine prefixes are a path which
contains all data wine needs to simulate a windows installation.

an example would be the following path for a game save snapshot. The game in this
example is Twin Mirror.
```
/media/benjamin/hdddrive/Games/twin_mirror/prefix/drive_c/users/benjamin/AppData/Local/LOA/Saved
```

Relative to the wine prefix, the game save always has the same path on all
machines, but the location of the prefix can be different between machines.

During `ins game setup` when choosing a path where the game saves should be
stored for this game on this specific device, use a snapshot to detect if the
snapshot is of a game save in a wine prefix. If so, offer the user to either
input a path manually, choose one with the chooser, or choose a wine prefix with
the chooser. 
If the user chooses a wine prefix, then set up the game to be stored in the
chosen wine prefix correctly, in the exampe cases this would be 

```
/path/to/wine/on/other/machine/drive_c/users/benjamin/AppData/Local/LOA/Saved
```

Also verify that the wine prefix is valid. It should contain a `drive_c` folder

There is already a utility to choose between interactively browsing a path or
typing one manually. Maybe this can be expanded to add new options? (Keep in
mind emulated or native games do not work using wine prefixes) check and see
what's appropriate. 

## Differently named folders

In some cases, the folder the save is in is pretty long and hard to type. For
example a game migh be saved in 

```
/home/benjamin/Games/SomeGame/superlong_named_folder_name_with_lots_of_characters_and_which_contains_the_save_game/
```


On anoter machine I might want to store the game in

```
/home/otheruser/Games/SomeGame/superlong_named_folder_name_with_lots_of_characters_and_which_contains_the_save_game/
```

(pay attention to the username difference)

It would be nice if I didn't have to type the whole long folder name on the new
machine for the folder path, instead I can select
`/home/otheruser/Games/SomeGame` as the save directory, and then the setup
should detect that `SomeGame` is a different name than the `superlong_named_folder_name_with_lots_of_characters_and_which_contains_the_save_game` name (we ignore that the location of the folder is different, we only care about the folder name itself) and then give a choice between the chosen folder path with the name appended to it (meaning `.../SomeGame/superlong_named_folder_name_with_lots_of_characters_and_which_contains_the_save_game`, or the chosen folder path as is. This sould only pop up if the chosen folder name is different.

The user should be informed that this is just a nicety with a message of "Chosen
directory name (name here) is different than the original save folder name
(other name here). Do you want to use the original folder name appended to the chosen
path, or use the chosen path as is? selected path (path here) 
alternative path (alternative path here)" 

Pay attention to when this data is getting fetched, how it is getting passed and
the DX. Restic operations are expensive, but for snapshots there is a caching
system. Ensure it is being used if not already done. 



