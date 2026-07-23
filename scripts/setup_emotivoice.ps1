$ErrorActionPreference = "Stop"
$env:http_proxy = "http://127.0.0.1:10809"
$env:https_proxy = "http://127.0.0.1:10809"

$repoRoot = Split-Path -Parent $PSScriptRoot
$runtimeRoot = Join-Path $repoRoot ".data\willowblossom\tts\emotivoice"
$sourceRoot = Join-Path $runtimeRoot "EmotiVoice"
$venvRoot = Join-Path $runtimeRoot ".venv313"
$venvPython = Join-Path $venvRoot "Scripts\python.exe"

$cudaPython = (Get-Command python -ErrorAction Stop).Source
& $cudaPython -c "import torch; assert torch.cuda.is_available(), 'CUDA PyTorch is required'"

New-Item -ItemType Directory -Force -Path $runtimeRoot | Out-Null
if (-not (Test-Path -LiteralPath (Join-Path $sourceRoot ".git"))) {
    git clone --depth 1 "https://github.com/netease-youdao/EmotiVoice.git" $sourceRoot
}
if (-not (Test-Path -LiteralPath (Join-Path $sourceRoot "outputs\.git"))) {
    git clone "https://www.modelscope.cn/syq163/outputs.git" (Join-Path $sourceRoot "outputs")
}
$bertRoot = Join-Path $sourceRoot "WangZeJun\simbert-base-chinese"
if (-not (Test-Path -LiteralPath (Join-Path $bertRoot ".git"))) {
    New-Item -ItemType Directory -Force -Path (Split-Path $bertRoot -Parent) | Out-Null
    git clone "https://www.modelscope.cn/syq163/WangZeJun.git" $bertRoot
}
if (-not (Test-Path -LiteralPath $venvPython)) {
    & $cudaPython -m venv --system-site-packages $venvRoot
}
& $venvPython -m pip install --upgrade pip wheel "setuptools<81"
& $venvPython -m pip install -r (Join-Path $PSScriptRoot "requirements-emotivoice.txt")

$env:EMOTIVOICE_SOURCE = $sourceRoot
'{"text":"\u4f60\u597d\uff0c\u5341\u4e2a\u89d2\u8272\u5df2\u7ecf\u51c6\u5907\u597d\u4e86\u3002","speaker":"9000","emotion":"\u666e\u901a","speed":1.0,"output_path":"smoke.wav"}' |
    & $venvPython (Join-Path $PSScriptRoot "emotivoice_worker.py")
