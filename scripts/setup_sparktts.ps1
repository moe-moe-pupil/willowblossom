$ErrorActionPreference = "Stop"
$env:http_proxy = "http://127.0.0.1:10809"
$env:https_proxy = "http://127.0.0.1:10809"

$repoRoot = Split-Path -Parent $PSScriptRoot
$runtimeRoot = Join-Path $repoRoot ".data\willowblossom\tts\spark"
$sourceRoot = Join-Path $runtimeRoot "Spark-TTS"
$sourceCommit = "2f1ea9082400547242641f5271b6f941c9f439d1"
$modelRoot = Join-Path $runtimeRoot "Spark-TTS-0.5B"
$voiceBankRoot = Join-Path $runtimeRoot "voice-bank"
$venvRoot = Join-Path $runtimeRoot ".venv"
$python = Join-Path $venvRoot "Scripts\python.exe"

New-Item -ItemType Directory -Force -Path $runtimeRoot | Out-Null

if (-not (Test-Path $sourceRoot)) {
    git clone --depth 1 https://github.com/SparkAudio/Spark-TTS.git $sourceRoot
}
git -C $sourceRoot fetch origin $sourceCommit --depth 1
git -C $sourceRoot checkout --detach $sourceCommit

if (-not (Test-Path $python)) {
    py -3.10 -m venv $venvRoot
}

& $python -m pip install --upgrade pip
& $python -m pip install `
    --index-url https://download.pytorch.org/whl/cu121 `
    torch==2.5.1+cu121 torchaudio==2.5.1+cu121
& $python -m pip install `
    einops einx "huggingface_hub>=0.28.1" "librosa>=0.10.2" `
    numpy==2.2.3 "omegaconf>=2.3.0" "packaging>=24.2" `
    "safetensors>=0.5.2" "soundfile>=0.12.1" "soxr>=0.5.0" `
    tokenizers==0.20.3 tqdm==4.66.5 transformers==4.46.2

if (-not (Test-Path (Join-Path $modelRoot "config.yaml"))) {
    & $python -c @"
from huggingface_hub import snapshot_download
snapshot_download(
    repo_id="SparkAudio/Spark-TTS-0.5B",
    local_dir=r"$modelRoot",
)
"@
}

$env:PYTHONPATH = $sourceRoot
& $python (Join-Path $PSScriptRoot "sparktts_build_voice_bank.py") `
    --source $sourceRoot `
    --model $modelRoot `
    --output $voiceBankRoot `
    --device cuda:0

Write-Host "Spark-TTS and the 32-profile Mandarin voice bank are ready."
