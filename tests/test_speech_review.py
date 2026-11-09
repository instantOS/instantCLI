"""Run with: uv run --no-project --python 3.12 --with numpy python -m unittest discover -s tests -p test_speech_review.py"""

from contextlib import nullcontext, redirect_stdout
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch
import wave

import numpy as np


SCRIPT = Path(__file__).resolve().parents[1] / "src/video/pipeline/speech_review.py"
spec = importlib.util.spec_from_file_location("speech_review", SCRIPT)
speech_review = importlib.util.module_from_spec(spec)
spec.loader.exec_module(speech_review)


class SpeechReviewTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.wav = Path(self.temporary.name) / "source.wav"

    def write_wav(self, samples, rate=16000, channels=1):
        with wave.open(str(self.wav), "wb") as wav:
            wav.setnchannels(channels)
            wav.setsampwidth(2)
            wav.setframerate(rate)
            wav.writeframes(np.asarray(samples, dtype="<i2").tobytes())

    def detector(self, ranges):
        segments = [SimpleNamespace(start_sample=start, end_sample=end) for start, end in ranges]
        run = Mock(return_value=SimpleNamespace(segments=segments))
        session = Mock(return_value=nullcontext(SimpleNamespace(run=run)))
        model = Mock(return_value=nullcontext(SimpleNamespace(vad_session=session)))
        return SimpleNamespace(Model=model), run, session

    def test_silence_is_valid_analysis_with_no_detected_speech(self):
        self.write_wav(np.zeros(16000, dtype=np.int16))
        detector, run, session = self.detector([])
        with patch.dict(sys.modules, transcribe_cpp=detector):
            activity = speech_review.review("model.gguf", str(self.wav))
        self.assertEqual(activity, {"version": 1, "duration_seconds": 1.0, "speech": []})
        detector.Model.assert_called_once_with("model.gguf", backend="cpu")
        session.assert_called_once_with(n_threads=1)
        _, options = run.call_args
        self.assertEqual(options, dict(threshold=0.35, min_speech_ms=0,
                                       min_silence_ms=100, speech_pad_ms=150))

    def test_pcm_is_little_endian_normalized_float32(self):
        self.write_wav([-32768, 0, 16384, 32767])
        pcm = speech_review.read_pcm_16k(str(self.wav))
        self.assertEqual(pcm.dtype, np.float32)
        np.testing.assert_array_equal(pcm, [-1.0, 0.0, 0.5, 32767 / 32768])

    def test_ranges_are_clipped_and_overlapping_padding_is_merged(self):
        self.write_wav(np.zeros(16000, dtype=np.int16))
        detector, _, _ = self.detector([(-100, 4000), (3000, 8000), (12000, 17000)])
        with patch.dict(sys.modules, transcribe_cpp=detector):
            activity = speech_review.review("model.gguf", str(self.wav))
        self.assertEqual(activity["speech"], [dict(start=0.0, end=0.5),
                                             dict(start=0.75, end=1.0)])

    def test_empty_or_incompatible_audio_is_not_treated_as_silence(self):
        for samples, rate, channels in [([], 16000, 1), ([0], 8000, 1), ([0, 0], 16000, 2)]:
            with self.subTest(rate=rate, channels=channels, samples=samples):
                self.write_wav(samples, rate, channels)
                with self.assertRaises(ValueError):
                    speech_review.read_pcm_16k(str(self.wav))

    def test_invalid_detector_ranges_fail_instead_of_flagging_speech(self):
        self.write_wav(np.zeros(16000, dtype=np.int16))
        for ranges in [[(4000, 2000)], [(8000, 10000), (0, 2000)]]:
            detector, _, _ = self.detector(ranges)
            with self.subTest(ranges=ranges), patch.dict(sys.modules, transcribe_cpp=detector):
                with self.assertRaises(ValueError):
                    speech_review.review("model.gguf", str(self.wav))

    def test_cli_emits_json_and_preserves_detector_failures(self):
        expected = dict(version=1, duration_seconds=1.0, speech=[])
        output = io.StringIO()
        with patch.object(speech_review, "review", return_value=expected), redirect_stdout(output):
            result = speech_review.main(["--model", "model.gguf", "--wav", str(self.wav)])
        self.assertEqual(result, 0)
        self.assertEqual(json.loads(output.getvalue()), expected)
        with patch.object(speech_review, "review", side_effect=ValueError("failed")), \
             patch("sys.stderr", new_callable=io.StringIO), redirect_stdout(io.StringIO()):
            self.assertEqual(speech_review.main(["--model", "model.gguf", "--wav", str(self.wav)]), 1)


if __name__ == "__main__":
    unittest.main()
