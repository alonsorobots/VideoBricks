# Builds the self-contained Python runtime that ships with the Windows app,
# so shot detection works without the user installing anything.
#
# Produces src-tauri/python/ (~95 MB), which is gitignored and bundled as a
# Tauri resource by tauri.windows.conf.json.
#
# Usage:  pwsh -File src-tauri/scripts/vendor-python.ps1

$ErrorActionPreference = "Stop"

$PyVersion = "3.11.9"
$Root      = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)   # repo root
$Dest      = Join-Path $Root "src-tauri\python"
$Work      = Join-Path ([System.IO.Path]::GetTempPath()) "videobricks-pyembed"

Write-Host "Vendoring Python $PyVersion into $Dest"

if (Test-Path $Work) { Remove-Item -Recurse -Force $Work }
New-Item -ItemType Directory -Path $Work | Out-Null

# 1. Embeddable CPython (no installer, no registry, fully relocatable)
$Zip = Join-Path $Work "python-embed.zip"
Invoke-WebRequest -Uri "https://www.python.org/ftp/python/$PyVersion/python-$PyVersion-embed-amd64.zip" -OutFile $Zip
Expand-Archive -Path $Zip -DestinationPath $Work -Force
Remove-Item $Zip

# 2. The embeddable distro ships a restricted sys.path; enable site-packages.
$Major = ($PyVersion -split '\.')[0..1] -join ''
@"
python$Major.zip
.
Lib\site-packages

# Uncommented to allow site-packages imports
import site
"@ | Set-Content -Path (Join-Path $Work "python$Major._pth") -Encoding ASCII

# 3. Shot-detection dependencies, as wheels for the embedded interpreter.
python -m pip install --target (Join-Path $Work "Lib\site-packages") `
  --only-binary=:all: --platform win_amd64 --python-version 3.11 `
  "onnxruntime>=1.17.0" "numpy>=1.24.0" "ffmpeg-python>=0.2.0"

# 4. Drop what the shot-detection code path never reaches (~40 MB).
$SP = Join-Path $Work "Lib\site-packages"
Get-ChildItem -Path (Join-Path $SP "numpy") -Recurse -Directory -Filter tests |
  ForEach-Object { Remove-Item -Recurse -Force $_.FullName }
foreach ($p in @(
    "numpy\_core\include", "numpy\_core\lib", "numpy\f2py",
    "onnxruntime\training", "onnxruntime\transformers",
    "onnxruntime\quantization", "onnxruntime\tools", "bin")) {
  $full = Join-Path $SP $p
  if (Test-Path $full) { Remove-Item -Recurse -Force $full }
}
Get-ChildItem -Path $Work -Recurse -Directory -Filter __pycache__ |
  ForEach-Object { Remove-Item -Recurse -Force $_.FullName }

# 5. Verify before installing it into the repo.
& (Join-Path $Work "python.exe") -c "import onnxruntime, numpy, ffmpeg; print('deps OK')"
if ($LASTEXITCODE -ne 0) { throw "Vendored runtime failed its import check" }

if (Test-Path $Dest) { Remove-Item -Recurse -Force $Dest }
Move-Item $Work $Dest

$Size = [math]::Round((Get-ChildItem $Dest -Recurse | Measure-Object Length -Sum).Sum / 1MB)
Write-Host "Done: $Dest ($Size MB)"
