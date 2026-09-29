$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath $PSScriptRoot
# Keep build caches and temporary files in this project.
$env:PYINSTALLER_CONFIG_DIR = Join-Path $PSScriptRoot 'build\pyinstaller-cache'
$env:TEMP = Join-Path $PSScriptRoot 'build\temp'
$env:TMP = $env:TEMP
New-Item -ItemType Directory -Force -Path $env:TEMP | Out-Null
# This machine's PyInstaller install lacks its packaging dependency. Reuse
# pip's licensed vendored copy locally; do not alter the global Python install.
$env:PYTHONPATH = Join-Path $PSScriptRoot 'build-deps'
py -3.14 -c "import importlib.util,pathlib,shutil; dst=pathlib.Path('build-deps/packaging'); dst.parent.mkdir(exist_ok=True); spec=importlib.util.find_spec('packaging'); import pip._vendor.packaging as vendored; shutil.copytree(pathlib.Path(vendored.__file__).parent,dst,dirs_exist_ok=True) if spec is None else None"
py -3.14 -m PyInstaller --noconfirm --clean --onedir --windowed --name EAGL-Remaster --distpath build/package --workpath build --specpath build --paths src --paths src/legacy --add-data "$PSScriptRoot/src/legacy/eagl_layouts.json;legacy" --exclude-module PIL --exclude-module customtkinter src/app.py
if ($LASTEXITCODE -ne 0) { throw 'PyInstaller build failed' }
Copy-Item -LiteralPath 'build/package/EAGL-Remaster/EAGL-Remaster.exe' -Destination 'EAGL-Remaster.exe' -Force
Copy-Item -LiteralPath 'build/package/EAGL-Remaster/_internal' -Destination '.' -Recurse -Force
py -3.14 -m PyInstaller --noconfirm --clean --onedir --console --contents-directory _cli_internal --name EAGL-CLI --distpath build/package --workpath build --specpath build --paths src --paths src/legacy --add-data "$PSScriptRoot/src/legacy/eagl_layouts.json;legacy" --exclude-module PIL --exclude-module customtkinter --exclude-module tkinter src/cli.py
if ($LASTEXITCODE -ne 0) { throw 'Headless PyInstaller build failed' }
Copy-Item -LiteralPath 'build/package/EAGL-CLI/EAGL-CLI.exe' -Destination 'EAGL-CLI.exe' -Force
Copy-Item -LiteralPath 'build/package/EAGL-CLI/_cli_internal' -Destination '.' -Recurse -Force
Write-Host "Built $PSScriptRoot\EAGL-Remaster.exe"
Write-Host "Built $PSScriptRoot\EAGL-CLI.exe"
