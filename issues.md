# Issues


## Immediately Outdated

after I run `instant dot add file.txt`
and then `instant dot status`, the file immediately shows as outdated
The file has the same content in the source and the target, because the source
was just created from the target. Some logic here is odd. 

## Chaotic directory management

This uses self-made and duplicated logic for resolving the home directory or xdg config path.
Tests also use their own weird logic. use the `dirs` crate instead and centralize this more. 

## E2E Tests touch instant.toml

Allow specifying which config file to use with the CLI (defaulting instant.toml
in the config dir)
The E2E tests should use their own config file, specified via the arg

## E2E test cleanup

E2E tests should clean up after themselves, meaning repos they created and
cloned should be removed
