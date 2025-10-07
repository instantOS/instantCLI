Refactor the video renderer to be more like a non linear editor according to this data structure
This needs a lot more research and thinking, but these are the basics


```
TimeLine {
    segments: vec<Segment>
}

Segment {
    start_time
    duration
    data: SegmentData 
}


enum SegmentData {
    VideoSubset(start_time, source_video, transform)
    Image(sourceImage, transform)
    Music(audiosource)
}


struct Transform {
    scale: Option<f32>
    rotate: Option<f32>
    translate: Option<(f32, f32)>
}

```

Inspiration here
https://stackoverflow.com/questions/36936354/is-it-possible-to-create-a-timeline-using-ffmpeg
```sh
ffmpeg -i file1.mp4 -i file2.mp4 -i file3.mp4 -loop 1 -t 20 -i logo.png \
-filter_complex "[0:v]trim=120:125,setpts=PTS-STARTPTS[v1];
        [1:v]trim=duration=90,setpts=PTS-STARTPTS[vt2];
        [vt2][3:v]overlay=eof_action=pass[v2];
        [2:v]drawtext=enable='between(t,10,30)':fontfile=font.ttf:text='Hello World'[v3];
        [0:a]atrim=120:125,asetpts=PTS-STARTPTS[a1];
        [1:a]trim=duration=90,setpts=PTS-STARTPTS[a2];
   [v1][a1][v2][a2][v3][2:a]concat=n=3:v=1:a=1[v][a]" -map "[v]" -map "[a]" output.mp4
```
