"""Persistent Chinese-only MeloTTS worker used by the replay studio."""

import json
import os
import sys
import traceback
from contextlib import redirect_stdout

os.environ.setdefault("HF_HUB_DISABLE_SYMLINKS_WARNING", "1")
os.environ.setdefault("TOKENIZERS_PARALLELISM", "false")


def reply(payload):
    print(json.dumps(payload, ensure_ascii=False), flush=True)


try:
    with redirect_stdout(sys.stderr):
        from melo.api import TTS
        model = TTS(language="ZH", device="cpu")
    speaker_id = model.hps.data.spk2id["ZH"]
    reply({"ready": True})
except Exception as error:
    reply({"ready": False, "error": f"{error}\n{traceback.format_exc()}"})
    raise SystemExit(1)


for line in sys.stdin:
    try:
        request = json.loads(line)
        text = str(request["text"]).strip()
        if not text:
            raise ValueError("台词中没有可朗读的中文文字")
        output_path = os.path.abspath(request["output_path"])
        os.makedirs(os.path.dirname(output_path), exist_ok=True)
        with redirect_stdout(sys.stderr):
            model.tts_to_file(
                text,
                speaker_id,
                output_path,
                speed=max(float(request.get("speed", 1.0)), 0.1),
                quiet=True,
            )
        reply({"ok": True})
    except Exception as error:
        reply({"ok": False, "error": f"{error}\n{traceback.format_exc()}"})
