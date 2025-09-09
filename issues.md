# Issues

These need investigation, fixing and changing

## Chaotic directory management

This uses self-made and duplicated logic for resolving the home directory or xdg config path.
Tests also use their own weird logic. use the `dirs` crate instead and centralize this more. 
The test environment does not need to fake the home directory.

## E2E Tests touch instant.toml

Allow specifying which config file to use with the CLI (defaulting instant.toml
in the config dir)
The E2E tests should use their own config file, specified via the arg. Users
might also be interested in being able to specify their own config file path. 

## E2E tests touch the database

Same as for the config file, allow specifying this to be different with the CLI (defaulting to what it is now), make the
test command runner use a different database

## E2E test cleanup

E2E tests should clean up after themselves, meaning repos they created and
cloned should be removed. Running the tests twice sometimes makes them fail
because of some leftover files. 

## E2E test dotfile names

The dotfiles the E2E tests create should be in ~/.config/instanttests/`<stuff>`
not `~/thisisatestfile` or similar

## Immediately Outdated

after I run `instant dot add file.txt`
and then `instant dot status`, the file immediately shows as outdated
The file has the same content in the source and the target, because the source
was just created from the target. Some logic here is odd. 
