---
date: '2025-10-05T14:56:07+02:00'
draft: true
title: 'Videomd'
showToc: true
---

# Video.md

implement this as the `ins video` command. Be creative with the structure

## Basic idea

I have an idea for a video editing tool based on markdown. 
It works the following way:

Transcribe the video using AI into a timestamped subtitle file. 
Convert that subtitle file into a markdown file, preserving the timestamps in a
format that is both human and machine readable. 

Now I can read the entire video in text form. 

After making edits to the markdown file, the program can edit the video itself
according to the edits I did to the markdown file. 

The most basic of these is cutting out a section of the video. 
If I remove an entire line from the markdown file, the program can recognize
which timestamps to cut out of the video. 

## Please build this

Personally I am very much sold on the idea. 

## Implementation

### Tech stack

Moviepy needs insane amounts of memory and is very slow, and necessitates
reencoding. I am leaning towards writing this in rust and using the ffmpeg-next
wrapper. 

Open issues:
- Which markdown parser?
- How exactly does ffmpeg work? Does it support all I want?

### Transcribing

I am leaning towards WhisperX and using uvx to simplify the insane installation
process. Calculate the hash of the video before transcribing, then transcribe
it. Save the transcription in a cache directory, along with the hash, so that
when dealing with the same video twice, I can easily skip transcribing it and
use the cached transcription. This can be a fast insecure hash, the only thing I
do not want are accidental collisions. This probably outputs SRT files. 

### Converting

Some yaml preamble with metadata (video hash, file name, file path)
Then

```
`<timestamp>-<timestamp>` text
```

Silences should also be inserted as
```
`<timestamp>-<timestamp>` SILENCE
```

Since maybe you would want to cut these out or keep parts of the video where
there is no talking. Only silences longer than a second should get a SILCENCE
line, as shorter silences should just be left in. If a silence is longer than 5
seconds it should be split into multiple SILENCE lines, that way I can remove
part of the silence while keeping the rest.

### Extra instructions

These should only be developed once the basics are in place, but be mindful that
these will be added later when developing the basics. 

Cutting out parts of the video is only the tip of the iceberg. I could parse
other markdown features to other video edits. I would like to stay as close to
normal markdown as possible, so no extra syntax unless necessary. 

The obvious one is to convert headings to text cards. 
```
# Heading
```
Will be an image with the text "Heading" on it, inserted into the video.
Different headings will be different images.
Instead of images I could also do videos, using blender to parameterize motion
graphics. But for the first version I would like to keep it simple, so images it
is. 

Images could be inserted as showing literal images. 
```
![Image](image.png)
```
Will show that image in the video.

Literal text can be shown as images of the text. 
Markdown formatting like bold, italics, quotes etc. will be preserved.

Codeblocks can be used for custom instructions like background music. 
The language of the codeblock will be used to determine the type of instruction.

````
```music
music:mymusic.mp3
```
````

or even
````
```music
https://link.to/music.mp3
```
````

For caching of remote docs, the link could be hashed, and the music or image
downloaded to a cache with the hash as the filename.


These might be difficult to implement with ffmpeg, but would be nice at some
point. 

````
```transition
wipe
```
````

### Exporting to markdown

I could also export the video markdown file to a completely human readable text
format. Images could be inserted as literal images, instructions like music
edited out, remote videos embedded or linked to. 

### IDE

I plan to develop this only once the basics are in place.

I am a big fan of luasnips, so crating snippets for the extra instructions will
speed up things massively. Refer to my neovim setup for how to do this, the
latex snippets are a lifesaver for creating anki cards and taking notes, I would
like the same for the extra instructions here. 

For inserting other video clips as b-roll, you might want to get specific
timestamps interactively. The way I plan to do this is open the videos in mpv
and use custom lua scripting to copy special video instructions to the clipboard.
For example, you could set loop markers in mpv, then press a hotkey to copy an
instruction to insert the marked section of the video into the `video.md` video

