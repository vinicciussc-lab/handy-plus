# Handy Plus: downloads the pinned llama.cpp release (Vulkan build, Windows x64)
# and extracts it to src-tauri/resources/llama-server/. Idempotent.
# Usage: powershell -ExecutionPolicy Bypass -File scripts\fetch-llama-server.ps1
$ErrorActionPreference = 'Stop'
$Tag = 'b12233'
$Asset = "llama-$Tag-bin-win-vulkan-x64.zip"
$Url = "https://github.com/ggml-org/llama.cpp/releases/download/$Tag/$Asset"
$Dest = Join-Path $PSScriptRoot '..\src-tauri\resources\llama-server'
$Exe = Join-Path $Dest 'llama-server.exe'

if (Test-Path $Exe) {
    Write-Host "llama-server.exe already present ($Dest); skipping."
    exit 0
}

$Tmp = Join-Path ([System.IO.Path]::GetTempPath()) $Asset
Write-Host "Downloading $Url"
Invoke-WebRequest -Uri $Url -OutFile $Tmp -UseBasicParsing
New-Item -ItemType Directory -Force -Path $Dest | Out-Null
Expand-Archive -Path $Tmp -DestinationPath $Dest -Force
Remove-Item $Tmp -Force

if (-not (Test-Path $Exe)) { throw "llama-server.exe not found after extraction in $Dest" }
Write-Host "Done: $Exe"
