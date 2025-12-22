



```sh
uvx --python 3.10 \
    --from deepfilternet \
    --with "torch<2.1" \
    --with "torchaudio<2.1" \
    deepFilter input.wav
```


Here's the help message

```
uvx --python 3.10 \
    --from deepfilternet \
    --with "torch<2.1" \
    --with "torchaudio<2.1" \
    deepFilter --help   
usage: deepFilter [-h] [--model-base-dir MODEL_BASE_DIR] [--pf] [--output-dir OUTPUT_DIR] [--log-level LOG_LEVEL]
                  [--debug] [--epoch EPOCH] [-v] [--no-delay-compensation] [--atten-lim ATTEN_LIM]
                  [--noisy-dir NOISY_DIR] [--no-suffix] [--no-df-stage]
                  [noisy_audio_files ...]

positional arguments:
  noisy_audio_files     List of noise files to mix with the clean speech file.

options:
  -h, --help            show this help message and exit
  --model-base-dir MODEL_BASE_DIR, -m MODEL_BASE_DIR
                        Model directory containing checkpoints and config. To load a pretrained model, you may
                        just provide the model name, e.g. `DeepFilterNet`. By default, the pretrained
                        DeepFilterNet2 model is loaded.
  --pf                  Post-filter that slightly over-attenuates very noisy sections.
  --output-dir OUTPUT_DIR, -o OUTPUT_DIR
                        Directory in which the enhanced audio files will be stored.
  --log-level LOG_LEVEL
                        Logger verbosity. Can be one of (debug, info, error, none)
  --debug, -d
  --epoch EPOCH, -e EPOCH
                        Epoch for checkpoint loading. Can be one of ['best', 'latest', <int>].
  -v, --version         Print DeepFilterNet version information
  --no-delay-compensation
                        Dont't add some paddig to compensate the delay introduced by the real-time STFT/ISTFT
                        implementation.
  --atten-lim ATTEN_LIM, -a ATTEN_LIM
                        Attenuation limit in dB by mixing the enhanced signal with the noisy signal.
  --noisy-dir NOISY_DIR, -i NOISY_DIR
                        Input directory containing noisy audio files. Use instead of `noisy_audio_files`.
  --no-suffix           Don't add the model suffix to the enhanced audio files
  --no-df-stage
```


Normalize the audio


```
uvx ffmpeg-normalize input_DeepFilterNet3.wav --preset podcast
```


Usage

```
usage: ffmpeg-normalize INPUT [INPUT ...] [-o OUTPUT [OUTPUT ...]] [options]

ffmpeg-normalize v1.36.0 -- command line tool for normalizing audio files

options:
  -h, --help            show this help message and exit

File Input/output:
  input                 Input media file(s)
  --input-list INPUT_LIST
                        Path to a text file containing a line-separated list of input files
  -o OUTPUT [OUTPUT ...], --output OUTPUT [OUTPUT ...]
                        Output file names. Will be applied per input file.
                        
                        If no output file name is specified for an input file, the output files
                        will be written to the default output folder with the name `<input>.<ext>`,
                        where `<ext>` is the output extension (see `-ext` option).
                        
                        Example: ffmpeg-normalize 1.wav 2.wav -o 1n.wav 2n.wav
  -of OUTPUT_FOLDER, --output-folder OUTPUT_FOLDER
                        Output folder (default: `normalized`)
                        
                        This folder will be used for input files that have no explicit output
                        name specified.

General Options:
  -f, --force           Force overwrite existing files
  -d, --debug           Print debugging output
  -v, --verbose         Print verbose output
  -q, --quiet           Only print errors in output
  -n, --dry-run         Do not run normalization, only print what would be done
  -pr, --progress       Show progress bar for files and streams
  --version             Print version and exit
  --preset PRESET       Load options from a preset file.
                        
                        Preset files are JSON files located in the presets directory.
                        The directory location depends on your OS:
                        - Linux/macOS: ~/.config/ffmpeg-normalize/presets/
                        - Windows: %APPDATA%/ffmpeg-normalize/presets/
                        
                        Use --list-presets to see available presets.
                        CLI options specified on the command line take precedence over preset values.
  --list-presets        List all available presets and exit

Normalization:
  -nt {ebu,rms,peak}, --normalization-type {ebu,rms,peak}
                        Normalization type (default: `ebu`).
                        
                        EBU normalization performs two passes and normalizes according to EBU
                        R128.
                        
                        RMS-based normalization brings the input file to the specified RMS
                        level.
                        
                        Peak normalization brings the signal to the specified peak level.
  -t TARGET_LEVEL, --target-level TARGET_LEVEL
                        Normalization target level in dB/LUFS (default: -23.0).
                        
                        For EBU normalization, it corresponds to Integrated Loudness Target
                        in LUFS. The range is -70.0 - -5.0.
                        
                        Otherwise, the range is -99 to 0.
  -p, --print-stats     Print loudness statistics for both passes formatted as JSON to stdout.
  --replaygain          Write ReplayGain tags to the original file without normalizing.
                        This mode will overwrite the input file and ignore other options.
                        Only works with EBU normalization, and only with .mp3, .mp4/.m4a, .ogg, .opus for now.
  --batch               Preserve relative loudness between files (album mode).
                        
                        When operating on a group of unrelated files, you usually want all of them at the same
                        level. However, a group of music files all from the same album is generally meant to be
                        listened to at the relative volumes they were recorded at. In batch mode, all the specified
                        files are considered to be part of a single album and their relative volumes are preserved.
                        This is done by averaging the loudness of all the files, computing a single adjustment from
                        that, and applying a relative adjustment to all the files.
                        
                        Batch mode works with all normalization types (EBU, RMS, peak).

EBU R128 Normalization:
  -lrt LOUDNESS_RANGE_TARGET, --loudness-range-target LOUDNESS_RANGE_TARGET
                        EBU Loudness Range Target in LUFS (default: 7.0).
                        Range is 1.0 - 50.0.
  --keep-loudness-range-target
                        Keep the input loudness range target to allow for linear normalization.
  --keep-lra-above-loudness-range-target
                        Keep input loudness range above loudness range target.
                        Can be used as an alternative to `--keep-loudness-range-target` to allow for linear normalization.
  -tp TRUE_PEAK, --true-peak TRUE_PEAK
                        EBU Maximum True Peak in dBTP (default: -2.0).
                        Range is -9.0 - +0.0.
  --offset OFFSET       EBU Offset Gain (default: 0.0).
                        The gain is applied before the true-peak limiter in the first pass only.
                        The offset for the second pass will be automatically determined based on the first pass statistics.
                        Range is -99.0 - +99.0.
  --lower-only          Whether the audio should not increase in loudness.
                        
                        If the measured loudness from the first pass is lower than the target
                        loudness then normalization will be skipped for the audio source.
                        
                        For EBU normalization, this compares input integrated loudness to the target level.
                        For peak normalization, this compares the input peak level to the target level.
                        For RMS normalization, this compares the input RMS level to the target level.
  --auto-lower-loudness-target
                        Automatically lower EBU Integrated Loudness Target to prevent falling
                        back to dynamic filtering.
                        
                        Makes sure target loudness is lower than measured loudness minus peak
                        loudness (input_i - input_tp) by a small amount (0.1 LUFS).
  --dual-mono           Treat mono input files as "dual-mono".
                        
                        If a mono file is intended for playback on a stereo system, its EBU R128
                        measurement will be perceptually incorrect. If set, this option will
                        compensate for this effect. Multi-channel input files are not affected
                        by this option.
  --dynamic             Force dynamic normalization mode.
                        
                        Instead of applying linear EBU R128 normalization, choose a dynamic
                        normalization. This is not usually recommended.
                        
                        Dynamic mode will automatically change the sample rate to 192 kHz. Use
                        -ar/--sample-rate to specify a different output sample rate.

Audio Stream Selection:
  -as AUDIO_STREAMS, --audio-streams AUDIO_STREAMS
                        Select specific audio streams to normalize by stream index (comma-separated).
                        Example: --audio-streams 0,2 will normalize only streams 0 and 2.
                        
                        By default, all audio streams are normalized.
  --audio-default-only  Only normalize audio streams with the 'default' disposition flag.
                        This is useful for files with multiple audio tracks where only the main track
                        should be normalized (e.g., keeping commentary tracks unchanged).
  --keep-other-audio    Keep non-selected audio streams in the output file (copy without normalization).
                        Only applies when --audio-streams or --audio-default-only is used.
                        
                        By default, only selected streams are included in the output.

Audio Encoding:
  -c:a AUDIO_CODEC, --audio-codec AUDIO_CODEC
                        Audio codec to use for output files.
                        See `ffmpeg -encoders` for a list.
                        
                        Will use PCM audio with input stream bit depth by default.
  -b:a AUDIO_BITRATE, --audio-bitrate AUDIO_BITRATE
                        Audio bitrate in bits/s, or with K suffix.
                        
                        If not specified, will use codec default.
  -ar SAMPLE_RATE, --sample-rate SAMPLE_RATE
                        Audio sample rate to use for output files in Hz.
                        
                        Will use input sample rate by default, except for EBU normalization,
                        which will change the input sample rate to 192 kHz.
  -ac AUDIO_CHANNELS, --audio-channels AUDIO_CHANNELS
                        Set the number of audio channels.
                        If not specified, the input channel layout will be used.
  -koa, --keep-original-audio
                        Copy original, non-normalized audio streams to output file
  -prf PRE_FILTER, --pre-filter PRE_FILTER
                        Add an audio filter chain before applying normalization.
                        Multiple filters can be specified by comma-separating them.
  -pof POST_FILTER, --post-filter POST_FILTER
                        Add an audio filter chain after applying normalization.
                        Multiple filters can be specified by comma-separating them.
                        
                        For EBU, the filter will be applied during the second pass.

Other Encoding Options:
  -vn, --video-disable  Do not write video streams to output
  -c:v VIDEO_CODEC, --video-codec VIDEO_CODEC
                        Video codec to use for output files (default: 'copy').
                        See `ffmpeg -encoders` for a list.
                        
                        Will attempt to copy video codec by default.
  -sn, --subtitle-disable
                        Do not write subtitle streams to output
  -mn, --metadata-disable
                        Do not write metadata to output
  -cn, --chapters-disable
                        Do not write chapters to output

Input/Output options:
  -ei EXTRA_INPUT_OPTIONS, --extra-input-options EXTRA_INPUT_OPTIONS
                        Extra input options list.
                        
                        A list of extra ffmpeg command line arguments valid for the input,
                        applied before ffmpeg's `-i`.
                        
                        You can either use a JSON-formatted list (i.e., a list of
                        comma-separated, quoted elements within square brackets), or a simple
                        string of space-separated arguments.
                        
                        If JSON is used, you need to wrap the whole argument in quotes to
                        prevent shell expansion and to preserve literal quotes inside the
                        string. If a simple string is used, you need to specify the argument
                        with `-e=`.
                        
                        Examples: `-e '[ "-f", "mpegts" ]'` or `-e="-f mpegts"`
  -e EXTRA_OUTPUT_OPTIONS, --extra-output-options EXTRA_OUTPUT_OPTIONS
                        Extra output options list.
                        
                        A list of extra ffmpeg command line arguments.
                        
                        You can either use a JSON-formatted list (i.e., a list of
                        comma-separated, quoted elements within square brackets), or a simple
                        string of space-separated arguments.
                        
                        If JSON is used, you need to wrap the whole argument in quotes to
                        prevent shell expansion and to preserve literal quotes inside the
                        string. If a simple string is used, you need to specify the argument
                        with `-e=`.
                        
                        Examples: `-e '[ "-vbr", "3" ]'` or `-e="-vbr 3"`
  -ofmt OUTPUT_FORMAT, --output-format OUTPUT_FORMAT
                        Media format to use for output file(s).
                        See 'ffmpeg -formats' for a list.
                        
                        If not specified, the format will be inferred by ffmpeg from the output
                        file name. If the output file name is not explicitly specified, the
                        extension will govern the format (see '--extension' option).
  -ext EXTENSION, --extension EXTENSION
                        Output file extension to use for output files that were not explicitly
                        specified. (Default: `mkv`)

The program additionally respects environment variables:

  - `TMP` / `TEMP` / `TMPDIR`
        Sets the path to the temporary directory in which files are
        stored before being moved to the final output directory.
        Note: You need to use full paths.

  - `FFMPEG_PATH`
        Sets the full path to an `ffmpeg` executable other than
        the system default.

Author: Werner Robitza
License: MIT
Website / Issues: https://github.com/slhck/ffmpeg-normalize

```
