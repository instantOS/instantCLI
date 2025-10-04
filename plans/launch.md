I want to properly implement the launch command.

# Ability to override launch command in installations toml

I want to change installations toml to optionally include a launch command for a
game. This is because different games may need different command line arguments
or have entirely different launch commands on different machines. When a launch
command is only present in the game toml, then use that, if both game toml and
installations toml have a launch command, then use the installations toml. 

# General behavior

When I execute `ins game launch` I want it to show me a list of all games which
have configured launch commands (in games toml or installations toml or both).
Then I want it to sync the games just like if I did `ins game sync` (including
all games, reuse the exact same behavior to keep things simple and
deduplicated). 

It then executes the launch command and waits until the game is closed. 
After the launch command terminates, wait a few seconds and then execute sync again as the game might have
overridden the save data. 

# Coding instructions

Good and clean DX is very important. You are allowed to change the architecture
to suit the features of the program best, legacy cruft and backwards
compatibility burden is discouraged. 
