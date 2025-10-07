


improvement idea for the `ins video` command: right now, the timestamps are full
precision, but this is not needed. a tenth of a second plus the content of a
line should be enough to accurately map a line of the markdown video to a line
in the subtitle, which then allows retrieving the exact timing of a line, even
without storing it in the markdown document. This means the markdown document is
less verbose and easier to read and edit, while still giving the opportunity to
render the video with full timing precision. 

This will probably require some adjustment of the architecture, since the
markdown file now needs to be used in conjunction with the subtitle file in the
cache to render the final video. It also means that the subtitle cache is not
just a cache anymore. Retranscribing the video multiple times might give
different results, which would break the mapping from video segment in the video
md file and the line in the subtitle file. I propose instead of generating
subtitles in the cache directory, generate them to `./insvideodata`

You should however keep the hash naming scheme exactly the same, this is only to
avoid deleting subtitles when the cache is cleared and so that the needed data
is in the same folder as the markdown document. 

