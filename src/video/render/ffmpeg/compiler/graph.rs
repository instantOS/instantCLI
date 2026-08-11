//! Typed construction of the renderer's FFmpeg filter graph.
//!
//! FFmpeg labels are single-output pads, not variables. Reusing a label as two
//! filter inputs does not mean "read this stream twice" and can produce a graph
//! whose behaviour depends on FFmpeg's negotiation. These move-only pad types
//! encode that rule: a produced pad can be consumed exactly once, while an
//! input stream may deliberately be read by several independent trims. Any
//! intentional fan-out must go through `split_audio`.

#[derive(Debug, Clone, Copy)]
pub(super) struct AudioInput(pub(super) usize);

#[derive(Debug, Clone, Copy)]
pub(super) struct VideoInput(pub(super) usize);

#[derive(Debug)]
pub(super) struct AudioPad(String);

#[derive(Debug)]
pub(super) struct VideoPad(String);

#[derive(Debug, Default)]
pub(super) struct FilterGraph {
    filters: Vec<String>,
}

impl FilterGraph {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn audio_from_input(
        &mut self,
        input: AudioInput,
        filter: impl AsRef<str>,
        output: impl Into<String>,
    ) -> AudioPad {
        let output = output.into();
        self.filters
            .push(format!("[{}:a]{}[{}]", input.0, filter.as_ref(), output));
        AudioPad(output)
    }

    pub(super) fn video_from_input(
        &mut self,
        input: VideoInput,
        filter: impl AsRef<str>,
        output: impl Into<String>,
    ) -> VideoPad {
        let output = output.into();
        self.filters
            .push(format!("[{}:v]{}[{}]", input.0, filter.as_ref(), output));
        VideoPad(output)
    }

    pub(super) fn audio_source(
        &mut self,
        filter: impl AsRef<str>,
        output: impl Into<String>,
    ) -> AudioPad {
        let output = output.into();
        self.filters
            .push(format!("{}[{}]", filter.as_ref(), output));
        AudioPad(output)
    }

    pub(super) fn audio_filter(
        &mut self,
        input: AudioPad,
        filter: impl AsRef<str>,
        output: impl Into<String>,
    ) -> AudioPad {
        let output = output.into();
        self.filters
            .push(format!("[{}]{}[{}]", input.0, filter.as_ref(), output));
        AudioPad(output)
    }

    pub(super) fn video_filter(
        &mut self,
        input: VideoPad,
        filter: impl AsRef<str>,
        output: impl Into<String>,
    ) -> VideoPad {
        let output = output.into();
        self.filters
            .push(format!("[{}]{}[{}]", input.0, filter.as_ref(), output));
        VideoPad(output)
    }

    pub(super) fn concat_av(
        &mut self,
        segments: Vec<(VideoPad, AudioPad)>,
        video_output: impl Into<String>,
        audio_output: impl Into<String>,
    ) -> (VideoPad, AudioPad) {
        let count = segments.len();
        let inputs = segments
            .into_iter()
            .map(|(video, audio)| format!("[{}][{}]", video.0, audio.0))
            .collect::<String>();
        let video_output = video_output.into();
        let audio_output = audio_output.into();
        self.filters.push(format!(
            "{inputs}concat=n={count}:v=1:a=1[{video_output}][{audio_output}]"
        ));
        (VideoPad(video_output), AudioPad(audio_output))
    }

    pub(super) fn overlay(
        &mut self,
        base: VideoPad,
        overlay: VideoPad,
        filter: impl AsRef<str>,
        output: impl Into<String>,
    ) -> VideoPad {
        let output = output.into();
        self.filters.push(format!(
            "[{}][{}]{}[{}]",
            base.0,
            overlay.0,
            filter.as_ref(),
            output
        ));
        VideoPad(output)
    }

    pub(super) fn split_audio(
        &mut self,
        input: AudioPad,
        first_output: impl Into<String>,
        second_output: impl Into<String>,
    ) -> (AudioPad, AudioPad) {
        let first_output = first_output.into();
        let second_output = second_output.into();
        self.filters.push(format!(
            "[{}]asplit=2[{}][{}]",
            input.0, first_output, second_output
        ));
        (AudioPad(first_output), AudioPad(second_output))
    }

    pub(super) fn audio_two_input_filter(
        &mut self,
        first: AudioPad,
        second: AudioPad,
        filter: impl AsRef<str>,
        output: impl Into<String>,
    ) -> AudioPad {
        let output = output.into();
        self.filters.push(format!(
            "[{}][{}]{}[{}]",
            first.0,
            second.0,
            filter.as_ref(),
            output
        ));
        AudioPad(output)
    }

    pub(super) fn mix_audio(
        &mut self,
        inputs: Vec<AudioPad>,
        options: impl AsRef<str>,
        output: impl Into<String>,
    ) -> AudioPad {
        let count = inputs.len();
        let inputs = inputs
            .into_iter()
            .map(|input| format!("[{}]", input.0))
            .collect::<String>();
        let output = output.into();
        self.filters.push(format!(
            "{inputs}amix=inputs={count}:{}[{output}]",
            options.as_ref()
        ));
        AudioPad(output)
    }

    pub(super) fn output_video(&mut self, input: VideoPad) {
        self.filters.push(format!("[{}]copy[outv]", input.0));
    }

    pub(super) fn output_audio(&mut self, input: AudioPad) {
        self.filters.push(format!("[{}]anull[outa]", input.0));
    }

    pub(super) fn finish(self) -> String {
        self.filters.join("; ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_split_is_the_only_audio_fan_out_operation() {
        let mut graph = FilterGraph::new();
        let input = graph.audio_from_input(AudioInput(2), "atrim=start=1:end=2", "voice");
        let (mix, sidechain) = graph.split_audio(input, "voice_mix", "voice_sidechain");
        let music = graph.audio_from_input(AudioInput(3), "volume=0.1", "music");
        let ducked = graph.audio_two_input_filter(
            music,
            sidechain,
            "sidechaincompress=threshold=0.05",
            "ducked",
        );
        let output = graph.mix_audio(vec![mix, ducked], "duration=first", "mixed");
        graph.output_audio(output);

        assert_eq!(
            graph.finish(),
            "[2:a]atrim=start=1:end=2[voice]; [voice]asplit=2[voice_mix][voice_sidechain]; [3:a]volume=0.1[music]; [music][voice_sidechain]sidechaincompress=threshold=0.05[ducked]; [voice_mix][ducked]amix=inputs=2:duration=first[mixed]; [mixed]anull[outa]"
        );
    }
}
