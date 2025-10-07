# New ins video subcommand

Add the `ins video titlecard` subcommand to the video CLI tool. This is meant to
manually use or test out the generation of titlecards for videos. It should use
the same logic as the video render command and not duplicate any code. 

Here's an idea for how the CLI should work:

```
ins video titlecard <markdownfile>
```

by default is should render to `markdownfilename.jpg`
with an optional `-o outputfile` argument. 
