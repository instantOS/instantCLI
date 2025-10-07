# ins game restoers for wine

Some game saves are made from wine prefixes. Wine prefixes are a path which
contains all data wine needs to simulate a windows installation.

an exampl would be the following path for a game save snapshot. The game in this
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

