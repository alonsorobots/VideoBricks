"""
TransNetV2 shot boundary detection script.

Preferred: uses ONNX Runtime (~60 MB) for inference -- no PyTorch required.
Fallback:  if no .onnx file is found, falls back to PyTorch inference.

Usage:
    python transnet_detect.py <video_path> [--threshold 0.35] [--model path/to/transnetv2.onnx]

Outputs JSON to stdout:
    [{"start": 0.0, "end": 3.5}, {"start": 3.5, "end": 8.2}, ...]
"""

import sys
import os
import json
import argparse
import subprocess
import numpy as np


# ---------------------------------------------------------------------------
# Locate resources
# ---------------------------------------------------------------------------

def _find_model_path():
    """Find the ONNX model file (transnetv2.onnx)."""
    script_dir = os.path.dirname(os.path.abspath(__file__))
    candidates = [
        os.path.join(script_dir, "transnetv2.onnx"),
        os.path.join(script_dir, "..", "models", "transnetv2.onnx"),
    ]
    for c in candidates:
        if os.path.isfile(c):
            return os.path.normpath(c)
    return None


def _find_transnet_dir():
    """Find the TransNetV2 inference-pytorch directory (PyTorch fallback)."""
    env_dir = os.environ.get("TRANSNETV2_DIR")
    if env_dir and os.path.isdir(env_dir):
        return os.path.normpath(env_dir)

    relative = os.path.normpath(os.path.join(
        os.path.dirname(os.path.abspath(__file__)),
        "..", "..", "..", "..",
        "TransNetV2", "inference-pytorch"
    ))
    if os.path.isdir(relative):
        return relative

    home = os.path.join(os.path.expanduser("~"), "TransNetV2", "inference-pytorch")
    if os.path.isdir(home):
        return os.path.normpath(home)

    return relative


# ---------------------------------------------------------------------------
# Video helpers
# ---------------------------------------------------------------------------

def get_video_fps(video_path):
    """Get the FPS of a video using ffprobe."""
    cmd = [
        "ffprobe", "-v", "quiet",
        "-select_streams", "v:0",
        "-show_entries", "stream=r_frame_rate",
        "-of", "csv=p=0",
        video_path
    ]
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, check=True,
                                creationflags=0x08000000 if sys.platform == "win32" else 0)
        fps_str = result.stdout.strip()
        if "/" in fps_str:
            num, den = fps_str.split("/")
            return float(num) / float(den)
        return float(fps_str)
    except Exception:
        return 30.0  # fallback


def extract_frames(video_path):
    """Extract all frames from video at 48x27 resolution using ffmpeg pipe."""
    import ffmpeg

    print("[TransNetV2] Extracting frames from {}".format(video_path), file=sys.stderr, flush=True)
    try:
        video_stream, _ = (
            ffmpeg.input(video_path)
            .output("pipe:", format="rawvideo", pix_fmt="rgb24", s="48x27")
            .run(capture_stdout=True, capture_stderr=True)
        )
    except ffmpeg.Error as e:
        print("[TransNetV2] FFmpeg error: {}".format(e.stderr.decode() if e.stderr else str(e)),
              file=sys.stderr)
        sys.exit(1)

    frames = np.frombuffer(video_stream, np.uint8).reshape([-1, 27, 48, 3])
    print("[TransNetV2] Extracted {} frames".format(len(frames)), file=sys.stderr, flush=True)
    return frames


# ---------------------------------------------------------------------------
# Sliding-window iterator (shared by both backends)
# ---------------------------------------------------------------------------

def sliding_window_iterator(frames):
    """Yield [1, 100, 27, 48, 3] uint8 windows with 25-frame padding."""
    no_padded_frames_start = 25
    no_padded_frames_end = 25 + 50 - (len(frames) % 50 if len(frames) % 50 != 0 else 50)

    start_frame = np.expand_dims(frames[0], 0)
    end_frame = np.expand_dims(frames[-1], 0)
    padded_inputs = np.concatenate(
        [start_frame] * no_padded_frames_start + [frames] + [end_frame] * no_padded_frames_end, 0
    )

    ptr = 0
    while ptr + 100 <= len(padded_inputs):
        out = padded_inputs[ptr:ptr + 100]
        ptr += 50
        yield out[np.newaxis]  # [1, 100, 27, 48, 3]


# ---------------------------------------------------------------------------
# ONNX Runtime inference (preferred -- lightweight, no PyTorch)
# ---------------------------------------------------------------------------

def predict_onnx(model_path, frames):
    """Run TransNetV2 ONNX model with onnxruntime."""
    import onnxruntime as ort

    print("[TransNetV2] Loading ONNX model from {}".format(model_path), file=sys.stderr, flush=True)
    sess = ort.InferenceSession(model_path, providers=["CPUExecutionProvider"])

    predictions = []
    for inp in sliding_window_iterator(frames):
        results = sess.run(None, {"frames": inp.astype(np.uint8)})
        single_frame_pred = 1.0 / (1.0 + np.exp(-results[0]))  # sigmoid
        predictions.append(single_frame_pred[0, 25:75, 0])

        print("[TransNetV2] Processing video frames {}/{}".format(
            min(len(predictions) * 50, len(frames)), len(frames)
        ), file=sys.stderr, flush=True)

    print("", file=sys.stderr, flush=True)
    single_frame_pred = np.concatenate(predictions)
    return single_frame_pred[:len(frames)]


# ---------------------------------------------------------------------------
# PyTorch inference (fallback)
# ---------------------------------------------------------------------------

def predict_pytorch(frames, weights_path=None):
    """Run TransNetV2 PyTorch inference."""
    transnet_dir = _find_transnet_dir()
    sys.path.insert(0, transnet_dir)

    try:
        from transnetv2_pytorch import TransNetV2
    except ImportError:
        print("[TransNetV2] ERROR: Could not import transnetv2_pytorch from {}".format(transnet_dir),
              file=sys.stderr)
        sys.exit(1)

    import torch

    if weights_path is None:
        weights_path = os.path.join(transnet_dir, "transnetv2-pytorch-weights.pth")
    if not os.path.isfile(weights_path):
        print("[TransNetV2] ERROR: Weights not found at {}".format(weights_path), file=sys.stderr)
        sys.exit(1)

    print("[TransNetV2] Loading PyTorch model from {}".format(weights_path), file=sys.stderr, flush=True)
    model = TransNetV2()
    state_dict = torch.load(weights_path, map_location="cpu", weights_only=True)
    model.load_state_dict(state_dict)
    model.eval()

    predictions = []
    with torch.no_grad():
        for inp in sliding_window_iterator(frames):
            single_frame_pred, _ = model(torch.from_numpy(inp))
            single_frame_pred = torch.sigmoid(single_frame_pred)
            predictions.append(single_frame_pred.numpy()[0, 25:75, 0])

            print("[TransNetV2] Processing video frames {}/{}".format(
                min(len(predictions) * 50, len(frames)), len(frames)
            ), file=sys.stderr, flush=True)

    print("", file=sys.stderr, flush=True)
    single_frame_pred = np.concatenate(predictions)
    return single_frame_pred[:len(frames)]


# ---------------------------------------------------------------------------
# Scene detection
# ---------------------------------------------------------------------------

def predictions_to_scenes(predictions, threshold=0.5):
    """Convert frame-level predictions to scene boundaries (frame indices)."""
    predictions = (predictions > threshold).astype(np.uint8)

    scenes = []
    t, t_prev, start = -1, 0, 0
    for i, t in enumerate(predictions):
        if t_prev == 1 and t == 0:
            start = i
        if t_prev == 0 and t == 1 and i != 0:
            scenes.append([start, i])
        t_prev = t
    if t == 0:
        scenes.append([start, i])

    if len(scenes) == 0:
        return np.array([[0, len(predictions) - 1]], dtype=np.int32)

    return np.array(scenes, dtype=np.int32)


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(description="TransNetV2 shot detection")
    parser.add_argument("video", type=str, help="Path to video file")
    parser.add_argument("--threshold", type=float, default=0.35,
                        help="Scene change threshold (default 0.35)")
    parser.add_argument("--model", type=str, default=None,
                        help="Path to .onnx model file (preferred) or .pth weights (fallback)")
    args = parser.parse_args()

    # Get video FPS
    fps = get_video_fps(args.video)
    print("[TransNetV2] Video FPS: {:.2f}".format(fps), file=sys.stderr, flush=True)

    # Extract frames
    frames = extract_frames(args.video)
    if len(frames) == 0:
        print("[TransNetV2] ERROR: No frames extracted", file=sys.stderr)
        sys.exit(1)

    # Choose backend: ONNX (preferred) or PyTorch (fallback)
    model_path = args.model or _find_model_path()

    if model_path and model_path.endswith(".onnx") and os.path.isfile(model_path):
        print("[TransNetV2] Using ONNX Runtime backend", file=sys.stderr, flush=True)
        predictions = predict_onnx(model_path, frames)
    else:
        print("[TransNetV2] ONNX model not found, falling back to PyTorch", file=sys.stderr, flush=True)
        weights = model_path if (model_path and model_path.endswith(".pth")) else None
        predictions = predict_pytorch(frames, weights_path=weights)

    # Convert to scenes
    scenes = predictions_to_scenes(predictions, threshold=args.threshold)
    print("[TransNetV2] Found {} scenes".format(len(scenes)), file=sys.stderr, flush=True)

    # Convert frame indices to timestamps
    result = []
    for start_frame, end_frame in scenes:
        result.append({
            "start": round(float(start_frame) / fps, 4),
            "end": round(float(end_frame) / fps, 4),
        })

    # Output JSON to stdout (only stdout, everything else goes to stderr)
    print(json.dumps(result))


if __name__ == "__main__":
    main()
