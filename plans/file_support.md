
ins game should support a single file as a save source (in addition to supporting
treating an entire directory as a save source).

The reason for this is that some game saves might only consist of a single file, so
I might want to store them alongside other single-file game saves. The current setup
would associate all these saves for different games with just a single game. 

In addition, dependencies should have the same feature. A dependency might
consist of a single file and be stored in a directory which contains other
files. If I upload the dependency, it should not also upload whatever else is in
the same directory. 

This is not trivial to implement, look up online how restic handles this. 
Restoring a single file should not override other things. This requires
extensive testing, extend the end-to-end tests to cover this and run them. 

Also make sure the data structures and toml files are clear on how this is
implemented. Backwards compatibility with existing saves should be maintained,
but this is not important for dependencies. Good DX and maintainability is very
important, do not add anything emulating older internal methods, the only thing
which matters is the CLI and its outward available features

