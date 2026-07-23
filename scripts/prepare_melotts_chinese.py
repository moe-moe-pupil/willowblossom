"""Reduce the upstream MeloTTS checkout to the Chinese inference path."""

from pathlib import Path
import sys


source = Path(sys.argv[1]).resolve()
api = source / "melo" / "api.py"
api_text = api.read_text(encoding="utf-8")
api_text = api_text.replace(
    "self.language = 'ZH_MIX_EN' if language == 'ZH' else language # we support a ZH_MIX_EN model",
    "self.language = language  # Willowblossom uses the Chinese-only path",
)
api.write_text(api_text, encoding="utf-8")

(source / "melo" / "text" / "cleaner.py").write_text(
    """from . import chinese
from . import cleaned_text_to_sequence

language_module_map = {"ZH": chinese}


def clean_text(text, language):
    language_module = language_module_map[language]
    norm_text = language_module.text_normalize(text)
    phones, tones, word2ph = language_module.g2p(norm_text)
    return norm_text, phones, tones, word2ph


def text_to_sequence(text, language):
    norm_text, phones, tones, word2ph = clean_text(text, language)
    return cleaned_text_to_sequence(phones, tones, language)
""",
    encoding="utf-8",
)

text_init = source / "melo" / "text" / "__init__.py"
init_text = text_init.read_text(encoding="utf-8")
start = init_text.index("def get_bert(")
init_text = (
    init_text[:start]
    + """def get_bert(norm_text, word2ph, language, device):
    if language != "ZH":
        raise ValueError("Willowblossom MeloTTS runtime only supports Chinese")
    from .chinese_bert import get_bert_feature
    return get_bert_feature(norm_text, word2ph, device)
"""
)
text_init.write_text(init_text, encoding="utf-8")
