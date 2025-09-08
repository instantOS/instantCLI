# Idea

I want to unify lots of instantOS things into an instantCLI
the base command will just be `instant`

# Dotfiles

I wrote about ideas for dotfile management a while back. 
I thought about it some more. I am abandoning the "home dir as git worktree"
approach. It's just not flexible enough.

What follows is incomprehensible pseudocode for the new solution

# Overlaying repos

Dotfiles can come from multiple repos. 
Those repos have an order. If a dotfile exists in multiple repos, the
version from the one with higher priority gets used. 
This allows for easy application of themes. 
It is not specified, which applications a theme can theme, and multiple
themes can be used and removed very easily

Repos are structured like in yadm, the main difference being that dotfiles
are kept in a subdirectory, corresponding to their place in the home directory

```
~/.local/share/instantdots/dots/.bashrc
installs to
~/.bashrc

~/.local/share/instantdots/dots/.config/kitty/kitty.conf
installs to
~/.config/kitty/kitty.conf

```


# Leaving the user alone

instantOS should remain hackable without bothering users who do not want to
learn its tools. This means user configurations should not be overridden, ever.
The tool will keep a list of unmodified hashes for a file, and if a file doesn't
match any of them, it is assumed to be modified by the user or another program
and will be left alone. 

# resetting files

If a modified file is marked as valid, it will automatically be reset.
to the latest official version on the next update.

# applying



```
struct dotfile {
    repo_path // absolute path to source git repo
    target_path
    hash: Option<hash>
    target_hash: Option<hash>
}


impl dotfile {
    fn is_outdated(self) -> bool {
        if not self.target_path.exists()
            return true
        if self.source is newer than the target
            return true
        current_hash = self.get
    }

    fn is_modified(self) -> bool {
        if not self.target_path.exists()
            modified = false
        else if newest_hash is newer than file modification date
            modified = false
        else if self.get_target_hash in get_unmodified_hashes(self.path)
            modified = false
        else
            modified = true
    }

    fn get_target_hash() -> Option<Hash> {
        if !target_path.exists()
            return none
        if hashes.contains(self.path) and self.path is older {
            return newest hashes[self.path]
        } else {
            newhash compute_hash(target_path)
            if hashes.get(newhash) is not None
                return hashes[newhash]
            else
                savehash(newhash)
                return newhash
        }
    }
    
    fn get_source_hash() {
        // similar to target_hash, but hash should be inserted into
unmodified_hashes. Slightly less lazy
    }
    fn apply {
        if self.is_modified()
            return
        if not self.is_outdated()
            return
        cp self.source_file self.target_path
        compute_and_save_target_hash()
    }

    fn fetch {
        if self.modified or self.current_hash != self.targethash
            cp self.target_path self.source_file
    }
}

filemap = HashMap<FilePath, Dotfile>

for repo in repos
    for file in repo.update_file_tree
        //this should override things when same path
        filemap.insert(path, file)

for file on filemap.values
    file.applyfile
    
                

```


# tech

Rust clap CLI
unmodified_hashes in sqlite DB
repo sources in toml config
    commands to add and remove repos
    command to edit the config file
libgit2 or simple git wrapper to clone things
probably anyhow for errors

TODO

look for good sql lib
should support schema updates
look for good terminal output lib

# interface

```
instant dot
    apply
    update
    reset
    clone
    status //which repos are currently applied etc
    init   // initialize new dotfile repo
    fetch  // fetch modified files from disk to repo
           // to allow for yadm style workflow
```

# DB schema

table hashes

```
DATE Created
String Hash
String Path
Bool valid
```

primary key Hash, Path

# dotfile repo

git repo with public source url (should be)
instantdots.toml

```
name = "catppuccin"
author = person_xyz
dots_dir = ./dots
```

# Cleanup

old modified hashes should be cleaned
newest modified hash should be kept
all unmodified hashes should be kept
