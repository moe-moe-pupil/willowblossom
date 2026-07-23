"""Persistent Chinese-only EmotiVoice worker used by the replay studio."""

import json
import os
import sys
import traceback
from contextlib import redirect_stdout
from pathlib import Path

import numpy as np
import soundfile as sf
import torch
from transformers import AutoTokenizer
from yacs import config as yacs_config


def reply(payload):
    print(json.dumps(payload, ensure_ascii=False), flush=True)


def scan_checkpoint(directory, prefix):
    matches = sorted(Path(directory).glob(f"{prefix}*"))
    if not matches:
        raise FileNotFoundError(f"missing {prefix} checkpoint in {directory}")
    return matches[-1]


try:
    source = Path(os.environ["EMOTIVOICE_SOURCE"]).resolve()
    os.chdir(source)
    sys.path.insert(0, str(source))

    with redirect_stdout(sys.stderr):
        from config.joint.config import Config
        from frontend_cn import g2p_cn
        from models.hifigan.get_vocoder import MAX_WAV_VALUE
        from models.prompt_tts_modified.jets import JETSGenerator
        from models.prompt_tts_modified.simbert import StyleEncoder

        config = Config()
        nested_bert = source / "WangZeJun" / "simbert-base-chinese" / "simbert-base-chinese"
        direct_bert = source / "WangZeJun" / "simbert-base-chinese"
        config.bert_path = str(nested_bert if nested_bert.is_dir() else direct_bert)

        device = torch.device("cuda" if torch.cuda.is_available() else "cpu")
        with open(config.model_config_path, encoding="utf-8") as stream:
            model_config = yacs_config.CfgNode.load_cfg(stream)
        model_config.n_vocab = config.n_symbols
        model_config.n_speaker = config.speaker_n_labels

        style_encoder = StyleEncoder(config)
        style_checkpoint = torch.load(
            scan_checkpoint(source / "outputs" / "style_encoder" / "ckpt", "checkpoint_"),
            map_location="cpu",
            weights_only=False,
        )
        style_encoder.load_state_dict(
            {key[7:]: value for key, value in style_checkpoint["model"].items()},
            strict=False,
        )
        style_encoder.eval()

        generator = JETSGenerator(model_config).to(device)
        generator_checkpoint = torch.load(
            scan_checkpoint(
                source / "outputs" / "prompt_tts_open_source_joint" / "ckpt",
                "g_",
            ),
            map_location=device,
            weights_only=False,
        )
        generator.load_state_dict(generator_checkpoint["generator"])
        generator.eval()

        tokenizer = AutoTokenizer.from_pretrained(config.bert_path, local_files_only=True)
        with open(config.token_list_path, encoding="utf-8") as stream:
            token2id = {token.strip(): index for index, token in enumerate(stream)}
        with open(config.speaker2id_path, encoding="utf-8") as stream:
            speaker2id = {speaker.strip(): index for index, speaker in enumerate(stream)}

    reply(
        {
            "ready": True,
            "device": str(device),
            "speaker_count": len(speaker2id),
        }
    )
except Exception as error:
    reply({"ready": False, "error": f"{error}\n{traceback.format_exc()}"})
    raise SystemExit(1)


def style_embedding(text):
    encoded = tokenizer([text], return_tensors="pt")
    with torch.no_grad():
        result = style_encoder(
            input_ids=encoded["input_ids"],
            token_type_ids=encoded.get("token_type_ids"),
            attention_mask=encoded["attention_mask"],
        )
    return result["pooled_output"].squeeze(0).to(device)


for line in sys.stdin:
    try:
        request = json.loads(line)
        text = str(request["text"]).strip()
        speaker_name = str(request["speaker"]).strip()
        emotion = str(request.get("emotion", "普通")).strip() or "普通"
        speed = max(float(request.get("speed", 1.0)), 0.1)
        if not text:
            raise ValueError("台词中没有可朗读的中文文字")
        if speaker_name not in speaker2id:
            raise ValueError(f"未知 EmotiVoice 说话人：{speaker_name}")

        phonemes = g2p_cn(text).split()
        sequence = torch.tensor(
            [[token2id[phoneme] for phoneme in phonemes]],
            dtype=torch.long,
            device=device,
        )
        sequence_len = torch.tensor([sequence.shape[1]], device=device)
        speaker = torch.tensor([speaker2id[speaker_name]], device=device)
        with torch.inference_mode():
            inference = generator(
                inputs_ling=sequence,
                inputs_style_embedding=style_embedding(emotion).unsqueeze(0),
                input_lengths=sequence_len,
                inputs_content_embedding=style_embedding(text).unsqueeze(0),
                inputs_speaker=speaker,
                alpha=1.0,
            )
        audio = (
            inference["wav_predictions"].squeeze().detach().cpu().numpy() * MAX_WAV_VALUE
        ).clip(-32768, 32767).astype(np.int16)
        output_path = os.path.abspath(request["output_path"])
        os.makedirs(os.path.dirname(output_path), exist_ok=True)
        sf.write(output_path, audio, config.sampling_rate, subtype="PCM_16")
        reply({"ok": True})
    except Exception as error:
        reply({"ok": False, "error": f"{error}\n{traceback.format_exc()}"})
