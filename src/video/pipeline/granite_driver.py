#!/usr/bin/env python3
"""Granite Speech transcription driver for `ins video`.

Runs a Granite Speech 4.1 GGUF through the transcribe.cpp Python bindings and
writes WhisperX-shaped JSON ({"segments": [{"start", "end", "text",
"words": [{"word", "start", "end"}]}]}) so ins's existing transcript parser
(parse_whisper_json) consumes it unchanged.

CLI:
    granite_driver.py --model MODEL.gguf --wav IN.wav --out OUT.json
                     [--language en|de|...] [--window-ms N] [--backend auto]

Progress is reported on stderr as `TC_PROGRESS <done_ms> <total_ms>` lines
(one per completed window) so the caller can map them to a percentage.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import tempfile
import wave

import numpy as np
import transcribe_cpp as tc


def read_pcm_16k(path: str) -> np.ndarray:
    """Read a 16 kHz mono s16 WAV into a float32 1-D numpy array."""
    with wave.open(path, "rb") as wav:
        sr = wav.getframerate()
        channels = wav.getnchannels()
        width = wav.getsampwidth()
        if sr != 16000:
            raise SystemExit(f"expected 16 kHz WAV, got {sr} Hz")
        if channels != 1:
            raise SystemExit(f"expected mono WAV, got {channels} channels")
        if width != 2:
            raise SystemExit(f"expected s16 WAV, got {width * 8}-bit samples")
        raw = wav.readframes(wav.getnframes())
    return np.frombuffer(raw, dtype=np.int16).astype(np.float32) / 32768.0


def progress(done_ms: int, total_ms: int) -> None:
    sys.stderr.write(f"TC_PROGRESS {done_ms} {total_ms}\n")
    sys.stderr.flush()


MIN_RETRY_SAMPLES = 16000
SAMPLE_RATE = 16000
VAD_FRAME_SAMPLES = 320  # 20 ms
VAD_MIN_SILENCE_FRAMES = 40  # 800 ms
VAD_PADDING_SAMPLES = 4000  # 250 ms


def speech_windows(pcm: np.ndarray) -> list[tuple[int, int, int | None, int | None]]:
    """Split audio around sustained acoustic silence.

    Granite Speech can return internally consistent words while compressing a
    long silent interval in its timestamp clock.  Feeding speech islands
    separately anchors every island to its real source offset and prevents that
    drift.  The threshold is relative to this recording's noise floor, so this
    also works for microphones recorded at different levels and distances.

    Boundaries include a little context to avoid clipping quiet consonants.
    Only sustained silence is split; ordinary pauses remain with the sentence.
    """
    if len(pcm) < SAMPLE_RATE:
        return [(0, len(pcm), None, None)]

    # Remove DC and very-low-frequency room/mechanical noise before measuring
    # energy.  A centered 100 ms moving average is inexpensive and sufficient
    # for VAD; this does not alter the samples sent to the model.
    radius = SAMPLE_RATE // 20
    cumulative = np.concatenate(
        (np.zeros(1, dtype=np.float64), np.cumsum(pcm, dtype=np.float64))
    )
    indices = np.arange(len(pcm))
    left = np.maximum(0, indices - radius)
    right = np.minimum(len(pcm), indices + radius + 1)
    local_mean = (cumulative[right] - cumulative[left]) / (right - left)
    filtered = pcm - local_mean

    frame_count = len(filtered) // VAD_FRAME_SAMPLES
    if frame_count == 0:
        return [(0, len(pcm), None, None)]
    frames = filtered[: frame_count * VAD_FRAME_SAMPLES].reshape(
        frame_count, VAD_FRAME_SAMPLES
    )
    rms = np.sqrt(np.mean(frames * frames, axis=1) + 1e-12)
    db = 20.0 * np.log10(rms)
    noise_floor = float(np.percentile(db, 20))
    speech_level = float(np.percentile(db, 90))
    threshold = min(noise_floor + 15.0, speech_level - 15.0)
    silent = db < threshold

    silences: list[tuple[int, int]] = []
    start = None
    for index, is_silent in enumerate(silent):
        if is_silent and start is None:
            start = index
        elif not is_silent and start is not None:
            if index - start >= VAD_MIN_SILENCE_FRAMES:
                silences.append(
                    (start * VAD_FRAME_SAMPLES, index * VAD_FRAME_SAMPLES)
                )
            start = None
    if start is not None and frame_count - start >= VAD_MIN_SILENCE_FRAMES:
        silences.append((start * VAD_FRAME_SAMPLES, len(pcm)))

    if not silences:
        return [(0, len(pcm), None, None)]

    windows: list[tuple[int, int, int | None, int | None]] = []
    cursor = 0
    speech_start = None
    for silence_start, silence_end in silences:
        end = min(len(pcm), silence_start + VAD_PADDING_SAMPLES)
        if end > cursor:
            windows.append((cursor, end, speech_start, silence_start))
        cursor = max(0, silence_end - VAD_PADDING_SAMPLES)
        speech_start = silence_end
    if cursor < len(pcm):
        windows.append((cursor, len(pcm), speech_start, None))

    # A leading/trailing all-silent island has no value to the recognizer.
    return [
        window
        for window in windows
        if window[1] - window[0] >= SAMPLE_RATE // 2
    ]


def align_segments_to_speech_boundaries(
    segments: list[dict], speech_start: int | None, speech_end: int | None
) -> None:
    """Affine-align Granite's word clock to acoustic island boundaries."""
    words = [word for segment in segments for word in segment["words"]]
    if not words or speech_start is None or speech_end is None:
        return
    model_start = words[0]["start"]
    model_end = words[-1]["end"]
    actual_start = speech_start / SAMPLE_RATE
    actual_end = speech_end / SAMPLE_RATE
    if model_end <= model_start or actual_end <= actual_start:
        return
    scale = (actual_end - actual_start) / (model_end - model_start)

    def aligned(value: float) -> float:
        return round(actual_start + (value - model_start) * scale, 3)

    for segment in segments:
        for word in segment["words"]:
            word["start"] = aligned(word["start"])
            word["end"] = aligned(word["end"])
        if segment["words"]:
            segment["start"] = segment["words"][0]["start"]
            segment["end"] = segment["words"][-1]["end"]


def transcribe_window(session, pcm, language: str | None, off_samples: int = 0):
    """Run a window, recursively splitting it when the model cannot finish.

    Both errors require retrying shorter audio.  In particular, accepting the
    partial result from OutputTruncated would silently discard the ungenerated
    end of the window.  Return every successful sub-window with its sample
    offset so timestamps remain relative to the complete recording.
    """
    try:
        result = session.run(
            pcm,
            task="transcribe",
            language=language,
            timestamps="word",
        )
        return [(result, off_samples)]
    except (tc.InputTooLong, tc.OutputTruncated) as exc:
        if len(pcm) <= MIN_RETRY_SAMPLES:
            raise

        split = len(pcm) // 2
        duration_ms = len(pcm) * 1000 // 16000
        sys.stderr.write(
            f"TCWARN {type(exc).__name__} for {duration_ms} ms window; "
            "retrying as two shorter windows\n"
        )
        sys.stderr.flush()

        return transcribe_window(
            session, pcm[:split], language, off_samples
        ) + transcribe_window(session, pcm[split:], language, off_samples + split)


def result_to_segments(result: tc.Result, off_ms: int) -> list[dict]:
    """Convert one window's Result into whisperx-shaped segments (seconds)."""
    segments = []
    for seg in result.segments:
        words = result.words[seg.first_word : seg.first_word + seg.n_words]
        rows = [
            {
                "word": w.text.strip(),
                "start": round((off_ms + w.t0_ms) / 1000.0, 3),
                "end": round((off_ms + w.t1_ms) / 1000.0, 3),
            }
            for w in words
        ]
        text = " ".join(w.text.strip() for w in words) or seg.text.strip()
        segments.append(
            {
                "start": round((off_ms + seg.t0_ms) / 1000.0, 3),
                "end": round((off_ms + seg.t1_ms) / 1000.0, 3),
                "text": text,
                "words": rows,
            }
        )
    return segments


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", required=True)
    ap.add_argument("--wav", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--language", default=None)
    ap.add_argument("--backend", default="auto", choices=["auto", "cpu", "vulkan"])
    ap.add_argument("--window-ms", type=int, default=0)
    ap.add_argument("--threads", type=int, default=0)
    args = ap.parse_args()

    pcm = read_pcm_16k(args.wav)
    total_ms = len(pcm) * 1000 // 16000

    with tc.Model(args.model, backend=args.backend) as model:
        caps = model.capabilities
        sys.stderr.write(
            f"TC_CAPS arch={model.arch} variant={model.variant} "
            f"backend={model.backend} max_ts={caps.max_timestamp_kind} "
            f"max_audio_ms={caps.max_audio_ms} "
            f"langs={','.join(caps.languages)}\n"
        )
        if caps.max_timestamp_kind not in ("word", "token"):
            raise SystemExit(
                f"model {model.variant} does not expose word timestamps "
                f"(max_timestamp_kind={caps.max_timestamp_kind})"
            )
        if args.language and caps.languages and args.language not in caps.languages:
            sys.stderr.write(
                f"TCWARN language {args.language!r} not in model languages "
                f"{sorted(caps.languages)}; transcribing anyway\n"
            )

        window_ms = args.window_ms or caps.max_audio_ms or 200000
        window_ms = max(1000, min(window_ms, 600000))
        win = window_ms * SAMPLE_RATE // 1000

        all_segments = []
        with model.session(n_threads=args.threads) as session:
            windows = speech_windows(pcm)
            sys.stderr.write(
                f"TC_VAD speech_islands={len(windows)} "
                f"covered_ms={sum(end - start for start, end, _, _ in windows) * 1000 // SAMPLE_RATE}\n"
            )
            sys.stderr.flush()
            for island_start, island_end, speech_start, speech_end in windows:
                island_segments = []
                for t0 in range(island_start, island_end, win):
                    t1 = min(t0 + win, island_end)
                    if t1 <= t0:
                        break
                    results = transcribe_window(session, pcm[t0:t1], args.language)
                    for result, local_off in results:
                        off_ms = (t0 + local_off) * 1000 // SAMPLE_RATE
                        island_segments.extend(result_to_segments(result, off_ms))
                    progress(t1 * 1000 // SAMPLE_RATE, total_ms)
                align_segments_to_speech_boundaries(
                    island_segments, speech_start, speech_end
                )
                all_segments.extend(island_segments)
            progress(total_ms, total_ms)

    out = {"segments": all_segments}
    tmp = tempfile.NamedTemporaryFile(
        "w", dir=os.path.dirname(os.path.abspath(args.out)), delete=False
    )
    try:
        json.dump(out, tmp, ensure_ascii=False, sort_keys=False)
    finally:
        tmp.close()
    os.replace(tmp.name, args.out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
