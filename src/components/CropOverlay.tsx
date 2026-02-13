import { useState, useCallback, useEffect, useRef } from "react";
import type { CropRect } from "../lib/types";

interface CropOverlayProps {
  cropRect: CropRect;
  onCropRectChange: (rect: CropRect) => void;
  videoWidth: number;
  videoHeight: number;
  /** Called when the user double-clicks inside the crop area to accept */
  onAccept?: () => void;
}

type HandlePosition =
  | "top-left"
  | "top"
  | "top-right"
  | "right"
  | "bottom-right"
  | "bottom"
  | "bottom-left"
  | "left"
  | "center";

interface DragState {
  handle: HandlePosition | "draw";
  startX: number;
  startY: number;
  startRect: CropRect;
  /** Normalized anchor point for "draw" mode */
  anchorNorm?: { x: number; y: number };
}

// Custom crosshair cursor: white lines with dark outline, center gap.
// Visible on both light and dark backgrounds.
const CROSSHAIR_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" fill="none">
  <line x1="12" y1="1" x2="12" y2="9" stroke="black" stroke-width="2.5" stroke-linecap="round"/>
  <line x1="12" y1="1" x2="12" y2="9" stroke="white" stroke-width="1" stroke-linecap="round"/>
  <line x1="12" y1="15" x2="12" y2="23" stroke="black" stroke-width="2.5" stroke-linecap="round"/>
  <line x1="12" y1="15" x2="12" y2="23" stroke="white" stroke-width="1" stroke-linecap="round"/>
  <line x1="1" y1="12" x2="9" y2="12" stroke="black" stroke-width="2.5" stroke-linecap="round"/>
  <line x1="1" y1="12" x2="9" y2="12" stroke="white" stroke-width="1" stroke-linecap="round"/>
  <line x1="15" y1="12" x2="23" y2="12" stroke="black" stroke-width="2.5" stroke-linecap="round"/>
  <line x1="15" y1="12" x2="23" y2="12" stroke="white" stroke-width="1" stroke-linecap="round"/>
</svg>`;
const CROSSHAIR_URL = `url("data:image/svg+xml,${encodeURIComponent(CROSSHAIR_SVG)}") 12 12, crosshair`;

export default function CropOverlay({
  cropRect,
  onCropRectChange,
  videoWidth,
  videoHeight,
  onAccept,
}: CropOverlayProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [dragState, setDragState] = useState<DragState | null>(null);
  const guidesRef = useRef<HTMLDivElement>(null);
  const vGuideRef = useRef<HTMLDivElement>(null);
  const hGuideRef = useRef<HTMLDivElement>(null);

  // "simple" = border + handles only (default).
  // "full"   = border + handles + grid lines + center-move area.
  // Clicking a handle promotes to "full". Drawing a new box resets to "simple".
  const [mode, setMode] = useState<"simple" | "full">("simple");

  // Reset to simple mode when cropRect is set back to full-frame (e.g. Reset button)
  useEffect(() => {
    if (cropRect.x === 0 && cropRect.y === 0 && cropRect.width === 1 && cropRect.height === 1) {
      setMode("simple");
    }
  }, [cropRect]);

  const getCursorStyle = (handle: HandlePosition): string => {
    switch (handle) {
      case "top-left":
      case "bottom-right":
        return "nwse-resize";
      case "top-right":
      case "bottom-left":
        return "nesw-resize";
      case "top":
      case "bottom":
        return "ns-resize";
      case "left":
      case "right":
        return "ew-resize";
      case "center":
        return "move";
    }
  };

  /** Clicking a handle/edge -- promotes to full mode.
   *  Immediately snaps the crop edge to the mouse position on click,
   *  then subsequent dragging continues smoothly from there. */
  const handleHandleMouseDown = useCallback(
    (e: React.MouseEvent, handle: HandlePosition) => {
      e.preventDefault();
      e.stopPropagation();
      setMode("full");

      const container = containerRef.current;
      if (!container) return;
      const containerRect = container.getBoundingClientRect();

      // Compute offset between click and handle center in normalized coords
      const el = e.currentTarget as HTMLElement;
      const r = el.getBoundingClientRect();
      const centerX = r.left + r.width / 2;
      const centerY = r.top + r.height / 2;
      const dx = (e.clientX - centerX) / containerRect.width;
      const dy = (e.clientY - centerY) / containerRect.height;

      // Immediately apply that offset to snap the crop edge to the cursor
      const snapped = applyHandleDelta(handle, cropRect, dx, dy);
      onCropRectChange(snapped);

      // Future drags start from the click position with the snapped rect
      setDragState({
        handle,
        startX: e.clientX,
        startY: e.clientY,
        startRect: snapped,
      });
    },
    [cropRect, onCropRectChange]
  );

  /** Click inside the crop box (simple mode) or on background -> draw a new box */
  const handleDrawMouseDown = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      const container = containerRef.current;
      if (!container) return;
      const rect = container.getBoundingClientRect();
      const normX = clamp((e.clientX - rect.left) / rect.width, 0, 1);
      const normY = clamp((e.clientY - rect.top) / rect.height, 0, 1);
      setDragState({
        handle: "draw",
        startX: e.clientX,
        startY: e.clientY,
        startRect: { ...cropRect },
        anchorNorm: { x: normX, y: normY },
      });
    },
    [cropRect]
  );

  /** Click center area in full mode -> move the box */
  const handleCenterMouseDown = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      e.stopPropagation();
      setDragState({
        handle: "center",
        startX: e.clientX,
        startY: e.clientY,
        startRect: { ...cropRect },
      });
    },
    [cropRect]
  );

  useEffect(() => {
    if (!dragState) return;
    const container = containerRef.current;
    if (!container) return;

    const rect = container.getBoundingClientRect();

    const handleMouseMove = (e: MouseEvent) => {
      const dx = (e.clientX - dragState.startX) / rect.width;
      const dy = (e.clientY - dragState.startY) / rect.height;
      const sr = dragState.startRect;

      let newRect: CropRect;

      if (dragState.handle === "draw") {
        const anchor = dragState.anchorNorm!;
        const curX = clamp((e.clientX - rect.left) / rect.width, 0, 1);
        const curY = clamp((e.clientY - rect.top) / rect.height, 0, 1);

        newRect = {
          x: Math.min(anchor.x, curX),
          y: Math.min(anchor.y, curY),
          width: Math.max(Math.abs(curX - anchor.x), 0.02),
          height: Math.max(Math.abs(curY - anchor.y), 0.02),
        };
        newRect.width = Math.min(newRect.width, 1 - newRect.x);
        newRect.height = Math.min(newRect.height, 1 - newRect.y);
      } else if (dragState.handle === "center") {
        newRect = applyHandleDelta("center", sr, dx, dy);
      } else {
        newRect = applyHandleDelta(dragState.handle as HandlePosition, sr, dx, dy);
      }

      onCropRectChange(newRect);
    };

    const handleMouseUp = () => {
      // After drawing a new box, promote to full mode (grid + move center)
      if (dragState.handle === "draw") {
        setMode("full");
      }
      setDragState(null);
    };

    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);
    return () => {
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
    };
  }, [dragState, onCropRectChange]);

  const handles: HandlePosition[] = [
    "top-left",
    "top",
    "top-right",
    "right",
    "bottom-right",
    "bottom",
    "bottom-left",
    "left",
  ];

  const getHandleStyle = (
    handle: HandlePosition
  ): React.CSSProperties => {
    const size = 12;
    const half = size / 2;
    const x = cropRect.x * 100;
    const y = cropRect.y * 100;
    const w = cropRect.width * 100;
    const h = cropRect.height * 100;

    const positions: Record<HandlePosition, { left: string; top: string }> = {
      "top-left": { left: `${x}%`, top: `${y}%` },
      top: { left: `${x + w / 2}%`, top: `${y}%` },
      "top-right": { left: `${x + w}%`, top: `${y}%` },
      right: { left: `${x + w}%`, top: `${y + h / 2}%` },
      "bottom-right": { left: `${x + w}%`, top: `${y + h}%` },
      bottom: { left: `${x + w / 2}%`, top: `${y + h}%` },
      "bottom-left": { left: `${x}%`, top: `${y + h}%` },
      left: { left: `${x}%`, top: `${y + h / 2}%` },
      center: { left: `${x + w / 2}%`, top: `${y + h / 2}%` },
    };

    return {
      position: "absolute",
      ...positions[handle],
      width: size,
      height: size,
      marginLeft: -half,
      marginTop: -half,
      cursor: getCursorStyle(handle),
    };
  };

  const isFull = mode === "full";

  // Track mouse position for guide lines -- direct DOM updates, no React re-render
  const handleMouseMoveGuides = useCallback((e: React.MouseEvent) => {
    const container = containerRef.current;
    if (!container) return;
    const rect = container.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;

    if (guidesRef.current) guidesRef.current.style.display = "";
    if (vGuideRef.current) vGuideRef.current.style.left = `${x}px`;
    if (hGuideRef.current) hGuideRef.current.style.top = `${y}px`;
  }, []);

  const handleMouseLeave = useCallback(() => {
    if (guidesRef.current) guidesRef.current.style.display = "none";
  }, []);

  return (
    <div
      ref={containerRef}
      className="absolute inset-0"
      style={{
        cursor: dragState
          ? dragState.handle === "draw"
            ? CROSSHAIR_URL
            : getCursorStyle(dragState.handle as HandlePosition)
          : CROSSHAIR_URL,
      }}
      onMouseDown={handleDrawMouseDown}
      onMouseMove={handleMouseMoveGuides}
      onMouseLeave={handleMouseLeave}
    >
      {/* Crop border + dark overlay via box-shadow (single element = no subpixel gaps) */}
      <div
        className="absolute border-2 border-white"
        style={{
          left: `${cropRect.x * 100}%`,
          top: `${cropRect.y * 100}%`,
          width: `${cropRect.width * 100}%`,
          height: `${cropRect.height * 100}%`,
          boxShadow: "0 0 0 9999px rgba(0, 0, 0, 0.5)",
        }}
      >
        {/* Rule-of-thirds grid -- only in full mode */}
        {isFull && (
          <div className="absolute inset-0 pointer-events-none">
            <div className="absolute left-1/3 inset-y-0 w-px bg-white/30" />
            <div className="absolute left-2/3 inset-y-0 w-px bg-white/30" />
            <div className="absolute top-1/3 inset-x-0 h-px bg-white/30" />
            <div className="absolute top-2/3 inset-x-0 h-px bg-white/30" />
          </div>
        )}

        {/* Interior area:
            - Full mode: acts as a move handle (cursor: move)
            - Simple mode: lets mousedown pass through to container for draw */}
        {isFull ? (
          <div
            className="absolute inset-2 cursor-move"
            onMouseDown={handleCenterMouseDown}
            onDoubleClick={(e) => { e.preventDefault(); e.stopPropagation(); onAccept?.(); }}
          />
        ) : (
          /* Transparent pass-through: clicks bubble up to the container's
             handleDrawMouseDown because we do NOT stopPropagation. */
          <div className="absolute inset-2" style={{ cursor: CROSSHAIR_URL }} />
        )}
      </div>

      {/* Resize handles */}
      {handles.map((handle) => (
        <div
          key={handle}
          style={getHandleStyle(handle)}
          onMouseDown={(e) => handleHandleMouseDown(e, handle)}
        >
          <div className="w-full h-full rounded-full bg-white border border-border-strong shadow" />
        </div>
      ))}

      {/* Guide lines -- simple blue dotted lines.
          Positioned via direct DOM updates (refs) to avoid React re-renders. */}
      <div
        ref={guidesRef}
        className="absolute inset-0 pointer-events-none"
        style={{ display: "none" }}
      >
        <div
          ref={vGuideRef}
          className="absolute top-0 bottom-0"
          style={{
            width: 1,
            backgroundImage:
              "repeating-linear-gradient(to bottom, #1588ff 0px, #1588ff 2px, transparent 2px, transparent 6px)",
            opacity: 0.6,
          }}
        />
        <div
          ref={hGuideRef}
          className="absolute left-0 right-0"
          style={{
            height: 1,
            backgroundImage:
              "repeating-linear-gradient(to right, #1588ff 0px, #1588ff 2px, transparent 2px, transparent 6px)",
            opacity: 0.6,
          }}
        />
      </div>
    </div>
  );
}

/** Apply a normalized delta to a crop rect for a given handle, returning a new rect. */
function applyHandleDelta(
  handle: HandlePosition,
  sr: CropRect,
  dx: number,
  dy: number
): CropRect {
  const r = { ...sr };

  switch (handle) {
    case "center":
      r.x = clamp(sr.x + dx, 0, 1 - sr.width);
      r.y = clamp(sr.y + dy, 0, 1 - sr.height);
      break;
    case "top-left":
      r.x = clamp(sr.x + dx, 0, sr.x + sr.width - 0.02);
      r.y = clamp(sr.y + dy, 0, sr.y + sr.height - 0.02);
      r.width = sr.x + sr.width - r.x;
      r.height = sr.y + sr.height - r.y;
      break;
    case "top-right":
      r.y = clamp(sr.y + dy, 0, sr.y + sr.height - 0.02);
      r.width = clamp(sr.width + dx, 0.02, 1 - sr.x);
      r.height = sr.y + sr.height - r.y;
      break;
    case "bottom-left":
      r.x = clamp(sr.x + dx, 0, sr.x + sr.width - 0.02);
      r.width = sr.x + sr.width - r.x;
      r.height = clamp(sr.height + dy, 0.02, 1 - sr.y);
      break;
    case "bottom-right":
      r.width = clamp(sr.width + dx, 0.02, 1 - sr.x);
      r.height = clamp(sr.height + dy, 0.02, 1 - sr.y);
      break;
    case "top":
      r.y = clamp(sr.y + dy, 0, sr.y + sr.height - 0.02);
      r.height = sr.y + sr.height - r.y;
      break;
    case "bottom":
      r.height = clamp(sr.height + dy, 0.02, 1 - sr.y);
      break;
    case "left":
      r.x = clamp(sr.x + dx, 0, sr.x + sr.width - 0.02);
      r.width = sr.x + sr.width - r.x;
      break;
    case "right":
      r.width = clamp(sr.width + dx, 0.02, 1 - sr.x);
      break;
  }

  r.x = clamp(r.x, 0, 1);
  r.y = clamp(r.y, 0, 1);
  r.width = clamp(r.width, 0.02, 1 - r.x);
  r.height = clamp(r.height, 0.02, 1 - r.y);
  return r;
}


function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}
