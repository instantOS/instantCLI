# Issues

## Running the cli for testing purposes

`cargo run` does not work outside of the working directory of this project
Testing might require cd ing into different directories, so you cannot just use
cargo run without further configuration or args, it needs to know which project
to run. 


## Immediately Outdated

after I run `instant dot add file.txt`
and then `instant dot status`, the file immediately shows as outdated
The file has the same content in the source and the target, because the source
was just created from the target. Some logic here is odd. 

