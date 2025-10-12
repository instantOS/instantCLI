# New `ins game edit` command

Introduce a new `ins game edit` command. This is an interactive menu which
allows editing a game's data across games.toml and installations.toml. It should
be done in a way similar to the `ins settings` command, but less sophisticated
and complex. You can however use some of the same utilities, or extract
utilities from `ins settings` in order to not duplicate code. 

The command should offer a menu (using the menu_wrapper in the codebase) which
allows choosing a property and then being able to edit the property. For paths,
look at how paths are chosen in `ins game setup` for example. 

If no name is specified, then a list of games should be shown to choose from.
This is also implemented already in other places, do not duplicate code.

There should be a save and exit option as well as a launch option in the menu
for games. 

Pay attention that installation command can be overridden in installations.toml,
come up with an untuitive way in which the menu should behave. 


