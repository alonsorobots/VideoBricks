# VideoBricks

Convert videos to high-quality GIFs and optimized MP4s with a multi-segment timeline editor and AI-powered shot detection.

<!-- ![VideoBricks Screenshot](screenshot.png) -->

## Features

- **High-quality GIF encoding** powered by the [gifski](https://github.com/ImageOptim/gifski) encoder
- **MP4 export** with FFmpeg for optimized video output
- **Multi-segment timeline** -- create, merge, delete, activate/deactivate trim segments
- **AI shot detection** -- automatically find scene boundaries using [TransNetV2](https://github.com/soCzech/TransNetV2) (optional)
- **Crop and resize** with aspect ratio presets and freeform editing
- **Real-time preview** with loop and bounce playback modes
- **Adjustable FPS, speed, and quality** controls
- **Estimated output size** shown before conversion

---

## Installation

### For End Users (Recommended)

Download the latest installer from the [GitHub Releases](../../releases) page:

| Platform | Format |
|----------|--------|
| Windows  | `.exe` (NSIS) or `.msi` |

The installer includes everything you need to convert videos to GIFs and MP4s. FFmpeg is bundled -- no separate download required.

> **Note:** AI shot detection (TransNetV2) is an optional add-on that requires a separate setup. See [TransNetV2 Setup](#transnetv2-setup-optional) below. VideoBricks works perfectly without it -- you can always place trim segments manually.

### For Developers

If you want to build from source or contribute:

#### Prerequisites

- [Rust](https://rustup.rs/) 1.77+
- [Node.js](https://nodejs.org/) 18+
- [FFmpeg & FFprobe](https://ffmpeg.org/) binaries

#### Setup

```bash
# Clone the repository
git clone https://github.com/AloAlto/VideoBricks.git
cd VideoBricks

# Install frontend dependencies
npm install

# Place FFmpeg binaries (not committed to the repo)
# Download from https://www.gyan.dev/ffmpeg/builds/ (Windows)
# Copy ffmpeg.exe and ffprobe.exe into src-tauri/binaries/
mkdir -p src-tauri/binaries
cp /path/to/ffmpeg.exe src-tauri/binaries/
cp /path/to/ffprobe.exe src-tauri/binaries/
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

## TransNetV2 Setup (Optional)

TransNetV2 adds automatic scene/shot boundary detection, letting you split a video into clips with one click. It runs locally on your machine using PyTorch -- no cloud services or API keys needed.

**This is completely optional.** VideoBricks works fine without it; you just place trim segments manually instead.

### Why it isn't bundled

TransNetV2 depends on PyTorch, which weighs in at ~2 GB. Bundling it would bloat the installer from ~80 MB to well over 2 GB. Keeping it separate means most users get a fast, lightweight install, and those who want AI shot detection can opt in.

### Requirements

- [Conda](https://docs.conda.io/), [Miniforge](https://github.com/conda-forge/miniforge), or [Miniconda](https://docs.anaconda.com/miniconda/)
- ~3 GB disk space (PyTorch + model weights)
- A CPU is sufficient; GPU is not required

### Step-by-step

1. **Install Conda** (if you don't have it already).
   [Miniforge](https://github.com/conda-forge/miniforge#miniforge3) is recommended -- it's lightweight and uses conda-forge by default.

2. **Clone TransNetV2** somewhere on your machine:
   ```bash
   cd ~
   git clone https://github.com/soCzech/TransNetV2.git
   ```

3. **Download the PyTorch weights** into the inference directory:
   ```bash
   cd TransNetV2/inference-pytorch
   # Download transnetv2-pytorch-weights.pth from the TransNetV2 releases
   # or from: https://github.com/soCzech/TransNetV2/releases
   ```

4. **Create the conda environment** that VideoBricks expects:
   ```bash
   conda create -n yt_filter python=3.10 -y
   conda activate yt_filter
   pip install torch numpy ffmpeg-python
   ```

5. **Tell VideoBricks where to find TransNetV2** by setting the `TRANSNETV2_DIR` environment variable to the `inference-pytorch` folder:

   **Windows (PowerShell):**
   ```powershell
   [System.Environment]::SetEnvironmentVariable("TRANSNETV2_DIR", "$HOME\TransNetV2\inference-pytorch", "User")
   ```
   **Windows (Command Prompt):**
   ```cmd
   setx TRANSNETV2_DIR "%USERPROFILE%\TransNetV2\inference-pytorch"
   ```
   **Git Bash / Linux / macOS:**
   ```bash
   echo 'export TRANSNETV2_DIR="$HOME/TransNetV2/inference-pytorch"' >> ~/.bashrc
   source ~/.bashrc
   ```

   > If you skip this step, VideoBricks will look for TransNetV2 at `~/TransNetV2/inference-pytorch` automatically.

6. **Restart VideoBricks** and the "Find Shots" button should now work.

### Verifying the setup

You can test the pipeline directly from the command line:

```bash
conda activate yt_filter
python path/to/VideoBricks/src-tauri/scripts/transnet_detect.py "some_video.mp4"
```

You should see progress messages on stderr and a JSON array of scene boundaries on stdout.

---

## Project Structure

```
VideoBricks/
  src/                    # React + TypeScript frontend
  src-tauri/
    src/                  # Rust backend (Tauri commands, gifski bridge, FFmpeg)
    binaries/             # FFmpeg/FFprobe executables (not committed)
    scripts/              # transnet_detect.py (bundled with installer)
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
