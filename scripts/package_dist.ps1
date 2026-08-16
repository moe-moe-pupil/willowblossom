[CmdletBinding()]
param(
    [string]$Version,
    [string]$UpdateLogPath,
    [string]$BuildTargetDir,
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repoRoot = Split-Path -Parent $PSScriptRoot
$distDir = Join-Path $repoRoot ".dist"
$manifestPath = Join-Path $repoRoot "Cargo.toml"

if (-not (Test-Path -LiteralPath $distDir -PathType Container)) {
    throw "Distribution directory does not exist: $distDir"
}

if ([string]::IsNullOrWhiteSpace($Version)) {
    $manifest = Get-Content -LiteralPath $manifestPath -Raw
    $packageSection = [regex]::Match($manifest, '(?ms)^\[package\]\s*(.*?)(?=^\[|\z)')
    $versionMatch = [regex]::Match($packageSection.Groups[1].Value, '(?m)^version\s*=\s*"([^"]+)"')
    if (-not $versionMatch.Success) {
        throw "Could not read the package version from Cargo.toml."
    }
    $Version = $versionMatch.Groups[1].Value
}

if ($Version -notmatch '^\d+\.\d+\.\d+([-.][0-9A-Za-z.-]+)?$') {
    throw "Invalid release version: $Version"
}

if ([string]::IsNullOrWhiteSpace($UpdateLogPath)) {
    $UpdateLogPath = Join-Path $repoRoot "docs\releases\$Version.zh-CN.txt"
} elseif (-not [System.IO.Path]::IsPathRooted($UpdateLogPath)) {
    $UpdateLogPath = Join-Path $repoRoot $UpdateLogPath
}

if (-not (Test-Path -LiteralPath $UpdateLogPath -PathType Leaf)) {
    throw "Chinese update log does not exist: $UpdateLogPath"
}

if ([string]::IsNullOrWhiteSpace($BuildTargetDir)) {
    $BuildTargetDir = Join-Path $repoRoot "target"
} elseif (-not [System.IO.Path]::IsPathRooted($BuildTargetDir)) {
    $BuildTargetDir = Join-Path $repoRoot $BuildTargetDir
}

if (-not $SkipBuild) {
    Push-Location $repoRoot
    try {
        & cargo build --target-dir $BuildTargetDir
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build failed with exit code $LASTEXITCODE."
        }
    } finally {
        Pop-Location
    }
}

$builtExe = Join-Path $BuildTargetDir "debug\willowblossom.exe"
if (-not (Test-Path -LiteralPath $builtExe -PathType Leaf)) {
    throw "Built executable was not found: $builtExe"
}

Copy-Item -LiteralPath $builtExe -Destination (Join-Path $distDir "willowblossom.exe") -Force
Copy-Item -LiteralPath $UpdateLogPath -Destination (Join-Path $distDir "更新日志.txt") -Force

$archivePath = Join-Path $distDir "柳絮$Version.zip"
$stagingDir = Join-Path ([System.IO.Path]::GetTempPath()) ("willowblossom-dist-" + [guid]::NewGuid().ToString("N"))

try {
    New-Item -ItemType Directory -Path $stagingDir | Out-Null
    Get-ChildItem -LiteralPath $distDir -Force | Where-Object {
        -not ($_.Name -like "柳絮*.zip")
    } | Copy-Item -Destination $stagingDir -Recurse -Force

    if (Test-Path -LiteralPath $archivePath) {
        Remove-Item -LiteralPath $archivePath -Force
    }

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [System.IO.Compression.ZipFile]::CreateFromDirectory(
        $stagingDir,
        $archivePath,
        [System.IO.Compression.CompressionLevel]::Optimal,
        $false
    )
} finally {
    if (Test-Path -LiteralPath $stagingDir) {
        Remove-Item -LiteralPath $stagingDir -Recurse -Force
    }
}

$exeInfo = Get-Item -LiteralPath (Join-Path $distDir "willowblossom.exe")
$archiveInfo = Get-Item -LiteralPath $archivePath
Write-Host "Built executable: $($exeInfo.FullName) ($($exeInfo.Length) bytes)"
Write-Host "Created archive:  $($archiveInfo.FullName) ($($archiveInfo.Length) bytes)"
