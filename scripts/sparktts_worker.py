#!/usr/bin/env python3
"""Persistent offline Spark-TTS worker used by Willowblossom.

The voice bank contains fixed reference recordings. Every dialogue line uses
Spark-TTS voice cloning against one of those recordings, which keeps a
character's timbre stable regardless of the line length.
"""

from __future__ import annotations

import contextlib
import hashlib
import json
import os
import random
import sys
import wave
from pathlib import Path

import numpy as np
import torch


def emit(payload: dict) -> None:
    print(json.dumps(payload, ensure_ascii=False), flush=True)


def seed_for(profile_id: str, text: str) -> None:
    digest = hashlib.sha256(f"{profile_id}\0{text}".encode("utf-8")).digest()
    seed = int.from_bytes(digest[:4], "little")
    random.seed(seed)
    np.random.seed(seed)
    torch.manual_seed(seed)
    if torch.cuda.is_available():
        torch.cuda.manual_seed_all(seed)


def write_pcm16(path: Path, audio: np.ndarray, sample_rate: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    samples = np.asarray(audio, dtype=np.float32).reshape(-1)
    samples = np.clip(samples, -1.0, 1.0)
    pcm = (samples * 32767.0).astype("<i2")
    with wave.open(str(path), "wb") as output:
        output.setnchannels(1)
        output.setsampwidth(2)
        output.setframerate(sample_rate)
        output.writeframes(pcm.tobytes())


def main() -> int:
    source = Path(os.environ["SPARK_TTS_SOURCE"]).resolve()
    model_dir = Path(os.environ["SPARK_TTS_MODEL"]).resolve()
    bank_dir = Path(os.environ["SPARK_TTS_VOICE_BANK"]).resolve()
    sys.path.insert(0, str(source))

    manifest = json.loads((bank_dir / "profiles.json").read_text(encoding="utf-8"))
    profiles = {profile["id"]: profile for profile in manifest["profiles"]}
    sample_rate = int(manifest.get("sample_rate", 16000))
    device = "cuda:0" if torch.cuda.is_available() else "cpu"

    from cli.SparkTTS import SparkTTS

    with contextlib.redirect_stdout(sys.stderr):
        model = SparkTTS(str(model_dir), device=device)

    emit({"ready": True, "device": device, "profile_count": len(profiles)})

    for raw_line in sys.stdin:
        try:
            request = json.loads(raw_line)
            text = str(request["text"]).strip()
            profile_id = str(request["speaker"])
            output_path = Path(request["output_path"]).resolve()
            if not text:
                raise ValueError("text is empty")
            profile = profiles.get(profile_id)
            if profile is None:
                raise ValueError(f"unknown Spark-TTS profile: {profile_id}")

            reference_path = bank_dir / profile["reference_wav"]
            seed_for(profile_id, text)
            with torch.inference_mode(), contextlib.redirect_stdout(sys.stderr):
                audio = model.inference(
                    text,
                    prompt_speech_path=str(reference_path),
                    prompt_text=profile["reference_text"],
                    temperature=0.65,
                    top_k=50,
                    top_p=0.95,
                )
            write_pcm16(output_path, audio, sample_rate)
            emit({"ok": True, "output_path": str(output_path)})
        except Exception as error:
            emit({"ok": False, "error": str(error)})

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
