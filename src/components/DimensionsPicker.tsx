import { useState, useCallback, useMemo, useEffect, useRef } from "react";
import type { DimensionMode } from "../lib/types";

interface DimensionsPickerProps {
  videoWidth: number;
  videoHeight: number;
  cropWidth: number;
  cropHeight: number;
  outputWidth: number;
  outputHeight: number;
  arLocked: boolean;
  onArLockedChange: (locked: boolean) => void;
  onChange: (width: number, height: number) => void;
}

const COMMON_PERCENTS = [100, 50, 33, 25, 20];

interface Preset {
  label: string;
  width: number;
  height: number;
}

export default function DimensionsPicker({
  videoWidth,
  videoHeight,
  cropWidth,
  cropHeight,
  outputWidth,
  outputHeight,
  arLocked,
  onArLockedChange,
  onChange,
}: DimensionsPickerProps) {
  const [mode, setMode] = useState<DimensionMode>("pixels");

  // Local string state for commit-on-blur (pixel inputs)
  const [widthStr, setWidthStr] = useState(String(outputWidth));
  const [heightStr, setHeightStr] = useState(String(outputHeight));
  const [percentStr, setPercentStr] = useState("100");

  // Track whether the user is actively editing an input (to avoid external overwrites)
  const widthFocusedRef = useRef(false);
  const heightFocusedRef = useRef(false);
  const percentFocusedRef = useRef(false);

  // The effective base for percentage calculations is the cropped dimensions
  const baseW = Math.max(4, cropWidth);
  const baseH = Math.max(4, cropHeight);
  const aspect = baseW / baseH;

  const presets = useMemo((): Preset[] => {
    const items: Preset[] = [];

    // Original (native video size, not cropped)
    items.push({
      label: `${videoWidth} x ${videoHeight} (Original)`,
      width: videoWidth,
      height: videoHeight,
    });

    // Percentage presets (based on cropped dimensions)
    for (const p of COMMON_PERCENTS) {
      if (p === 100) continue; // already covered by "Original"
      const w = Math.max(4, Math.round((baseW * p) / 100));
      const h = Math.max(4, Math.round((baseH * p) / 100));
      items.push({
        label: `${p}% (${w} x ${h})`,
        width: w,
        height: h,
      });
    }

    return items;
  }, [videoWidth, videoHeight, baseW, baseH]);

  // Find matching preset
  const selectedPreset = useMemo(() => {
    return presets.find(
      (p) => p.width === outputWidth && p.height === outputHeight
    );
  }, [presets, outputWidth, outputHeight]);

  // Sync local string state when output dims change externally (e.g., crop change syncs dims)
  useEffect(() => {
    if (!widthFocusedRef.current) setWidthStr(String(outputWidth));
    if (!heightFocusedRef.current) setHeightStr(String(outputHeight));
    if (!percentFocusedRef.current && baseW > 0) {
      setPercentStr(String(Math.round((outputWidth / baseW) * 100)));
    }
  }, [outputWidth, outputHeight, baseW]);

  // --- Commit helpers ---

  // Commit pixel dimensions (called on blur/Enter)
  const commitPixels = useCallback(
    (wStr: string, hStr: string) => {
      let w = Math.max(4, parseInt(wStr) || 4);
      let h = Math.max(4, parseInt(hStr) || 4);

      if (arLocked) {
        // Determine which field changed relative to current output
        const wChanged = w !== outputWidth;
        const hChanged = h !== outputHeight;

        if (wChanged && !hChanged) {
          h = Math.max(4, Math.round(w / aspect));
        } else if (hChanged && !wChanged) {
          w = Math.max(4, Math.round(h * aspect));
        } else if (wChanged && hChanged) {
          // Both changed (e.g., preset or first commit) -- use width as primary
          h = Math.max(4, Math.round(w / aspect));
        }
      }

      setWidthStr(String(w));
      setHeightStr(String(h));
      setPercentStr(String(Math.round((w / baseW) * 100)));
      onChange(w, h);
    },
    [arLocked, aspect, baseW, outputWidth, outputHeight, onChange]
  );

  // Commit percent (called on blur/Enter)
  const commitPercent = useCallback(
    (pStr: string) => {
      const p = Math.max(1, Math.min(100, parseInt(pStr) || 1));
      const w = Math.max(4, Math.round((baseW * p) / 100));
      const h = Math.max(4, Math.round((baseH * p) / 100));
      setPercentStr(String(p));
      setWidthStr(String(w));
      setHeightStr(String(h));
      onChange(w, h);
    },
    [baseW, baseH, onChange]
  );

  const handlePresetChange = useCallback(
    (e: React.ChangeEvent<HTMLSelectElement>) => {
      const value = e.target.value;
      if (value === "custom") return;

      const preset = presets[parseInt(value)];
      if (preset) {
        setWidthStr(String(preset.width));
        setHeightStr(String(preset.height));
        setPercentStr(String(Math.round((preset.width / baseW) * 100)));
        onChange(preset.width, preset.height);
      }
    },
    [presets, baseW, onChange]
  );

  // --- Arrow key helpers (immediate commit) ---

  const handleWidthArrow = useCallback(
    (delta: number) => {
      const w = Math.max(4, (parseInt(widthStr) || outputWidth) + delta);
      const h = arLocked ? Math.max(4, Math.round(w / aspect)) : (parseInt(heightStr) || outputHeight);
      setWidthStr(String(w));
      setHeightStr(String(h));
      setPercentStr(String(Math.round((w / baseW) * 100)));
      onChange(w, h);
    },
    [widthStr, heightStr, outputWidth, outputHeight, arLocked, aspect, baseW, onChange]
  );

  const handleHeightArrow = useCallback(
    (delta: number) => {
      const h = Math.max(4, (parseInt(heightStr) || outputHeight) + delta);
      const w = arLocked ? Math.max(4, Math.round(h * aspect)) : (parseInt(widthStr) || outputWidth);
      setWidthStr(String(w));
      setHeightStr(String(h));
      setPercentStr(String(Math.round((w / baseW) * 100)));
      onChange(w, h);
    },
    [widthStr, heightStr, outputWidth, outputHeight, arLocked, aspect, baseW, onChange]
  );

  const handlePercentArrow = useCallback(
    (delta: number) => {
      const p = Math.max(1, Math.min(100, (parseInt(percentStr) || 100) + delta));
      commitPercent(String(p));
    },
    [percentStr, commitPercent]
  );

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <label className="text-xs font-medium text-text-secondary">
          Dimensions
        </label>
        {/* Pixels / Percent toggle */}
        <div className="flex bg-bg-secondary rounded-md p-0.5">
          <button
            className={`px-2 py-0.5 text-[10px] rounded transition-colors ${
              mode === "pixels"
                ? "bg-surface-raised text-text-primary shadow-sm"
                : "text-text-tertiary"
            }`}
            onClick={() => setMode("pixels")}
          >
            px
          </button>
          <button
            className={`px-2 py-0.5 text-[10px] rounded transition-colors ${
              mode === "percent"
                ? "bg-surface-raised text-text-primary shadow-sm"
                : "text-text-tertiary"
            }`}
            onClick={() => setMode("percent")}
          >
            %
          </button>
        </div>
      </div>

      {/* Preset dropdown */}
      <select
        value={
          selectedPreset
            ? presets.indexOf(selectedPreset).toString()
            : "custom"
        }
        onChange={handlePresetChange}
        className="w-full px-2 py-1.5 text-xs border border-border rounded-md
          bg-surface-raised text-text-primary focus:border-accent focus:outline-none
          cursor-pointer"
      >
        {!selectedPreset && (
          <option value="custom">
            Custom - {mode === "percent" ? `${percentStr}%` : `${widthStr} x ${heightStr}`}
          </option>
        )}
        {presets.map((preset, i) => (
          <option key={i} value={i}>
            {preset.label}
          </option>
        ))}
      </select>

      {/* Direct input */}
      <div className="flex items-center gap-1.5">
        {mode === "pixels" ? (
          <>
            <input
              type="number"
              value={widthStr}
              onChange={(e) => setWidthStr(e.target.value)}
              onFocus={() => { widthFocusedRef.current = true; }}
              onBlur={() => {
                widthFocusedRef.current = false;
                commitPixels(widthStr, heightStr);
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  (e.target as HTMLInputElement).blur();
                } else if (e.key === "ArrowUp") {
                  e.preventDefault();
                  handleWidthArrow(e.altKey ? 10 : 1);
                } else if (e.key === "ArrowDown") {
                  e.preventDefault();
                  handleWidthArrow(-(e.altKey ? 10 : 1));
                }
              }}
              className="flex-1 min-w-0 px-2 py-1 text-xs text-center border border-border rounded-md
                bg-surface-raised text-text-primary focus:border-accent focus:outline-none tabular-nums"
              min={4}
            />

            {/* Lock/unlock AR toggle */}
            <button
              onClick={() => onArLockedChange(!arLocked)}
              className={`p-1 rounded transition-colors shrink-0 ${
                arLocked
                  ? "text-accent hover:text-accent-hover"
                  : "text-text-tertiary hover:text-text-secondary"
              }`}
              title={arLocked ? "Aspect ratio linked (click to unlink)" : "Aspect ratio unlinked (click to link)"}
            >
              {arLocked ? (
                /* Locked chain icon */
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <rect x="3" y="11" width="18" height="11" rx="2" ry="2" />
                  <path d="M7 11V7a5 5 0 0110 0v4" />
                </svg>
              ) : (
                /* Unlocked chain icon */
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                  <rect x="3" y="11" width="18" height="11" rx="2" ry="2" />
                  <path d="M7 11V7a5 5 0 019.9-1" />
                </svg>
              )}
            </button>

            <input
              type="number"
              value={heightStr}
              onChange={(e) => setHeightStr(e.target.value)}
              onFocus={() => { heightFocusedRef.current = true; }}
              onBlur={() => {
                heightFocusedRef.current = false;
                commitPixels(widthStr, heightStr);
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  (e.target as HTMLInputElement).blur();
                } else if (e.key === "ArrowUp") {
                  e.preventDefault();
                  handleHeightArrow(e.altKey ? 10 : 1);
                } else if (e.key === "ArrowDown") {
                  e.preventDefault();
                  handleHeightArrow(-(e.altKey ? 10 : 1));
                }
              }}
              className="flex-1 min-w-0 px-2 py-1 text-xs text-center border border-border rounded-md
                bg-surface-raised text-text-primary focus:border-accent focus:outline-none tabular-nums"
              min={4}
            />
          </>
        ) : (
          <div className="flex items-center gap-2 w-full">
            <input
              type="number"
              value={percentStr}
              onChange={(e) => setPercentStr(e.target.value)}
              onFocus={() => { percentFocusedRef.current = true; }}
              onBlur={() => {
                percentFocusedRef.current = false;
                commitPercent(percentStr);
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  (e.target as HTMLInputElement).blur();
                } else if (e.key === "ArrowUp") {
                  e.preventDefault();
                  handlePercentArrow(e.altKey ? 10 : 1);
                } else if (e.key === "ArrowDown") {
                  e.preventDefault();
                  handlePercentArrow(-(e.altKey ? 10 : 1));
                }
              }}
              className="w-16 px-2 py-1 text-xs text-center border border-border rounded-md
                bg-surface-raised text-text-primary focus:border-accent focus:outline-none tabular-nums"
              min={1}
              max={100}
            />
            <span className="text-xs text-text-tertiary">
              ({outputWidth} x {outputHeight})
            </span>
          </div>
        )}
      </div>
    </div>
  );
}
