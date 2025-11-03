add some safeguards so that it's not possible to accidentally delete the home
directory or `~/.config` or something like that. 

`ins game` has the ability to override entire directories with a game save. Make
sure the user cannot accidentally override the entirety of `~/` or `~/.config` or `~/.local`
Make the safeguards generic, they could be extended in the future, and saves and
deps both need them. 

Having the blocked directories as a save path or deps path should be an error.
Be smart with where to block these directories (we can assume that treating the
entire home directory or something like that as a save path is a mistake on the
user's part)

