use clap::ValueEnum;

/// Spoken language for WhisperX transcription and forced alignment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum TranscriptLanguage {
    #[default]
    En,
    De,
}

impl TranscriptLanguage {
    pub const fn whisper_code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::De => "de",
        }
    }

    pub const fn align_model(self) -> &'static str {
        match self {
            Self::En => "WAV2VEC2_ASR_LARGE_LV60K_960H",
            Self::De => "VOXPOPULI_ASR_BASE_10K_DE",
        }
    }

    /// Cache file under the video hash directory. English keeps the legacy `{hash}.json` name.
    pub fn transcript_json_filename(self, video_hash: &str) -> String {
        match self {
            Self::En => format!("{video_hash}.json"),
            Self::De => format!("{video_hash}_de.json"),
        }
    }
}
