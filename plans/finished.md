I want to change up the CLI of the `ins arch` command. Right now `ins install`
asks all questions and then exits, which is kind of a misnomer, as `ins ask` is
also an existing command. Change it so that `ins ask` by default asks all
questions (same behavior as `ins install` has currently) and just like current
`ins install` serializes them to toml. If `ins ask` is supplied an argument,
then it should behave as the current ask command, asking the supplied question. 


Also add an "installation finished" menu to `ins arch`. It should be reachable
via `ins arch finished`. It should display the option to reboot, shutdown,
continue in the live session, with nerd font icons and menu utils. 

It should also display an installation summary, detailling how long the
installation took, and maybe how much storage is used by the new installation. 

This might require changes to the installation state tracking to contain the
start time of the installation. 

`ins arch install` should be changed to first run `ins arch ask`, then `ins arch
exec` and finally `ins arch finished`, to guide the user through an arch linux
installation start to finish
