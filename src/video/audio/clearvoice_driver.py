#!/usr/bin/env python3
"""ClearVoice enhancement driver for `ins` (kept in sync with clearvoice.rs).

Usage: clearvoice_driver.py <input.wav> <output.wav>

Runs fullband 48 kHz AI speech enhancement (denoise + restoration) via
Alibaba's ClearVoice (MossFormer2_SE_48K) and writes the enhanced audio back
as a WAV. The package downloads its checkpoints into ./checkpoints relative to
the process working directory, so the Rust side runs this with cwd = the
shared models directory (downloads happen once per model).

Progress is reported on stderr as `CV_PROGRESS <done_ms> <total_ms>` lines
so the Rust caller can render a progress bar. Other stderr output is kept
for error reporting.
"""
import sys
import wave

import numpy as np

MODEL = "MossFormer2_SE_48K"
SAMPLE_RATE = 48000
CHUNK_SECONDS = 10
OVERLAP_SECONDS = 1
PROGRESS_PREFIX = "CV_PROGRESS"


def report_progress(done_samples: int, total_samples: int) -> None:
    """Report machine-readable enhancement progress on stderr."""
    if total_samples <= 0:
        return
    done_ms = done_samples * 1000 // SAMPLE_RATE
    total_ms = total_samples * 1000 // SAMPLE_RATE
    print(f"{PROGRESS_PREFIX} {done_ms} {total_ms}", file=sys.stderr, flush=True)


def read_pcm(path: str) -> np.ndarray:
    """Read the mono 48 kHz s16 WAV prepared by the Rust pipeline."""
    with wave.open(path, "rb") as wav:
        if wav.getframerate() != SAMPLE_RATE:
            raise SystemExit(
                f"expected {SAMPLE_RATE} Hz input, got {wav.getframerate()} Hz"
            )
        if wav.getnchannels() != 1 or wav.getsampwidth() != 2:
            raise SystemExit("expected mono 16-bit PCM input")
        raw = wav.readframes(wav.getnframes())
    return np.frombuffer(raw, dtype="<i2").astype(np.float32) / 32768.0


def write_pcm(path: str, audio: np.ndarray) -> None:
    """Write mono 48 kHz s16 WAV without relying on ClearVoice's IO state."""
    pcm = np.clip(np.rint(audio * 32768.0), -32768, 32767).astype("<i2")
    with wave.open(path, "wb") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(SAMPLE_RATE)
        wav.writeframes(pcm.tobytes())


def enhance_overlapped(cv, audio: np.ndarray) -> np.ndarray:
    """Enhance short overlapping chunks and blend their shared samples.

    ClearVoice hard-splices 4-second model windows for inputs longer than 20
    seconds. Keeping every call below that threshold selects its whole-window
    inference path. A complementary Hann crossfade then joins independently
    enhanced chunks without a waveform step or overlap gain bump.
    """
    chunk_samples = CHUNK_SECONDS * SAMPLE_RATE
    overlap_samples = OVERLAP_SECONDS * SAMPLE_RATE
    stride_samples = chunk_samples - overlap_samples
    output = np.zeros(len(audio), dtype=np.float32)

    start = 0
    previous_end = 0
    while start < len(audio):
        end = min(start + chunk_samples, len(audio))

        # Tensor mode returns [batch, samples] for this single-output model.
        enhanced = np.asarray(cv(audio[np.newaxis, start:end], False))
        enhanced = np.squeeze(enhanced).astype(np.float32, copy=False)
        if enhanced.ndim != 1 or len(enhanced) < end - start:
            raise RuntimeError(
                f"ClearVoice returned shape {enhanced.shape} for "
                f"{end - start} input samples"
            )
        enhanced = enhanced[: end - start]

        shared = max(0, previous_end - start)
        if shared:
            phase = np.linspace(0.0, np.pi, shared, endpoint=True, dtype=np.float32)
            fade_in = 0.5 - 0.5 * np.cos(phase)
            output[start:previous_end] = (
                output[start:previous_end] * (1.0 - fade_in)
                + enhanced[:shared] * fade_in
            )
        output[previous_end:end] = enhanced[shared:]

        previous_end = end
        report_progress(end, len(audio))
        if end == len(audio):
            break
        start += stride_samples

    return output


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: clearvoice_driver.py <input.wav> <output.wav>", file=sys.stderr)
        return 2
    input_path, output_path = sys.argv[1], sys.argv[2]

    from clearvoice import ClearVoice

    audio = read_pcm(input_path)
    report_progress(0, len(audio))
    cv = ClearVoice(task="speech_enhancement", model_names=[MODEL])
    enhanced = enhance_overlapped(cv, audio)
    write_pcm(output_path, enhanced)
    return 0


if __name__ == "__main__":
    sys.exit(main())
