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
