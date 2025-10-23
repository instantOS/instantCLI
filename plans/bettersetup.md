# Better setup menu

I want to improve the `ins game setup` menu to also account for new features and
behaviors like the dependencies feature and single file features, and make it
more streamlined and user friendly as well. 

This is a completely interactive command, so no json support will be needed.
This is a breaking change, prioritize DX and usability over backwards compatibility. If some code can be simplified or is redundant after making the changes, remove it. 

I want the following flow:

1. I can choose a game

There is a special order I want: 

1. Games in the games toml, but without a save entry in installations toml
2. Games in the games toml but with dependencies which are not installed (check
   if there is a system for keepnig track of installed dependencies, if there
isn't, make one)
3. Games which have snapshots but which are not in the games toml

The preview window should show what about a game is not set up yet, this could
be an entry in the games toml file, no save entry in the installations toml, or
dependencies which are not installed yet (it could be one, none, multiple or all
of these). Also display other info about the game if present. 

After choosing a game:
If the game has no entry in the games toml, prompt to create one.
If there is an entry in the game toml, skip the above step. 

Collect a list of items about the game which is not set up yet, this could be
either the save directory or dependencies. If there is only one item in this
list, then immediately prompt for that, otherwise allow choosing. 

For setting up the save file, Find out if the game uses a single
file as a save or a directory with multiple files inspect a snapshot for that,
if the latest snapshot for the game contains more than a single file, then it's directory
mode. 
Prompt for the location just as other commands already do. 

For setting up a depencency, try to found out if it is single/milti file and
then prompt accordingly and set it up (utils are already partially present)


Make sure the logic for the single file modes si stable and reliable and
readable and account for edge cases, it is quite new and maybe has issues. 

Make sure the functions are not too long, break them up if necessary, you are
allowed to create new files and modules. 

Be creative, reflect what is useful, intuitive and maintainable. 

