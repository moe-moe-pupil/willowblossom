$ErrorActionPreference = "Stop"
$env:http_proxy = "http://127.0.0.1:10809"
$env:https_proxy = "http://127.0.0.1:10809"

$repoRoot = Split-Path -Parent $PSScriptRoot
$runtimeRoot = Join-Path $repoRoot ".data\willowblossom\tts\melotts"
$pythonRoot = Join-Path $runtimeRoot "python"
$venvRoot = Join-Path $runtimeRoot ".venv"
$installer = Join-Path $runtimeRoot "python-3.10.11-amd64.exe"
$python = Join-Path $pythonRoot "python.exe"
$venvPython = Join-Path $venvRoot "Scripts\python.exe"
$sourceRoot = Join-Path $runtimeRoot "MeloTTS"

New-Item -ItemType Directory -Force -Path $runtimeRoot | Out-Null

if (-not (Test-Path -LiteralPath $python)) {
    Invoke-WebRequest `
        -Uri "https://www.python.org/ftp/python/3.10.11/python-3.10.11-amd64.exe" `
        -OutFile $installer
    $install = Start-Process -FilePath $installer -Wait -PassThru -ArgumentList @(
        "/quiet",
        "InstallAllUsers=0",
        "Include_launcher=0",
        "Include_test=0",
        "PrependPath=0",
        "Shortcuts=0",
        "TargetDir=$pythonRoot"
    )
    if ($install.ExitCode -ne 0) {
        throw "Python installer failed with exit code $($install.ExitCode)"
    }
}

if (-not (Test-Path -LiteralPath $venvPython)) {
    & $python -m venv $venvRoot
}

& $venvPython -m pip install --upgrade pip wheel "setuptools<81"
& $venvPython -m pip install `
    --index-url "https://download.pytorch.org/whl/cpu" `
    "torch==2.2.2+cpu" `
    "torchaudio==2.2.2+cpu"
& $venvPython -m pip install -r (Join-Path $PSScriptRoot "requirements-melotts.txt")
if (-not (Test-Path -LiteralPath (Join-Path $sourceRoot "melo\api.py"))) {
    git clone --depth 1 "https://github.com/myshell-ai/MeloTTS.git" $sourceRoot
}
& $venvPython (Join-Path $PSScriptRoot "prepare_melotts_chinese.py") $sourceRoot

$env:HF_HOME = Join-Path $runtimeRoot "huggingface"
$env:PYTHONPATH = $sourceRoot
& $venvPython -c "from melo.api import TTS; m=TTS(language='ZH', device='cpu'); print('MeloTTS ZH ready:', m.hps.data.spk2id)"
