`ins dot fetch` and `ins dot add` are very similar commands and should be
merged into just `ins dot add`. Here is how it should behave:

If I run `ins dot add file.txt` it should look if this file has a source file.
If it has, then override the source file with the potentially modified file.
This should show if anything changed. 

If I run it on a file that is not yet tracked, it should behave like the old
`add` command, prompting the user for a repo/dotflie dir to add to. 

If I run it on a director, it should behave like the old `fetch` command,
skipping all files which are not already tracked. 

If I want to recursively add files all files in a directory, I should use `ins
dot add --all <dir>`. This should recursively add all files in the directory,
even untracked ones. If I run add on a directory without --all, it should output
an info message telling me that untracked files have been ignored and I can use --all to add untracked files. 

This is a breaking change, do not keep around any legacy code or backwards compatibility. 

Good DX and maintainablility as well as readability trump everything else. 

