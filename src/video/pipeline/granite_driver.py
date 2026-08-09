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
        win = window_ms * 16000 // 1000

        all_segments = []
        with model.session(n_threads=args.threads) as session:
            for t0 in range(0, len(pcm), win):
                t1 = min(t0 + win, len(pcm))
                if t1 <= t0:
                    break
                results = transcribe_window(session, pcm[t0:t1], args.language)
                for result, local_off in results:
                    off_ms = (t0 + local_off) * 1000 // 16000
                    all_segments.extend(result_to_segments(result, off_ms))
                progress(t1 * 1000 // 16000, total_ms)

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
