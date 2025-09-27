# The problem

I do not trust many cloud save systems. Partially that's not their fault.

I am on Linux, so everything EpicGames is janky (even more than on Windows). 
Steam is pretty good, but even there, the cloud save system occasionally refuses
to sync. 

Regardless, I have spent a fair bit of time in some games, left them sitting for
a year, only to find out that something has swallowed my save file or that it's
on an old machine and has not synced over. I have enough new games that I do not
deem it worth my time to redo the beginning of a game multiple times, if I could
instead be playing something I haven't done already, so unfortunately these
games tend to rot in a pile of shame. 

I also like to play emulated games, or use tools like umu-launcher, so the save
file will not necessarily be in any folder steam expects or knows. 

I have tried Ludusavi, but it's very poorly documented, very slow, and partially
very janky. Apart from custom game paths, it also does really care about
emulated games. I also like to keep games self contained. Separate folder with
dependencies, wine prefix etc. Ludusavi doesn't really like that. 

# The solution

I do not believe I can build a better cloud save system than Steam or EpicGames.
I believe it has already been built, we just need to use it and build a friendly
user interface.

Restic has pretty much everything we need. It can detect changes for large
amounts of binary files, keep a deduplicated version history, upload to slow
cloud storage in an efficient manner, restore from that storage, guarantee
integrity, pull changes from the cloud, manage custom metadata, keep track of
different devices and more. 

# Ideas

Game saves as restic snapshots

`ins game sync <game>` command

For each game installation do the following:

- Check when the game save was last modified locally
- Check when the latest restic snapshot for the game was created
- If the game save is newer, create a restic snapshot for the game
- If the newest restic snapshot is newer, then restore the game save from the
snapshot

`ins game add`

interactively prompt for
- game name
- path


`ins game launch` command

1. sync game saves
2. Launch game using provided launchcommand


`ins game init`

prompt for restic repo to use and its password
test out restic repo afterwards
prompt for creating a new repo if none is found

# Architecture

All game saves are stored in a single restic repository.
Probably doesn't need a password, game saves are not that sensitive. 


## Config Files

### Games

List of games with universal metadata. Can be checked into dofile managers and
synced across devices.
`~/.config/instant/games/games.toml`

### Installations

Device specific metadata. A game can be installed or not, or installed in
different locations on different devices. This contains info about the
installation of the game on this specific device. Do not check into dotfile
managers or sync. 
`~/.config/instant/games/installations.toml`

## Data
```

struct GameName(String)
struct PathId(String)

struct InstantGameConfig {
    repo: ResticRepository,
    repo_password: Option<String>,
    games: vec<Game>
}

struct SavePath {
    id: PathId,
    // example: this is the path where preferences are stored or 
    // example: this is where the game saves are stored
    description: String,
}

// go in games.toml
Game {
    name: GameName, //Unique identifier for a game
    description: Option<String>,
    // A game can have multiple paths which contain save data
    // defaults to a single path named "saves"
    // this is only an ID
    save_paths: Option<vec<SavePath>>,
    launch_command: Option<String>, // command to launch the game, if it can
easily be done from CLI
}

// each game path needs to be mapped to an actual path
// go in installations.toml
GameInstallation {
    game_name: GameName,
    saves: HashMap<PathId, Path>, // make sure tilde and env variables in the
path are expanded when it is saved to the toml file
}
```


A game save is a restic snapshot tagged with `instantgame/<gameid>`


## Libraries

Use rustic_core to interact with restic
Use fzf wrapper already in the project for menus
use fzfselectable trait already in the project


