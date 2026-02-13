"""
TransNetV2 shot boundary detection script.

Usage:
    python transnet_detect.py <video_path> [--threshold 0.35] [--weights path/to/weights.pth]

Outputs JSON to stdout:
    [{"start": 0.0, "end": 3.5}, {"start": 3.5, "end": 8.2}, ...]
"""

import sys
import os
import json
import argparse
import subprocess
import numpy as np

# Resolve the TransNetV2 inference-pytorch directory.
# Priority:
#   1. TRANSNETV2_DIR environment variable (set by the user or app config)
#   2. Relative to this script: ../../../../TransNetV2/inference-pytorch
#   3. Common install location: ~/TransNetV2/inference-pytorch
def _find_transnet_dir():
    # 1. Environment variable
    env_dir = os.environ.get("TRANSNETV2_DIR")
    if env_dir and os.path.isdir(env_dir):
        return os.path.normpath(env_dir)

    # 2. Relative to this script (works in dev layout)
    relative = os.path.normpath(os.path.join(
        os.path.dirname(os.path.abspath(__file__)),
        "..", "..", "..", "..",
        "TransNetV2", "inference-pytorch"
    ))
    if os.path.isdir(relative):
        return relative

    # 3. Home directory
    home = os.path.join(os.path.expanduser("~"), "TransNetV2", "inference-pytorch")
    if os.path.isdir(home):
        return os.path.normpath(home)

    return relative  # return best-guess so error message is useful

TRANSNET_DIR = _find_transnet_dir()


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


def predict_frames_pytorch(model, frames):
    """Run TransNetV2 PyTorch inference on frames, porting the sliding-window logic."""
    import torch

    def input_iterator():
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
            yield out[np.newaxis]

    predictions = []

    with torch.no_grad():
        for inp in input_iterator():
            single_frame_pred, all_frames_pred = model(torch.from_numpy(inp))
            single_frame_pred = torch.sigmoid(single_frame_pred)

            predictions.append(single_frame_pred.numpy()[0, 25:75, 0])

            print("[TransNetV2] Processing video frames {}/{}".format(
                min(len(predictions) * 50, len(frames)), len(frames)
            ), file=sys.stderr, flush=True)

    print("", file=sys.stderr, flush=True)

    single_frame_pred = np.concatenate(predictions)
    return single_frame_pred[:len(frames)]


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


def main():
    parser = argparse.ArgumentParser(description="TransNetV2 shot detection")
    parser.add_argument("video", type=str, help="Path to video file")
    parser.add_argument("--threshold", type=float, default=0.35,
                        help="Scene change threshold (default 0.35)")
    parser.add_argument("--weights", type=str, default=None,
                        help="Path to .pth weights file")
    args = parser.parse_args()

    # Ensure we can import from TransNetV2
    sys.path.insert(0, TRANSNET_DIR)
    try:
        from transnetv2_pytorch import TransNetV2
    except ImportError:
        print("[TransNetV2] ERROR: Could not import transnetv2_pytorch from {}".format(TRANSNET_DIR),
              file=sys.stderr)
        sys.exit(1)

    import torch

    # Locate weights
    weights_path = args.weights
    if weights_path is None:
        weights_path = os.path.join(TRANSNET_DIR, "transnetv2-pytorch-weights.pth")

    if not os.path.isfile(weights_path):
        print("[TransNetV2] ERROR: Weights not found at {}".format(weights_path), file=sys.stderr)
        sys.exit(1)

    # Load model
    print("[TransNetV2] Loading model from {}".format(weights_path), file=sys.stderr, flush=True)
    model = TransNetV2()
    state_dict = torch.load(weights_path, map_location="cpu", weights_only=True)
    model.load_state_dict(state_dict)
    model.eval()

    # Get video FPS
    fps = get_video_fps(args.video)
    print("[TransNetV2] Video FPS: {:.2f}".format(fps), file=sys.stderr, flush=True)

    # Extract frames
    frames = extract_frames(args.video)

    if len(frames) == 0:
        print("[TransNetV2] ERROR: No frames extracted", file=sys.stderr)
        sys.exit(1)

    # Run inference
    predictions = predict_frames_pytorch(model, frames)

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
