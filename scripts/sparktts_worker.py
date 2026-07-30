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
import unicodedata
import wave
from pathlib import Path

import numpy as np
import torch


def split_speech_text(text: str, max_units: int = 18) -> list[str]:
    """Keep Spark generations short enough that early EOS cannot drop a long suffix."""
    chunks: list[str] = []
    current: list[str] = []
    units = 0
    for character in text:
        current.append(character)
        category = unicodedata.category(character)
        if not character.isspace() and not category.startswith("P"):
            units += 1
        natural_break = category.startswith("P") and units >= 6
        if natural_break or units > max_units:
            chunk = "".join(current).strip()
            if chunk:
                chunks.append(chunk)
            current = []
            units = 0
    chunk = "".join(current).strip()
    if chunk:
        chunks.append(chunk)
    return chunks


def trim_runaway_tail(text: str, audio: np.ndarray, sample_rate: int) -> np.ndarray:
    """Bound pathological generations that repeat the final word indefinitely."""
    units = sum(
        1
        for character in text
        if not character.isspace()
        and not unicodedata.category(character).startswith("P")
    )
    pauses = sum(1 for character in text if unicodedata.category(character).startswith("P"))
    max_seconds = max(3.0, units * 0.55 + pauses * 0.25 + 1.5)
    max_samples = int(max_seconds * sample_rate)
    samples = np.asarray(audio, dtype=np.float32).reshape(-1)
    if samples.size <= max_samples:
        return samples

    samples = samples[:max_samples].copy()
    fade_samples = min(int(sample_rate * 0.12), samples.size)
    if fade_samples:
        samples[-fade_samples:] *= np.linspace(1.0, 0.0, fade_samples, dtype=np.float32)
    return samples


def emit(payload: dict) -> bool:
    """Write one protocol message, returning false when the parent is gone."""
    try:
        print(json.dumps(payload, ensure_ascii=False), flush=True)
        return True
    except OSError:
        # Windows reports a closed anonymous pipe as EINVAL (22), rather than
        # BrokenPipeError. The Rust parent has gone, so the worker should exit
        # quietly instead of trying to report the same error through that pipe.
        return False


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
    if samples.size == 0:
        raise ValueError("Spark-TTS returned empty audio")
    peak = float(np.max(np.abs(samples)))
    if not np.isfinite(peak) or peak < 0.005:
        raise ValueError(
            f"Spark-TTS returned silent audio (peak={peak:.6f}); "
            "the generated voice was not saved"
        )
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

    if not emit({"ready": True, "device": device, "profile_count": len(profiles)}):
        return 0

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

            parts: list[np.ndarray] = []
            chunks = split_speech_text(text)
            for chunk_index, chunk in enumerate(chunks):
                seed_for(profile_id, chunk)
                with torch.inference_mode(), contextlib.redirect_stdout(sys.stderr):
                    # Short, deterministic generations avoid Spark silently
                    # ending a long line after only its first clause.
                    audio = model.inference(
                        chunk,
                        gender=profile["gender"],
                        pitch=profile["pitch"],
                        speed=profile["speed"],
                        temperature=0.65,
                        top_k=50,
                        top_p=0.95,
                    )
                parts.append(trim_runaway_tail(chunk, audio, sample_rate))
                if chunk_index + 1 < len(chunks):
                    parts.append(np.zeros(int(sample_rate * 0.08), dtype=np.float32))
            audio = np.concatenate(parts)
            write_pcm16(output_path, audio, sample_rate)
            if not emit({"ok": True, "output_path": str(output_path)}):
                return 0
        except Exception as error:
            if not emit({"ok": False, "error": str(error)}):
                return 0

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
