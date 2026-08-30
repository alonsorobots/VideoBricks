# Downloads the FFmpeg binaries the Windows app bundles.
#
# They are ~200 MB each and gitignored, so CI and fresh clones need this before
# a release build. macOS gets FFmpeg from Homebrew instead.
#
# Usage:  pwsh -File src-tauri/scripts/vendor-ffmpeg.ps1

$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$Dest = Join-Path $Root "src-tauri\binaries"
$Work = Join-Path ([System.IO.Path]::GetTempPath()) "videobricks-ffmpeg"

if ((Test-Path (Join-Path $Dest "ffmpeg.exe")) -and (Test-Path (Join-Path $Dest "ffprobe.exe"))) {
  Write-Host "FFmpeg binaries already present in $Dest -- skipping"
  exit 0
}

if (Test-Path $Work) { Remove-Item -Recurse -Force $Work }
New-Item -ItemType Directory -Path $Work | Out-Null
New-Item -ItemType Directory -Path $Dest -Force | Out-Null

$Archive = Join-Path $Work "ffmpeg-release-full.7z"
Write-Host "Downloading FFmpeg (full build) from gyan.dev..."
Invoke-WebRequest -Uri "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-full.7z" -OutFile $Archive

$SevenZip = (Get-Command 7z -ErrorAction SilentlyContinue).Source
if (-not $SevenZip) { $SevenZip = "C:\Program Files\7-Zip\7z.exe" }
if (-not (Test-Path $SevenZip)) { throw "7-Zip is required to extract the FFmpeg archive" }

& $SevenZip x -y -o"$Work" $Archive | Out-Null

foreach ($exe in @("ffmpeg.exe", "ffprobe.exe")) {
  $found = Get-ChildItem -Path $Work -Recurse -Filter $exe | Select-Object -First 1
  if (-not $found) { throw "$exe not found in the downloaded archive" }
  Copy-Item $found.FullName (Join-Path $Dest $exe) -Force
}

Remove-Item -Recurse -Force $Work

& (Join-Path $Dest "ffmpeg.exe") -version | Select-Object -First 1
Write-Host "Done: $Dest"
