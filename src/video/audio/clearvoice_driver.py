#!/usr/bin/env python3
"""ClearVoice enhancement driver for `ins` (kept in sync with clearvoice.rs).

Usage: clearvoice_driver.py <input.wav> <output.wav>

Runs fullband 48 kHz AI speech enhancement (denoise + restoration) via
Alibaba's ClearVoice (MossFormer2_SE_48K) and writes the enhanced audio back
as a WAV. The package downloads its checkpoints into ./checkpoints relative to
the process working directory, so the Rust side runs this with cwd = the
shared models directory (downloads happen once per model).

Quiet mode is not used: tqdm progress goes to stderr, which the Rust side
passes through to the user.
"""
import sys

MODEL = "MossFormer2_SE_48K"


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: clearvoice_driver.py <input.wav> <output.wav>", file=sys.stderr)
        return 2
    input_path, output_path = sys.argv[1], sys.argv[2]

    from clearvoice import ClearVoice

    cv = ClearVoice(task="speech_enhancement", model_names=[MODEL])
    # online_write=False -> waveform kept in memory; write() stores it to disk
    # with the package's own resampling/int16 handling.
    cv(input_path=input_path, online_write=False)
    cv.write(None, output_path=output_path)
    return 0


if __name__ == "__main__":
    sys.exit(main())
