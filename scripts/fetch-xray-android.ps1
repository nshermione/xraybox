# Fetch a pinned Xray-core Android arm64 build into jniLibs as libxray.so.
#
# Pin: v26.9.9 (XTLS/Xray-core, pre-release). Override with -Version or
# $env:XRAY_VERSION (digits only, e.g. 26.9.9).
#
# Android 10+ refuses exec from the app files dir. The binary is disguised
# as libxray.so and extracted to nativeLibraryDir via useLegacyPackaging.
#
# Usage:
#   .\scripts\fetch-xray-android.ps1
#   .\scripts\fetch-xray-android.ps1 -Version 26.9.9

[CmdletBinding()]
param(
    [string]$Version = $(
        if ($env:XRAY_VERSION) { $env:XRAY_VERSION } else { "26.9.9" }
    )
)

$ErrorActionPreference = "Stop"

$version = $Version.TrimStart("v")
if ($version -notmatch "^\d+\.\d+\.\d+") {
    throw "XRAY_VERSION must look like 26.3.27, got '$Version'"
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$destDir = Join-Path $repoRoot "src-tauri\gen\android\app\src\main\jniLibs\arm64-v8a"
$destPath = Join-Path $destDir "libxray.so"
$assetName = "Xray-android-arm64-v8a.zip"
$downloadUrl = "https://github.com/XTLS/Xray-core/releases/download/v$version/$assetName"
$dgstUrl = "$downloadUrl.dgst"

New-Item -ItemType Directory -Force -Path $destDir | Out-Null

$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("xraybox-xray-android-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $tempRoot | Out-Null

try {
    $zipPath = Join-Path $tempRoot $assetName
    Write-Host "Downloading $downloadUrl"
    Invoke-WebRequest -Uri $downloadUrl -OutFile $zipPath -UseBasicParsing

    $dgstPath = Join-Path $tempRoot "$assetName.dgst"
    Write-Host "Downloading $dgstUrl"
    Invoke-WebRequest -Uri $dgstUrl -OutFile $dgstPath -UseBasicParsing

    $dgstLine = Get-Content -Path $dgstPath | Where-Object { $_ -match "SHA2-256=" } | Select-Object -First 1
    if (-not $dgstLine) {
        throw "SHA2-256= line not found in $assetName.dgst"
    }
    $expected = (($dgstLine -split "SHA2-256=", 2)[1]).Trim() -replace "\s", ""
    if (-not $expected) {
        throw "SHA2-256= line in $assetName.dgst has no hash"
    }
    $actual = (Get-FileHash -Path $zipPath -Algorithm SHA256).Hash
    if ($actual -ne $expected) {
        throw "SHA256 mismatch for $assetName`: expected $expected, got $actual"
    }
    Write-Host "SHA2-256 verified"

    $extractDir = Join-Path $tempRoot "extract"
    Expand-Archive -Path $zipPath -DestinationPath $extractDir -Force

    $found = Get-ChildItem -Path $extractDir -Filter "xray" -Recurse -File | Select-Object -First 1
    if (-not $found) {
        throw "xray binary not found inside $assetName"
    }
    Copy-Item -Path $found.FullName -Destination $destPath -Force
    Write-Host "Wrote $destPath"
}
finally {
    Remove-Item -Path $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}
