param([switch]$DecoderOnly)
$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath $PSScriptRoot
$remaster = Join-Path (Split-Path $PSScriptRoot) 'Remaster'
if (-not $DecoderOnly) {
    cargo build --locked --offline --release -j 6
    if ($LASTEXITCODE -ne 0) { throw 'Rust build failed. For a first build, run cargo fetch --locked with network access.' }
    Copy-Item -LiteralPath 'target/release/EAGL-Workbench.exe' -Destination 'EAGL-Workbench.exe' -Force
}
$env:PYINSTALLER_CONFIG_DIR = Join-Path $PSScriptRoot 'build/pyinstaller-cache'
$env:TEMP = Join-Path $PSScriptRoot 'build/temp'
$env:TMP = $env:TEMP
$env:PYTHONPATH = Join-Path $remaster 'build-deps'
New-Item -ItemType Directory -Force -Path $env:TEMP | Out-Null
py -3.14 -m PyInstaller --noconfirm --onedir --console --contents-directory _decoder_internal --name EAGL-Decoder --distpath build/package --workpath build/decoder --specpath build --paths "$remaster/src" --paths "$remaster/src/legacy" --add-data "$remaster/src/legacy/eagl_layouts.json;legacy" --exclude-module PIL --exclude-module customtkinter --exclude-module tkinter tools/decoder_bridge.py
if ($LASTEXITCODE -ne 0) { throw 'Decoder packaging failed' }
Copy-Item -LiteralPath 'build/package/EAGL-Decoder/EAGL-Decoder.exe' -Destination 'EAGL-Decoder.exe' -Force
Copy-Item -LiteralPath 'build/package/EAGL-Decoder/_decoder_internal' -Destination '.' -Recurse -Force
Copy-Item -LiteralPath "$remaster/reference" -Destination '.' -Recurse -Force
New-Item -ItemType Directory -Force -Path 'research' | Out-Null
Copy-Item -LiteralPath "$remaster/research/coverage.json" -Destination 'research/coverage.json' -Force
Write-Host 'Built native viewer and/or packaged decoder in' $PSScriptRoot
