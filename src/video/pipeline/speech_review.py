#!/usr/bin/env python3
"""Review raw source speech activity with Silero VAD, independently of ASR."""

from __future__ import annotations

import argparse
from contextlib import contextmanager, redirect_stdout
import json
import operator
import os
import sys
import wave

import numpy as np

SAMPLE_RATE = 16000


def read_pcm_16k(path: str) -> np.ndarray:
    with wave.open(path, "rb") as wav:
        if wav.getframerate() != SAMPLE_RATE:
            raise ValueError(f"expected 16 kHz WAV, got {wav.getframerate()} Hz")
        if wav.getnchannels() != 1:
            raise ValueError(f"expected mono WAV, got {wav.getnchannels()} channels")
        if wav.getsampwidth() != 2 or wav.getcomptype() != "NONE":
            raise ValueError("expected uncompressed s16 WAV")
        raw = wav.readframes(wav.getnframes())
    if not raw:
        raise ValueError("cannot review empty audio")
    return np.frombuffer(raw, dtype="<i2").astype(np.float32) / 32768.0


@contextmanager
def diagnostics_to_stderr():
    # Native bindings can write directly to fd 1, bypassing redirect_stdout.
    sys.stdout.flush()
    saved_stdout = os.dup(1)
    try:
        os.dup2(2, 1)
        with redirect_stdout(sys.stderr):
            yield
    finally:
        sys.stderr.flush()
        os.dup2(saved_stdout, 1)
        os.close(saved_stdout)


def review(model_path: str, wav_path: str) -> dict:
    pcm = read_pcm_16k(wav_path)
    with diagnostics_to_stderr():
        import transcribe_cpp as tc

        with tc.Model(model_path, backend="cpu") as model:
            with model.vad_session(n_threads=1) as vad:
                result = vad.run(
                    pcm,
                    threshold=0.35,
                    min_speech_ms=0,
                    min_silence_ms=100,
                    speech_pad_ms=150,
                )
                # Consume the native result while its session is still alive.
                ranges = [
                    (operator.index(segment.start_sample), operator.index(segment.end_sample))
                    for segment in result.segments
                ]

    speech = []
    previous_start = None
    for start, end in ranges:
        if end < start or (previous_start is not None and start < previous_start):
            raise ValueError("VAD returned reversed or unsorted sample ranges")
        previous_start = start
        start = max(0, min(len(pcm), start))
        end = max(0, min(len(pcm), end))
        if end <= start:
            continue
        start_seconds, end_seconds = start / SAMPLE_RATE, end / SAMPLE_RATE
        # Padding may overlap neighboring detections; store their union.
        if speech and start_seconds <= speech[-1]["end"]:
            speech[-1]["end"] = max(speech[-1]["end"], end_seconds)
        else:
            speech.append({"start": start_seconds, "end": end_seconds})
    return {"version": 1, "duration_seconds": len(pcm) / SAMPLE_RATE, "speech": speech}


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", required=True)
    parser.add_argument("--wav", required=True)
    args = parser.parse_args(argv)
    try:
        activity = review(args.model, args.wav)
        print(json.dumps(activity, allow_nan=False, separators=(",", ":")))
    except Exception as error:
        print(f"Speech review failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
