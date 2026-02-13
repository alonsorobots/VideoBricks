import { useCallback, useState, useRef, useEffect } from "react";
import { createPortal } from "react-dom";
import type { CropRect } from "../lib/types";
import { ASPECT_RATIO_PRESETS } from "../lib/types";

interface CropToolbarProps {
  active: boolean;
  onToggle: () => void;
  cropRect: CropRect;
  onCropRectChange: (rect: CropRect) => void;
  videoWidth: number;
  videoHeight: number;
  /** Called when Reset is pressed -- resets crop and dimensions */
  onReset?: () => void;
}

export default function CropToolbar({
  active,
  onToggle,
  cropRect,
  onCropRectChange,
  videoWidth,
  videoHeight,
  onReset,
}: CropToolbarProps) {
  const [showPresets, setShowPresets] = useState(false);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const [dropdownPos, setDropdownPos] = useState({ top: 0, left: 0 });

  // Position the dropdown relative to the Ratio button
  useEffect(() => {
    if (showPresets && buttonRef.current) {
      const rect = buttonRef.current.getBoundingClientRect();
      setDropdownPos({
        top: rect.bottom + 4,
        left: rect.left,
      });
    }
  }, [showPresets]);

  const applyAspectRatio = useCallback(
    (ratio: number | null) => {
      if (ratio === null) {
        // Free - just close the dropdown, keep the current crop unchanged
        setShowPresets(false);
        return;
      } else {
        const videoAspect = videoWidth / videoHeight;
        let w: number, h: number;

        if (ratio > videoAspect) {
          w = 1;
          h = (videoWidth / ratio) / videoHeight;
        } else {
          h = 1;
          w = (videoHeight * ratio) / videoWidth;
        }

        w = Math.min(1, w);
        h = Math.min(1, h);

        onCropRectChange({
          x: (1 - w) / 2,
          y: (1 - h) / 2,
          width: w,
          height: h,
        });
      }
      setShowPresets(false);
    },
    [videoWidth, videoHeight, onCropRectChange]
  );

  const handleReset = useCallback(() => {
    if (onReset) {
      onReset();
    } else {
      onCropRectChange({ x: 0, y: 0, width: 1, height: 1 });
    }
  }, [onCropRectChange, onReset]);

  return (
    <div className="flex items-center gap-1">
      <button
        onClick={onToggle}
        className={`px-3 py-1.5 rounded-md text-xs font-medium transition-colors ${
          active
            ? "bg-accent text-white"
            : "bg-surface-raised text-text-secondary hover:bg-surface-hover"
        }`}
      >
        Crop
      </button>

      {active && (
        <>
          <button
            ref={buttonRef}
            onClick={() => setShowPresets(!showPresets)}
            className={`px-2 py-1.5 rounded-md text-xs transition-colors ${
              showPresets
                ? "bg-accent/10 text-accent"
                : "bg-surface-raised text-text-secondary hover:bg-surface-hover"
            }`}
          >
            Ratio
          </button>

          <button
            onClick={handleReset}
            className="px-2 py-1.5 rounded-md text-xs bg-surface-raised text-text-secondary
              hover:bg-surface-hover transition-colors"
          >
            Reset
          </button>
        </>
      )}

      {/* Aspect ratio presets dropdown -- rendered via portal so it can't be clipped */}
      {showPresets &&
        createPortal(
          <>
            {/* Backdrop to close on click outside */}
            <div
              className="fixed inset-0"
              style={{ zIndex: 9998 }}
              onClick={() => setShowPresets(false)}
            />
            {/* Dropdown */}
            <div
              className="bg-surface-raised rounded-lg border border-border shadow-xl py-1 min-w-[120px]"
              style={{
                position: "fixed",
                top: dropdownPos.top,
                left: dropdownPos.left,
                zIndex: 9999,
              }}
            >
              {ASPECT_RATIO_PRESETS.map((preset) => (
                <button
                  key={preset.label}
                  onClick={() => applyAspectRatio(preset.ratio)}
                  className="w-full px-3 py-2 text-xs text-left text-text-primary
                    hover:bg-surface-hover transition-colors"
                >
                  {preset.label}
                </button>
              ))}
            </div>
          </>,
          document.body
        )}
    </div>
  );
}
