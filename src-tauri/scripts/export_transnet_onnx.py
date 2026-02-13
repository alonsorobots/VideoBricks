"""
One-time script: export TransNetV2 PyTorch weights to ONNX format.

Usage (requires PyTorch -- run in the yt_filter conda env):
    conda activate yt_filter
    python export_transnet_onnx.py --weights path/to/transnetv2-pytorch-weights.pth --output transnetv2.onnx

The resulting .onnx file can then be used with transnet_detect.py,
which only needs onnxruntime + numpy (no PyTorch).
"""

import argparse
import os
import sys
import torch

# Resolve TransNetV2 source
def find_transnet_dir():
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


def main():
    parser = argparse.ArgumentParser(description="Export TransNetV2 to ONNX")
    parser.add_argument("--weights", type=str, default=None,
                        help="Path to transnetv2-pytorch-weights.pth")
    parser.add_argument("--output", type=str, default=None,
                        help="Output path for .onnx file (default: next to this script)")
    parser.add_argument("--opset", type=int, default=16,
                        help="ONNX opset version (default: 16)")
    args = parser.parse_args()

    transnet_dir = find_transnet_dir()
    sys.path.insert(0, transnet_dir)

    try:
        from transnetv2_pytorch import TransNetV2
    except ImportError:
        print("ERROR: Could not import transnetv2_pytorch from {}".format(transnet_dir))
        print("Set TRANSNETV2_DIR environment variable or clone TransNetV2 repo.")
        sys.exit(1)

    # Locate weights
    weights_path = args.weights
    if weights_path is None:
        weights_path = os.path.join(transnet_dir, "transnetv2-pytorch-weights.pth")
    if not os.path.isfile(weights_path):
        print("ERROR: Weights not found at {}".format(weights_path))
        sys.exit(1)

    # Output path
    output_path = args.output
    if output_path is None:
        output_path = os.path.join(
            os.path.dirname(os.path.abspath(__file__)),
            "transnetv2.onnx"
        )

    print("Loading model from {} ...".format(weights_path))
    model = TransNetV2()
    state_dict = torch.load(weights_path, map_location="cpu", weights_only=True)
    model.load_state_dict(state_dict)
    model.eval()

    # TransNetV2 expects [batch, frames, height, width, channels] as uint8
    # The sliding window always sends 100 frames at a time
    dummy_input = torch.randint(0, 255, (1, 100, 27, 48, 3), dtype=torch.uint8)

    # Force everything onto CPU (avoids device mismatch in BatchNorm running stats)
    model = model.cpu()
    for buf_name, buf in model.named_buffers():
        if buf.device.type != "cpu":
            print("  Moving buffer {} from {} to cpu".format(buf_name, buf.device))

    print("Exporting to ONNX (opset {}, legacy trace mode) ...".format(args.opset))
    # Use dynamo=False to force legacy trace-based export (avoids torch.export issues)
    torch.onnx.export(
        model,
        (dummy_input,),
        output_path,
        opset_version=args.opset,
        input_names=["frames"],
        output_names=["single_frame_pred", "many_hot_pred"],
        dynamic_axes=None,  # fixed shape: [1, 100, 27, 48, 3]
        dynamo=False,
    )

    size_mb = os.path.getsize(output_path) / (1024 * 1024)
    print("Exported to {} ({:.1f} MB)".format(output_path, size_mb))
    print("Done! This .onnx file can be used with transnet_detect.py (onnxruntime mode).")


if __name__ == "__main__":
    main()
