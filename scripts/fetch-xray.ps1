# Fetch a pinned Xray-core Windows amd64 build into the Tauri sidecar path.
#
# Pin: v26.9.9 (XTLS/Xray-core, pre-release). Override with -Version or
# $env:XRAY_VERSION (digits only, e.g. 26.9.9).
#
# Why a pre-release: no stable tag since v26.3.27 has a usable TUN. The
# TUN config surface this app emits — gateway, dns, autoSystemRoutingTable,
# autoOutboundsInterface, desc, and lowercase mtu — only exists from
# v26.5.9. v26.3.27 parsed name / MTU / userLevel and silently ignored
# the rest (Go drops unknown JSON keys), so `xray run -test` exiting 0
# never proved those fields were honoured.
#
# v26.9.9 also clears the server-side minClientVer floor of 26.3.27 with
# margin. If this pin misbehaves, the documented fallback is v26.7.28.
#
# The Windows zip ships xray.exe, wintun.dll, geoip.dat and geosite.dat.
# A sibling .dgst file is verified (SHA2-256) before extract.
#
# Usage:
#   .\scripts\fetch-xray.ps1
#   .\scripts\fetch-xray.ps1 -Version 26.9.9

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
$destDir = Join-Path $repoRoot "src-tauri\binaries"
$destName = "xray-x86_64-pc-windows-msvc.exe"
$destPath = Join-Path $destDir $destName
$assetName = "Xray-windows-64.zip"
$downloadUrl = "https://github.com/XTLS/Xray-core/releases/download/v$version/$assetName"
$dgstUrl = "$downloadUrl.dgst"

New-Item -ItemType Directory -Force -Path $destDir | Out-Null

$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("xraybox-xray-" + [guid]::NewGuid().ToString("N"))
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

    $required = @{
        "xray.exe"   = $destPath
        "wintun.dll" = Join-Path $destDir "wintun.dll"
        "geoip.dat"  = Join-Path $destDir "geoip.dat"
        "geosite.dat"= Join-Path $destDir "geosite.dat"
    }

    foreach ($name in $required.Keys) {
        $found = Get-ChildItem -Path $extractDir -Filter $name -Recurse | Select-Object -First 1
        if (-not $found) {
            throw "$name not found inside $assetName"
        }
        Copy-Item -Path $found.FullName -Destination $required[$name] -Force
        Write-Host "Wrote $($required[$name])"
    }

    # Loyalsoldier builds carry the extra catalog categories (youtube, gmail, …).
    # Seed them into resources/assets/ for bundled_geo_dirs / tauri.conf.json.
    $assetsDir = Join-Path $repoRoot "src-tauri\resources\assets"
    New-Item -ItemType Directory -Force -Path $assetsDir | Out-Null
    $geoUrls = @{
        "geosite.dat" = "https://github.com/Loyalsoldier/v2ray-rules-dat/releases/latest/download/geosite.dat"
        "geoip.dat"   = "https://github.com/Loyalsoldier/v2ray-rules-dat/releases/latest/download/geoip.dat"
    }
    foreach ($name in $geoUrls.Keys) {
        $dest = Join-Path $assetsDir $name
        Write-Host "Downloading $($geoUrls[$name])"
        Invoke-WebRequest -Uri $geoUrls[$name] -OutFile $dest -UseBasicParsing
        if (-not (Test-Path $dest) -or (Get-Item $dest).Length -eq 0) {
            throw "Loyalsoldier $name download produced an empty file"
        }
        Write-Host "Wrote $dest"
    }
}
finally {
    Remove-Item -Path $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}
