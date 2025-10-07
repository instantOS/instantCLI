Refactor the video renderer according to this data structure


```
TimeLine {
    segments: vec<VideoSegment>
}

VideoSegment {
    start_time
    duration
    data: VideoSegmentData 
    transform: Option<Transform>
}


enum VideoSegmentData {
    VideoSubset(start_time, source_video)
    Image(sourceImage)
}


struct Transform {
    scale: Option<f32>
    rotate: Option<f32>
    translate: Option<(f32, f32)>
}

```

Inspiration here
https://stackoverflow.com/questions/36936354/is-it-possible-to-create-a-timeline-using-ffmpeg
