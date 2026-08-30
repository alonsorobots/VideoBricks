# VideoBricks

Convert videos to high-quality GIFs and optimized MP4s with a multi-segment timeline editor and AI-powered shot detection.

<div align="center">

[![Watch the demo](https://img.youtube.com/vi/rJ6XCk_fB5Y/maxresdefault.jpg)](https://youtu.be/rJ6XCk_fB5Y)

**[Watch the Demo Video](https://youtu.be/rJ6XCk_fB5Y)**

</div>

## Features

- **High-quality GIF encoding** powered by the [gifski](https://github.com/ImageOptim/gifski) encoder
- **MP4 export** with FFmpeg for optimized video output
- **Multi-segment timeline** -- create multiple trim segments on a single video, then choose to merge them into one file or export each segment as an individual file
- **Precision trim controls** -- drag a trim handle to roughly position it, then hold still to enter a detail mode that zooms into that region of the timeline, letting you land on the exact frame you want
- **AI shot detection** -- automatically find scene boundaries using [TransNetV2](https://github.com/soCzech/TransNetV2) and split your video into clips with one click (model is bundled -- no setup required)
- **Crop and resize** with aspect ratio presets and freeform editing
- **Real-time preview** with loop and bounce playback modes
- **Adjustable FPS, speed, and quality** controls
- **Estimated output size** shown before conversion

---

## Installation

### macOS

The easiest way to install is via [Homebrew](https://brew.sh/). This handles everything -- no scary warnings, no extra steps:

```bash
brew install --cask alonsorobots/tap/videobricks
```

This installs VideoBricks and its FFmpeg dependency automatically. To update later, run `brew upgrade --cask videobricks`.

<details>
<summary>Alternative: manual install from DMG</summary>

Download `VideoBricks_x.x.x_aarch64.dmg` from the [GitHub Releases](../../releases) page, open it, and drag VideoBricks to Applications.

Because the app is not signed with an Apple Developer certificate, macOS Gatekeeper will block the first launch. To allow it:

- **Option A**: Right-click (or Control-click) the app > **Open** > click **Open** in the dialog.
- **Option B**: Run this in Terminal after copying to Applications:

```bash
xattr -cr /Applications/VideoBricks.app
```

You also need FFmpeg installed separately:

```bash
brew install ffmpeg
```

</details>

### Windows

Download the `.exe` (NSIS) or `.msi` installer from the [GitHub Releases](../../releases) page. Everything is bundled -- FFmpeg, the TransNetV2 AI model, and (since 1.0.4) a self-contained Python runtime for shot detection. No separate downloads or setup required.

---

### For Developers

If you want to build from source or contribute:

#### Prerequisites

- [Rust](https://rustup.rs/) 1.77+
- [Node.js](https://nodejs.org/) 18+
- [FFmpeg & FFprobe](https://ffmpeg.org/) binaries
- Python 3.9+ with `onnxruntime`, `numpy`, and `ffmpeg-python` (for AI shot detection)

#### Setup

```bash
# Clone the repository
git clone https://github.com/AloAlto/VideoBricks.git
cd VideoBricks

# Install frontend dependencies
npm install

# Place FFmpeg binaries (not committed to the repo due to size ~190 MB each)
#
# Windows: download from https://www.gyan.dev/ffmpeg/builds/
mkdir -p src-tauri/binaries
cp /path/to/ffmpeg.exe src-tauri/binaries/
cp /path/to/ffprobe.exe src-tauri/binaries/
#
# macOS: install via Homebrew (no need to copy binaries -- found via PATH)
brew install ffmpeg

# Vendor the self-contained Python runtime that ships with the Windows build
# (downloads embeddable CPython + onnxruntime/numpy/ffmpeg-python, ~95 MB).
# Not committed to the repo; only needed for Windows release builds.
pwsh -File src-tauri/scripts/vendor-python.ps1

# Alternatively, for development, use your own Python for shot detection.
# The app falls back to system Python (then conda) when no bundled runtime
# is present.
pip install -r src-tauri/scripts/requirements.txt
```

#### Run & Build

```bash
# Development mode (hot-reload frontend + Rust backend)
npm run tauri dev

# Production build (creates installer)
npm run tauri build
# Output: src-tauri/target/release/bundle/
```

---

## Project Structure

```
VideoBricks/
  src/                    # React + TypeScript frontend
  src-tauri/
    src/                  # Rust backend (Tauri commands, gifski bridge, FFmpeg)
    binaries/             # FFmpeg/FFprobe executables (not committed)
    scripts/
      transnet_detect.py      # Shot detection script (ONNX or PyTorch)
      transnetv2.onnx         # Pre-exported ONNX model (~30 MB, committed)
      export_transnet_onnx.py # One-time PyTorch-to-ONNX export tool
      requirements.txt        # Python deps for shot detection
    icons/                # App icons
    Cargo.toml            # Rust dependencies
    tauri.conf.json       # Tauri config (window, bundle, resources)
  dist/                   # Built frontend (generated)
  package.json            # Node dependencies and scripts
  vite.config.ts          # Vite + Tailwind CSS config
```

---

## Credits

VideoBricks builds on the work of several open-source projects:

- **[gifski](https://github.com/ImageOptim/gifski)** by Kornel Lesinski -- high-quality GIF encoder (AGPL-3.0)
- **[Gifski for macOS](https://github.com/sindresorhus/Gifski)** by Sindre Sorhus -- the original inspiration
- **[FFmpeg](https://ffmpeg.org/)** -- video processing (LGPL-2.1+)
- **[TransNetV2](https://github.com/soCzech/TransNetV2)** by Tomas Soucek -- shot boundary detection (MIT)
- **[Tauri](https://tauri.app/)** -- desktop application framework (MIT / Apache-2.0)
- **[React](https://react.dev/)** -- UI framework (MIT)

## License

This project is licensed under the [AGPL-3.0](LICENSE) license, as required by its use of the gifski encoder library.

You are free to use, modify, and distribute this software. If you distribute modified versions, you must make the source code available under the same license.

## Bugs / Feature Requests

Found a bug or have an idea? Email [alonsorobots@gmail.com](mailto:alonsorobots@gmail.com).

## Support

If you find VideoBricks useful, consider [buying me a coffee](https://buymeacoffee.com/alonsorobots).
