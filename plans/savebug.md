
I found a bug with single file saves

```
$ ins game setup
ℹ Adding 'Metroid Fusion' to games.toml...
✓ Added 'Metroid Fusion' to games.toml
ℹ Setting up game: Metroid Fusion

Found 1 unique save path(s) from different devices/snapshots:

 Select the save path for 'Metroid Fusion':
 Could not infer save type from snapshot: Failed to inspect snapshot contents
✓ Created save directory: ~/Games/metroid_fusion/Metroid Fusion (Europe) (En,Fr,De,Es,It).sav
 Restoring latest backup (d1a69066c955d990008b1690f7da8364a331efed56e5794521b148f9e0d39f3c) into ~/Games/metroid_fusion/Metroid Fusion (Europe) (En,Fr,De,Es,It).sav...
Error handling game command: Failed to restore latest backup
  Caused by: Failed to restore restic snapshot
  Caused by: Restic command failed: Exit code 1: {"message_type":"exit_error","code":1,"message":"path home/benjamin/Games/metroid_fusion/Metroid Fusion (Europe) (En,Fr,De,Es,It).sav: not a directory"}

Error: Failed to restore latest backup

Caused by:
    0: Failed to restore restic snapshot
    1: Restic command failed: Exit code 1: {"message_type":"exit_error","code":1,"message":"path home/benjamin/Games/metroid_fusion/Metroid Fusion (Europe) (En,Fr,De,Es,It).sav: not a directory"}

```

This save is only a single file, but the setup process creates a directory for
it. 

```
$ file Games/metroid_fusion/Metroid\ Fusion\ \(Europe\)\ \(En,Fr,De,Es,It\).sav
Games/metroid_fusion/Metroid Fusion (Europe) (En,Fr,De,Es,It).sav: directory
```

check the logic, find the error and fix it. 
Also check that the prompts say the correct thing depending on if we're working
with a single file save or a directory save.


