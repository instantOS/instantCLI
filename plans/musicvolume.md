
Background music should not be in its original volume, since dialogue should be
hearable over it. Make the volume of the background music 20% of the original.
Make sure not to use a magic number, but put a config file in

`~/.config/instant/video.toml`

Look at how game.toml and instant.toml are done for reference. For now the
config file just contains the music volume. The config file should not be read
when the subcommand is not `video`.


