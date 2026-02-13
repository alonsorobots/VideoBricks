import type { CropRect } from "../lib/types";

interface CropMaskProps {
  cropRect: CropRect;
}

/**
 * Fully opaque mask that hides everything outside the crop rect.
 * Uses a single element with an enormous box-shadow to cover all surrounding area,
 * ensuring no gaps even when the parent container is transformed (zoom/pan).
 *
 * Coordinates are percentage-based relative to the parent container,
 * which should be sized to match the video render area.
 */
export default function CropMask({ cropRect }: CropMaskProps) {
  const x = cropRect.x * 100;
  const y = cropRect.y * 100;
  const w = cropRect.width * 100;
  const h = cropRect.height * 100;

  return (
    <div
      className="absolute pointer-events-none"
      style={{
        left: `${x}%`,
        top: `${y}%`,
        width: `${w}%`,
        height: `${h}%`,
        // The crop window itself is transparent; everything outside is covered
        // by an enormous box-shadow in the background color.
        boxShadow: "0 0 0 9999px var(--color-bg)",
      }}
    />
  );
}
