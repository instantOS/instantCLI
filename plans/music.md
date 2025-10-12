I want to add the ability to add background music to videos rendered with
`ins video render`

For this, there are a few things to consider when mentioning the name of a piece
of background music. 

There are a number of paths which should be searched for the music file.

if I have 

````
```music
mymusic.mp3
```
````

in my file, then it should first search for `./music/mymusic.mp3`, then
`./mymusic.mp3`, then ~/music/mymusic.mp3, then `~/.local/share/instant/music/mymusic.mp3`,
then `~/.cache/instant/music/mymusic.mp3`

(in that order)

The first one of these whiche exists is what the expression resolve to and should be used.
Make sure you use `dirs` for the home dirctory, cache and local dirs. 

If the music file is an URL, then yt-dlp should be used to download it.
It should be downloaded to the cache dir, and the filename should be the hash of
the URL

````
```music
https://lnk.to/mymusic.mp3
```
````


A music statement should set the background music until the next `music`
statement.

````
```music
none
```
````
means no background music. 

You might have to rearchitecture the rendering to get this done (or maybe not,
investigate what is appropriate)
Make sure there is good DX and maintainable data structures. 
