"""Build Willowblossom's stable local Mandarin Spark-TTS voice bank."""

import argparse
import json
import random
import sys
from pathlib import Path

import numpy as np
import soundfile as sf
import torch


REFERENCE_TEXT = "你好，我是本次远航任务的成员。请放心，我会清楚地汇报现场情况。"

MALE_PROFILES = [
    ("深沉", "very_low"),
    ("厚重", "very_low"),
    ("沉稳", "low"),
    ("冷静", "low"),
    ("温和", "low"),
    ("硬朗", "low"),
    ("可靠", "low"),
    ("清朗", "moderate"),
    ("青年", "moderate"),
    ("成熟", "moderate"),
    ("克制", "moderate"),
    ("机敏", "moderate"),
    ("悠然", "moderate"),
    ("严肃", "high"),
    ("亲切", "high"),
    ("明快", "high"),
]

FEMALE_PROFILES = [
    ("温柔", "low"),
    ("沉静", "low"),
    ("从容", "moderate"),
    ("知性", "moderate"),
    ("可靠", "moderate"),
    ("清澈", "moderate"),
    ("成熟", "moderate"),
    ("克制", "moderate"),
    ("活泼", "high"),
    ("明快", "high"),
    ("灵动", "high"),
    ("亲切", "high"),
    ("坚定", "high"),
    ("轻盈", "very_high"),
    ("稚气", "very_high"),
    ("元气", "very_high"),
]


def seed_everything(seed):
    random.seed(seed)
    np.random.seed(seed)
    torch.manual_seed(seed)
    torch.cuda.manual_seed_all(seed)


def profile_rows():
    rows = []
    for gender, prefix, profiles, seed_base in [
        ("male", "m", MALE_PROFILES, 31000),
        ("female", "f", FEMALE_PROFILES, 32000),
    ]:
        for index, (description, pitch) in enumerate(profiles, start=1):
            rows.append(
                {
                    "id": f"spark-{prefix}{index:02}",
                    "label": f"{'男声' if gender == 'male' else '女声'} {index:02} · {description}",
                    "gender": gender,
                    "pitch": pitch,
                    "speed": "moderate",
                    "seed": seed_base + index,
                    "reference_text": REFERENCE_TEXT,
                    "reference_wav": f"spark-{prefix}{index:02}.wav",
                }
            )
    return rows


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--model", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--device", default="cuda:0")
    parser.add_argument("--force", action="store_true")
    args = parser.parse_args()

    sys.path.insert(0, str(args.source.resolve()))
    from cli.SparkTTS import SparkTTS

    args.output.mkdir(parents=True, exist_ok=True)
    device = torch.device(args.device if torch.cuda.is_available() else "cpu")
    model = SparkTTS(args.model.resolve(), device)
    rows = profile_rows()

    for completed, profile in enumerate(rows, start=1):
        output_path = args.output / profile["reference_wav"]
        if args.force or not output_path.is_file():
            seed_everything(profile["seed"])
            wav = model.inference(
                REFERENCE_TEXT,
                gender=profile["gender"],
                pitch=profile["pitch"],
                speed=profile["speed"],
                temperature=0.65,
            )
            sf.write(output_path, wav, 16000, subtype="PCM_16")
        print(
            json.dumps(
                {
                    "completed": completed,
                    "total": len(rows),
                    "id": profile["id"],
                    "path": str(output_path),
                },
                ensure_ascii=False,
            ),
            flush=True,
        )

    manifest_path = args.output / "profiles.json"
    manifest_path.write_text(
        json.dumps(
            {
                "format_version": 1,
                "engine": "Spark-TTS-0.5B",
                "sample_rate": 16000,
                "profiles": rows,
            },
            ensure_ascii=False,
            indent=2,
        ),
        encoding="utf-8",
    )
    print(json.dumps({"ready": True, "profiles": len(rows)}, ensure_ascii=False))


if __name__ == "__main__":
    main()
