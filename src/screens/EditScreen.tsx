import { useState, useCallback, useMemo, useEffect, useRef } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  VideoMetadata,
  AppSettings,
  ConversionSettings,
  CropRect,
  EditState,
  TrimSegment,
} from "../lib/types";
import { FPS_MIN, FPS_MAX, SPEED_MIN, SPEED_MAX, SPEED_STEP, newSegmentId } from "../lib/types";
import VideoPlayer from "../components/VideoPlayer";
import TrimSlider from "../components/TrimSlider";
import CropOverlay from "../components/CropOverlay";
import CropMask from "../components/CropMask";
import CropToolbar from "../components/CropToolbar";
import DimensionsPicker from "../components/DimensionsPicker";
import EstimatedSize from "../components/EstimatedSize";
import EditableValue from "../components/EditableValue";
import AboutModal from "../components/AboutModal";
import * as api from "../lib/tauri";

/**
 * Compute where the video actually renders inside a container.
 *
 * The <video> element uses `w-full h-full object-contain`, which means:
 *  - It scales to fit the container in both directions (up or down),
 *  - maintaining aspect ratio, centered within the container.
 *
 * scale = min(containerW / videoW, containerH / videoH)
 */
function computeVideoRect(
  containerWidth: number,
  containerHeight: number,
  videoWidth: number,
  videoHeight: number
) {
  if (containerWidth <= 0 || containerHeight <= 0 || videoWidth <= 0 || videoHeight <= 0) {
    return { left: 0, top: 0, width: containerWidth, height: containerHeight };
  }

  // Standard object-contain: scale to fit, no cap (video can scale up beyond native size)
  const scale = Math.min(containerWidth / videoWidth, containerHeight / videoHeight);
  const renderWidth = videoWidth * scale;
  const renderHeight = videoHeight * scale;

  return {
    left: (containerWidth - renderWidth) / 2,
    top: (containerHeight - renderHeight) / 2,
    width: renderWidth,
    height: renderHeight,
  };
}

interface EditScreenProps {
  videoPath: string;
  metadata: VideoMetadata;
  settings: AppSettings;
  onSettingsChange: (settings: AppSettings) => void;
  onConvert: (settings: ConversionSettings) => void;
  onConvertMp4: (settings: ConversionSettings) => void;
  onCancel: () => void;
  initialEditState?: EditState | null;
  onEditStateChange?: (state: EditState) => void;
}

export default function EditScreen({
  videoPath,
  metadata,
  settings,
  onSettingsChange,
  onConvert,
  onConvertMp4,
  onCancel,
  initialEditState,
  onEditStateChange,
}: EditScreenProps) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const videoContainerRef = useRef<HTMLDivElement>(null);
  const [segments, setSegments] = useState<TrimSegment[]>(() => {
    const raw = initialEditState?.segments;
    if (raw && raw.length > 0) {
      // Ensure all segments have stable IDs (backcompat with old saved state)
      return raw.map((s) => s.id ? s : { ...s, id: newSegmentId() });
    }
    return [{ id: newSegmentId(), start: 0, end: metadata.duration }];
  });
  const [segmentMode, setSegmentMode] = useState<"merge" | "split">("merge");
  const [currentTime, setCurrentTime] = useState(0);
  const [isPlaying, setIsPlaying] = useState(false);
  const [isScrubbing, setIsScrubbing] = useState(false);
  const wasPlayingBeforeScrub = useRef(false);
  const [cropActive, setCropActive] = useState(false);
  const [cropRect, setCropRect] = useState<CropRect>(
    initialEditState?.cropRect ?? { x: 0, y: 0, width: 1, height: 1 }
  );
  const [outputWidth, setOutputWidth] = useState(initialEditState?.outputWidth ?? metadata.width);
  const [outputHeight, setOutputHeight] = useState(initialEditState?.outputHeight ?? metadata.height);
  const [arLocked, setArLocked] = useState(true);

  // User-editable output name (no extension). Derived from source video name initially.
  const defaultOutputName = useMemo(() => {
    const filename = videoPath.split("\\").pop()?.split("/").pop() || "output";
    return filename.replace(/\.[^.]+$/, "");
  }, [videoPath]);
  const [outputName, setOutputName] = useState(defaultOutputName);
  const [editingName, setEditingName] = useState(false);
  const nameInputRef = useRef<HTMLInputElement>(null);
  const [aboutOpen, setAboutOpen] = useState(false);

  const [findingShotsLoading, setFindingShotsLoading] = useState(false);
  const [hasUsedFindShots, setHasUsedFindShots] = useState(false);
  const [thumbnails, setThumbnails] = useState<string[]>([]);
  const [detailThumbnails, setDetailThumbnails] = useState<string[] | null>(null);
  const [videoRect, setVideoRect] = useState({ left: 0, top: 0, width: 0, height: 0 });
  const [containerSize, setContainerSize] = useState({ width: 0, height: 0 });
  const timelineContainerRef = useRef<HTMLDivElement>(null);

  // Convert path for video element src using Tauri asset protocol
  const videoSrc = useMemo(() => convertFileSrc(videoPath), [videoPath]);

  // Load thumbnails for timeline -- recalculate count when container resizes
  useEffect(() => {
    const container = timelineContainerRef.current;
    const THUMB_HEIGHT = 40;
    const THUMB_WIDTH_APPROX = 60; // approximate width per thumbnail

    const loadThumbnails = () => {
      const containerWidth = container ? container.getBoundingClientRect().width : 600;
      const count = Math.max(8, Math.ceil(containerWidth / THUMB_WIDTH_APPROX));
      api.getVideoThumbnails(videoPath, count, THUMB_HEIGHT)
        .then(setThumbnails)
        .catch(() => {});
    };

    loadThumbnails();

    if (!container) return;
    let debounceTimer: ReturnType<typeof setTimeout> | null = null;
    const observer = new ResizeObserver(() => {
      // Debounce to avoid excessive FFmpeg calls during drag-resize
      if (debounceTimer) clearTimeout(debounceTimer);
      debounceTimer = setTimeout(loadThumbnails, 300);
    });
    observer.observe(container);
    return () => {
      observer.disconnect();
      if (debounceTimer) clearTimeout(debounceTimer);
    };
  }, [videoPath]);

  // Track the actual video render rect within the container for crop overlay positioning
  useEffect(() => {
    const container = videoContainerRef.current;
    if (!container) return;

    const updateRect = () => {
      const { width: cw, height: ch } = container.getBoundingClientRect();
      setContainerSize({ width: cw, height: ch });
      setVideoRect(computeVideoRect(cw, ch, metadata.width, metadata.height));
    };

    updateRect();
    const observer = new ResizeObserver(updateRect);
    observer.observe(container);
    return () => observer.disconnect();
  }, [metadata.width, metadata.height]);

  // Report edit state changes upward for persistence
  useEffect(() => {
    onEditStateChange?.({
      segments,
      cropRect,
      outputWidth,
      outputHeight,
    });
  }, [segments, cropRect, outputWidth, outputHeight, onEditStateChange]);

  // Scrub handlers: pause video during scrub, resume after
  const handleScrubStart = useCallback(() => {
    setIsScrubbing(true);
    wasPlayingBeforeScrub.current = isPlaying;
    if (videoRef.current && isPlaying) {
      videoRef.current.pause();
    }
  }, [isPlaying]);

  const handleScrubEnd = useCallback(() => {
    setIsScrubbing(false);
    if (wasPlayingBeforeScrub.current && videoRef.current) {
      videoRef.current.play().catch(() => {});
    }
  }, []);

  // Max FPS based on video frame rate and speed
  const maxFps = Math.min(
    FPS_MAX,
    Math.round(metadata.frameRate * settings.outputSpeed)
  );

  // The actual FPS value to use -- clamped to the range but not mutated by speed changes
  const effectiveFps = Math.max(FPS_MIN, Math.min(settings.outputFps, maxFps));

  // Only active segments get rendered
  const activeSegments = useMemo(
    () => segments.filter((s) => s.active !== false),
    [segments]
  );

  // Build conversion settings
  const conversionSettings = useMemo((): ConversionSettings => {
    const hasCrop =
      cropRect.x !== 0 ||
      cropRect.y !== 0 ||
      cropRect.width !== 1 ||
      cropRect.height !== 1;

    return {
      sourcePath: videoPath,
      outputName: outputName || defaultOutputName,
      quality: settings.outputQuality,
      width: outputWidth,
      height: outputHeight,
      fps: effectiveFps,
      speed: settings.outputSpeed,
      loopForever: settings.loopGif,
      loopCount: settings.loopCount,
      bounce: settings.bounceGif,
      segments: activeSegments,
      segmentMode,
      crop: hasCrop ? cropRect : null,
    };
  }, [
    videoPath,
    outputName,
    defaultOutputName,
    settings,
    outputWidth,
    outputHeight,
    activeSegments,
    segmentMode,
    cropRect,
    effectiveFps,
  ]);

  // Duration considering trim and bounce
  const trimmedDuration = activeSegments.reduce((sum, s) => sum + (s.end - s.start), 0) / settings.outputSpeed;
  const gifDuration = settings.bounceGif
    ? trimmedDuration * 2
    : trimmedDuration;

  // Effective cropped pixel dimensions (before any scaling)
  const cropPixelWidth = Math.round(cropRect.width * metadata.width);
  const cropPixelHeight = Math.round(cropRect.height * metadata.height);

  // --- Crop-to-dimensions sync (replaces the old fragile useEffect) ---
  // Refs keep values fresh for the rapid-fire callback during crop drag,
  // avoiding stale closures and eliminating the need for skip flags.
  const cropRectRef = useRef(cropRect);
  cropRectRef.current = cropRect;
  const outputWidthRef = useRef(outputWidth);
  outputWidthRef.current = outputWidth;
  const outputHeightRef = useRef(outputHeight);
  outputHeightRef.current = outputHeight;
  const arLockedRef = useRef(arLocked);
  arLockedRef.current = arLocked;

  // Called ONLY by user-initiated crop changes (CropOverlay drag).
  // Programmatic setCropRect calls (Reset, handleDimensionsChange) bypass this
  // and manage dimensions themselves -- no skip flags needed.
  const handleCropRectChange = useCallback(
    (newCrop: CropRect) => {
      const prev = cropRectRef.current;
      setCropRect(newCrop);

      // Same crop? Nothing to sync.
      if (prev.x === newCrop.x && prev.y === newCrop.y &&
          prev.width === newCrop.width && prev.height === newCrop.height) return;

      // Maintain the user's current scale ratio across the crop change.
      const prevCropW = Math.max(1, Math.round(prev.width * metadata.width));
      const prevCropH = Math.max(1, Math.round(prev.height * metadata.height));
      const scaleW = prevCropW > 0 ? outputWidthRef.current / prevCropW : 1;
      const scaleH = prevCropH > 0 ? outputHeightRef.current / prevCropH : 1;

      const cw = Math.max(4, Math.round(newCrop.width * metadata.width));
      const ch = Math.max(4, Math.round(newCrop.height * metadata.height));

      if (arLockedRef.current) {
        setOutputWidth(Math.max(4, Math.round(cw * scaleW)));
        setOutputHeight(Math.max(4, Math.round(ch * scaleW)));
      } else {
        setOutputWidth(Math.max(4, Math.round(cw * scaleW)));
        setOutputHeight(Math.max(4, Math.round(ch * scaleH)));
      }
    },
    [metadata.width, metadata.height]
  );

  /**
   * Scale-first-then-crop: given a source (crop) region and target dimensions,
   * compute a centered crop rect that maximizes preserved pixels.
   *
   * Algorithm:
   *  1. For each axis, compute the scale factor: targetW/sourceW and targetH/sourceH
   *  2. Use the LARGER scale (= the axis that needs less reduction) to scale uniformly
   *  3. Crop the other axis (the one that overflows) symmetrically
   */
  const computeAutoCrop = useCallback(
    (targetW: number, targetH: number) => {
      const sourceW = metadata.width;
      const sourceH = metadata.height;
      const targetAR = targetW / targetH;
      const sourceAR = sourceW / sourceH;

      if (Math.abs(targetAR - sourceAR) < 0.005) {
        // Already matching AR -- full crop
        return { x: 0, y: 0, width: 1, height: 1 };
      }

      if (targetAR < sourceAR) {
        // Target is taller than source -> need to crop width
        // Scale by H: sourceH -> targetH, sourceW * (targetH/sourceH) > targetW
        const scaledW = sourceW * (targetH / sourceH);
        const cropFrac = targetW / scaledW; // fraction of width to keep
        const x = (1 - cropFrac) / 2;
        return { x, y: 0, width: cropFrac, height: 1 };
      } else {
        // Target is wider than source -> need to crop height
        // Scale by W: sourceW -> targetW, sourceH * (targetW/sourceW) > targetH
        const scaledH = sourceH * (targetW / sourceW);
        const cropFrac = targetH / scaledH; // fraction of height to keep
        const y = (1 - cropFrac) / 2;
        return { x: 0, y, width: 1, height: cropFrac };
      }
    },
    [metadata.width, metadata.height]
  );

  // Handle dimension changes from DimensionsPicker.
  // Calls setCropRect directly (not handleCropRectChange) so no dimension re-sync fires.
  const handleDimensionsChange = useCallback(
    (w: number, h: number) => {
      setOutputWidth(w);
      setOutputHeight(h);

      // When AR is unlocked and the new dims don't match the source AR,
      // auto-compute a centered crop using scale-first-then-crop.
      if (!arLocked) {
        const sourceAR = metadata.width / metadata.height;
        const targetAR = w / h;
        if (Math.abs(targetAR - sourceAR) > 0.005) {
          setCropRect(computeAutoCrop(w, h));
        } else {
          setCropRect({ x: 0, y: 0, width: 1, height: 1 });
        }
      }
    },
    [arLocked, metadata.width, metadata.height, computeAutoCrop]
  );

  const handleConvert = useCallback(() => {
    onConvert(conversionSettings);
  }, [onConvert, conversionSettings]);

  const handleConvertMp4 = useCallback(() => {
    onConvertMp4(conversionSettings);
  }, [onConvertMp4, conversionSettings]);

  // Find Shots handler with progress
  const [findShotsError, setFindShotsError] = useState<string | null>(null);
  const [findShotsProgress, setFindShotsProgress] = useState<{
    stage: string; progress: number; current: number; total: number;
  } | null>(null);

  // Smooth time-based ring progress: asymptotically fills during long phases
  // so it feels linear rather than jumping between fixed values.
  const [ringProgress, setRingProgress] = useState(0);
  const findShotsStartRef = useRef<number>(0);
  const stageStartRef = useRef<number>(0);

  // Listen for find-shots-progress events
  useEffect(() => {
    const unlisten = listen<{ stage: string; progress: number; current: number; total: number }>(
      "find-shots-progress",
      (event) => {
        const prev = findShotsProgress;
        if (!prev || prev.stage !== event.payload.stage) {
          stageStartRef.current = Date.now();
        }
        setFindShotsProgress(event.payload);
      }
    );
    return () => { unlisten.then((fn) => fn()); };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [findShotsProgress?.stage]);

  // Animation loop: smoothly update ringProgress based on stage + elapsed time
  useEffect(() => {
    if (!findingShotsLoading) {
      setRingProgress(0);
      return;
    }
    let raf: number;
    const tick = () => {
      const now = Date.now();
      const totalElapsed = (now - findShotsStartRef.current) / 1000;
      const stageElapsed = (now - stageStartRef.current) / 1000;
      const stage = findShotsProgress?.stage ?? null;

      let target: number;
      if (stage === "done") {
        target = 100;
      } else if (stage === "analyzing") {
        // 88-99%, driven by actual frame progress
        const p = findShotsProgress?.progress ?? 0;
        target = 88 + p * 11;
      } else if (stage === "extracting") {
        // 85-88%, fill over ~10s asymptotically
        target = 85 + 3 * (1 - Math.exp(-stageElapsed / 5));
      } else if (stage === "loading") {
        // 2-85%: asymptotic fill. Time constant ~20s means it reaches ~63% of 85
        // at 20s, ~86% of 85 at 40s. Feels smooth and never stalls visually.
        target = 2 + 83 * (1 - Math.exp(-stageElapsed / 20));
      } else {
        // No events yet -- creep up slowly based on total elapsed
        target = Math.min(5, 2 + totalElapsed * 0.5);
      }

      setRingProgress(target);
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [findingShotsLoading, findShotsProgress]);

  const handleFindShots = useCallback(async () => {
    setFindingShotsLoading(true);
    setFindShotsError(null);
    setFindShotsProgress(null);
    findShotsStartRef.current = Date.now();
    stageStartRef.current = Date.now();
    try {
      const scenes = await api.findShots(videoPath, 0.35);
      if (scenes.length > 0) {
        const newSegments: TrimSegment[] = scenes.map((scene) => ({
          id: newSegmentId(),
          start: scene.start,
          end: Math.min(scene.end, metadata.duration),
          active: true,
        }));
        setSegments(newSegments);
        setHasUsedFindShots(true);
      } else {
        setFindShotsError("No shots detected");
      }
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      console.error("Find Shots failed:", msg);
      setFindShotsError(msg);
    } finally {
      setFindingShotsLoading(false);
      setFindShotsProgress(null);
    }
  }, [videoPath, metadata.duration]);

  // Segment activation helpers
  const handleActivateAll = useCallback(() => {
    setSegments((prev) => prev.map((s) => ({ ...s, active: true })));
  }, []);

  const handleDeactivateAll = useCallback(() => {
    setSegments((prev) => prev.map((s) => ({ ...s, active: false })));
  }, []);

  const handleInverseActive = useCallback(() => {
    setSegments((prev) => prev.map((s) => ({ ...s, active: s.active === false ? true : false })));
  }, []);

  // Check if all segments are active or all deactivated
  const allActive = segments.every((s) => s.active !== false);

  // Is crop preview active? (crop accepted, non-trivial crop)
  const hasCropPreview = !cropActive &&
    (cropRect.x !== 0 || cropRect.y !== 0 || cropRect.width !== 1 || cropRect.height !== 1);

  // Compute the CSS transform to zoom into the crop region
  const cropPreviewTransform = useMemo(() => {
    if (!hasCropPreview || videoRect.width <= 0 || videoRect.height <= 0) return null;

    // Crop box in container pixels
    const cropBoxX = videoRect.left + cropRect.x * videoRect.width;
    const cropBoxY = videoRect.top + cropRect.y * videoRect.height;
    const cropBoxW = cropRect.width * videoRect.width;
    const cropBoxH = cropRect.height * videoRect.height;

    if (cropBoxW <= 0 || cropBoxH <= 0) return null;

    // Scale so the crop box fills the container
    const cW = containerSize.width;
    const cH = containerSize.height;
    const scale = Math.min(cW / cropBoxW, cH / cropBoxH);

    // Center of the crop box (before transform)
    const centerX = cropBoxX + cropBoxW / 2;
    const centerY = cropBoxY + cropBoxH / 2;

    // Translate so crop box center lands at container center after scaling
    // CSS: translate(tx, ty) scale(S)  =>  first scale, then translate
    const tx = cW / 2 - scale * centerX;
    const ty = cH / 2 - scale * centerY;

    return {
      transform: `translate(${tx}px, ${ty}px) scale(${scale})`,
      transformOrigin: "0 0",
    };
  }, [hasCropPreview, cropRect, videoRect, containerSize]);

  // Frame duration for creating minimal segments
  const frameDuration = metadata.frameRate > 0 ? 1 / metadata.frameRate : 1 / 30;

  // Keyboard shortcut for trim: [ = set in, ] = set out
  // If playhead is inside a segment, adjust that segment.
  // If outside all segments, create a new segment.
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "[") {
        const insideIdx = segments.findIndex(
          (s) => currentTime >= s.start && currentTime <= s.end
        );
        if (insideIdx >= 0) {
          // Adjust this segment's start
          const updated = [...segments];
          updated[insideIdx] = { ...updated[insideIdx], start: currentTime };
          setSegments(updated);
        } else {
          // Create a new segment: in-point = currentTime, out-point = +1 frame
          const newSeg: TrimSegment = {
            id: newSegmentId(),
            start: currentTime,
            end: Math.min(currentTime + frameDuration, metadata.duration),
          };
          const updated = [...segments, newSeg].sort((a, b) => a.start - b.start);
          setSegments(updated);
        }
      } else if (e.key === "]") {
        const insideIdx = segments.findIndex(
          (s) => currentTime >= s.start && currentTime <= s.end
        );
        if (insideIdx >= 0) {
          // Adjust this segment's end
          const updated = [...segments];
          updated[insideIdx] = { ...updated[insideIdx], end: currentTime };
          setSegments(updated);
        } else {
          // Create a new segment: out-point = currentTime, in-point = -1 frame
          const newSeg: TrimSegment = {
            id: newSegmentId(),
            start: Math.max(currentTime - frameDuration, 0),
            end: currentTime,
          };
          const updated = [...segments, newSeg].sort((a, b) => a.start - b.start);
          setSegments(updated);
        }
      } else if (e.key === "Enter") {
        if (cropActive) {
          e.preventDefault();
          // If an input has focus, blur it first so its value commits
          const active = document.activeElement;
          if (active && (active as HTMLElement).tagName === "INPUT") {
            (active as HTMLElement).blur();
          }
          setCropActive(false);
        }
      } else if (e.key === "Escape") {
        if (cropActive) {
          setCropActive(false);
        } else {
          onCancel();
        }
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [currentTime, segments, cropActive, onCancel, frameDuration, metadata.duration]);

  return (
    <div className="h-full flex flex-col">
      {/* Toolbar */}
      <div className="frosted border-b border-border px-4 py-2 flex items-center gap-3 shrink-0">
        {/* Editable output name -- double-click to edit */}
        <div className="flex-1 min-w-0">
          {editingName ? (
            <input
              ref={nameInputRef}
              type="text"
              value={outputName}
              onChange={(e) => setOutputName(e.target.value)}
              onBlur={() => {
                if (!outputName.trim()) setOutputName(defaultOutputName);
                setEditingName(false);
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  if (!outputName.trim()) setOutputName(defaultOutputName);
                  setEditingName(false);
                } else if (e.key === "Escape") {
                  setOutputName(defaultOutputName);
                  setEditingName(false);
                }
              }}
              className="text-xs text-text-secondary bg-transparent w-full
                border border-white/15 rounded px-1.5 py-0.5 outline-none
                focus:border-white/25 caret-text-secondary"
              spellCheck={false}
            />
          ) : (
            <span
              className="text-xs text-text-secondary truncate block cursor-default
                border border-transparent rounded px-1.5 py-0.5"
              onDoubleClick={() => {
                setEditingName(true);
                requestAnimationFrame(() => {
                  const input = nameInputRef.current;
                  if (input) {
                    input.focus();
                    const len = input.value.length;
                    input.setSelectionRange(len, len);
                  }
                });
              }}
              title="Double-click to rename output"
            >
              {outputName}
            </span>
          )}
        </div>
        <div className="flex items-center gap-2">
          {/* Crop toggle + toolbar */}
          <CropToolbar
            active={cropActive}
            onToggle={() => setCropActive(!cropActive)}
            cropRect={cropRect}
            onCropRectChange={handleCropRectChange}
            videoWidth={metadata.width}
            videoHeight={metadata.height}
            onReset={() => {
              setCropRect({ x: 0, y: 0, width: 1, height: 1 });
              setOutputWidth(metadata.width);
              setOutputHeight(metadata.height);
            }}
          />

          {/* About button */}
          <button
            onClick={() => setAboutOpen(true)}
            className="px-2.5 py-1 rounded text-[11px] text-text-tertiary
              hover:text-text-secondary hover:bg-surface-hover transition-colors"
            title="About VideoBricks"
          >
            About
          </button>
        </div>
      </div>

      {/* Video player area */}
      <div ref={videoContainerRef} className="relative flex-1 min-h-0 bg-bg overflow-hidden">
        {/* Zoom wrapper: contains video + crop mask; transformed when crop preview is active */}
        <div
          className="absolute inset-0"
          style={cropPreviewTransform ? {
            ...cropPreviewTransform,
            transition: "transform 0.3s ease",
          } : undefined}
        >
          <VideoPlayer
            ref={videoRef}
            src={videoSrc}
            duration={metadata.duration}
            segments={segments}
            loop={settings.loopGif}
            bounce={settings.bounceGif}
            isPlaying={isPlaying && !isScrubbing}
            onPlayingChange={setIsPlaying}
            onTimeUpdate={setCurrentTime}
          />

          {/* Opaque crop mask (bg-bg) -- hides area outside crop when preview is active */}
          {/* Positioned over the video area; uses box-shadow to cover everything outside */}
          {hasCropPreview && (
            <div
              className="absolute pointer-events-none overflow-visible"
              style={{
                left: videoRect.left,
                top: videoRect.top,
                width: videoRect.width,
                height: videoRect.height,
              }}
            >
              <CropMask cropRect={cropRect} />
            </div>
          )}
        </div>

        {/* Crop editing overlay -- positioned over the video, NOT inside zoom wrapper */}
        {cropActive && (
          <div
            className="absolute z-10 overflow-hidden"
            style={{
              left: videoRect.left,
              top: videoRect.top,
              width: videoRect.width,
              height: videoRect.height,
            }}
          >
            <CropOverlay
              cropRect={cropRect}
              onCropRectChange={handleCropRectChange}
              videoWidth={metadata.width}
              videoHeight={metadata.height}
              onAccept={() => setCropActive(false)}
            />
          </div>
        )}
      </div>

      {/* Trim slider */}
      <div ref={timelineContainerRef} className="frosted border-t border-border shrink-0">
        <div className="flex items-center px-3 py-2 gap-2">
          {/* Play/Pause button */}
          <button
            onClick={() => {
              if (videoRef.current) {
                if (isPlaying) {
                  videoRef.current.pause();
                } else {
                  videoRef.current.play();
                }
                setIsPlaying(!isPlaying);
              }
            }}
            className="w-8 h-8 flex items-center justify-center rounded-md
              hover:bg-surface-hover transition-colors shrink-0"
          >
            {isPlaying ? (
              <svg className="w-4 h-4 text-text-primary" fill="currentColor" viewBox="0 0 24 24">
                <path d="M6 4h4v16H6V4zm8 0h4v16h-4V4z" />
              </svg>
            ) : (
              <svg className="w-4 h-4 text-text-primary" fill="currentColor" viewBox="0 0 24 24">
                <path d="M8 5v14l11-7z" />
              </svg>
            )}
          </button>

          {/* Timeline / Trim */}
          <TrimSlider
            duration={metadata.duration}
            segments={segments}
            onSegmentsChange={setSegments}
            currentTime={currentTime}
            thumbnails={thumbnails}
            detailThumbnails={detailThumbnails}
            onSeek={(time: number) => {
              setCurrentTime(time);
              if (videoRef.current) {
                videoRef.current.currentTime = time;
              }
            }}
            onScrubStart={handleScrubStart}
            onScrubEnd={handleScrubEnd}
            onRequestDetailThumbnails={(startTime: number, endTime: number) => {
              const container = timelineContainerRef.current;
              const containerWidth = container ? container.getBoundingClientRect().width : 600;
              const rawCount = Math.max(8, Math.ceil(containerWidth / 60));
              // Cap detail thumbnail count for long videos to keep generation fast
              const count = metadata.duration > 300 ? Math.min(rawCount, 12) : rawCount;
              api.getVideoThumbnailsRange(videoPath, count, 40, startTime, endTime)
                .then(setDetailThumbnails)
                .catch(() => {});
            }}
            onClearDetailThumbnails={() => setDetailThumbnails(null)}
          />
        </div>
      </div>

      {/* Settings panel */}
      <div className="bg-surface border-t border-border shrink-0">
        <div className="grid grid-cols-2 gap-0 divide-x divide-border">
          {/* Left column */}
          <div className="p-4 space-y-4">
            {/* Dimensions */}
            <DimensionsPicker
              videoWidth={metadata.width}
              videoHeight={metadata.height}
              cropWidth={cropPixelWidth}
              cropHeight={cropPixelHeight}
              outputWidth={outputWidth}
              outputHeight={outputHeight}
              arLocked={arLocked}
              onArLockedChange={setArLocked}
              onChange={handleDimensionsChange}
            />

            {/* Speed */}
            <div>
              <div className="flex items-center justify-between mb-1.5">
                <label className="text-xs font-medium text-text-secondary">
                  Speed
                </label>
                <EditableValue
                  value={settings.outputSpeed}
                  onChange={(v) => onSettingsChange({ ...settings, outputSpeed: v })}
                  min={SPEED_MIN}
                  max={SPEED_MAX}
                  step={SPEED_STEP}
                  format={(v) => `${v.toFixed(2)}x`}
                  className="text-xs tabular-nums text-text-primary font-medium"
                />
              </div>
              <input
                type="range"
                min={SPEED_MIN}
                max={SPEED_MAX}
                step={SPEED_STEP}
                value={settings.outputSpeed}
                onChange={(e) =>
                  onSettingsChange({
                    ...settings,
                    outputSpeed: parseFloat(e.target.value),
                  })
                }
                className="w-full"
              />
            </div>
          </div>

          {/* Right column */}
          <div className="p-4 space-y-4">
            {/* FPS */}
            <div>
              <div className="flex items-center justify-between mb-1.5">
                <label className="text-xs font-medium text-text-secondary">
                  FPS
                </label>
                <span className="text-xs tabular-nums font-medium">
                  <EditableValue
                    value={effectiveFps}
                    onChange={(v) => onSettingsChange({ ...settings, outputFps: v })}
                    min={FPS_MIN}
                    max={maxFps}
                    step={1}
                    parse={(s) => parseInt(s)}
                    className="text-text-primary"
                  />
                  <span className="text-text-tertiary font-normal ml-1">
                    / {maxFps}
                  </span>
                </span>
              </div>
              <input
                type="range"
                min={FPS_MIN}
                max={FPS_MAX}
                step={1}
                value={settings.outputFps}
                onChange={(e) =>
                  onSettingsChange({
                    ...settings,
                    outputFps: parseInt(e.target.value),
                  })
                }
                className="w-full"
              />
            </div>

            {/* Quality */}
            <div>
              <div className="flex items-center justify-between mb-1.5">
                <label className="text-xs font-medium text-text-secondary">
                  Quality
                </label>
                <EditableValue
                  value={Math.round(settings.outputQuality * 100)}
                  onChange={(v) => onSettingsChange({ ...settings, outputQuality: v / 100 })}
                  min={1}
                  max={100}
                  step={1}
                  parse={(s) => parseInt(s)}
                  format={(v) => `${v}%`}
                  className="text-xs tabular-nums text-text-primary font-medium"
                />
              </div>
              <input
                type="range"
                min={0.01}
                max={1}
                step={0.01}
                value={settings.outputQuality}
                onChange={(e) =>
                  onSettingsChange({
                    ...settings,
                    outputQuality: parseFloat(e.target.value),
                  })
                }
                className="w-full"
              />
            </div>

            {/* Loop + Bounce */}
            <div className="flex items-center gap-4">
              <label className="text-xs font-medium text-text-secondary">
                Loop
              </label>
              <div className="flex items-center gap-3 flex-1">
                {/* Loop count */}
                {!settings.loopGif && (
                  <input
                    type="number"
                    min={0}
                    max={100}
                    value={settings.loopCount}
                    onChange={(e) =>
                      onSettingsChange({
                        ...settings,
                        loopCount: Math.max(
                          0,
                          Math.min(100, parseInt(e.target.value) || 0)
                        ),
                      })
                    }
                    className="w-12 px-1.5 py-1 text-xs text-center border border-border rounded-md
                      bg-surface-raised text-text-primary focus:border-accent focus:outline-none"
                  />
                )}

                {/* Forever toggle */}
                <label className="flex items-center gap-1.5 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={settings.loopGif}
                    onChange={(e) =>
                      onSettingsChange({
                        ...settings,
                        loopGif: e.target.checked,
                      })
                    }
                    className="w-3.5 h-3.5 rounded border-border-strong text-accent focus:ring-accent/30"
                  />
                  <span className="text-xs text-text-secondary">Forever</span>
                </label>

                {/* Bounce toggle */}
                <label className="flex items-center gap-1.5 cursor-pointer">
                  <input
                    type="checkbox"
                    checked={settings.bounceGif}
                    onChange={(e) =>
                      onSettingsChange({
                        ...settings,
                        bounceGif: e.target.checked,
                      })
                    }
                    className="w-3.5 h-3.5 rounded border-border-strong text-accent focus:ring-accent/30"
                  />
                  <span className="text-xs text-text-secondary">Bounce</span>
                </label>

                {/* Merge/Split toggle -- only when multiple segments */}
                {segments.length > 1 && (
                  <div className="flex items-center gap-1.5">
                    <span className="text-[10px] text-text-tertiary">Output mode:</span>
                    <button
                      onClick={() => setSegmentMode(segmentMode === "merge" ? "split" : "merge")}
                      className={`px-2 py-0.5 rounded text-[10px] font-medium transition-colors ${
                        segmentMode === "merge"
                          ? "bg-accent/15 text-accent"
                          : "bg-orange-500/15 text-orange-400"
                      }`}
                    >
                      {segmentMode === "merge" ? "Merge" : "Split"}
                    </button>
                  </div>
                )}
              </div>
            </div>

            {/* Find Shots row */}
            <div className="flex items-center gap-2">
              {(() => {
                // ringProgress is driven by the animation loop effect above
                // SVG ring params
                const size = 14;
                const strokeWidth = 2;
                const radius = (size - strokeWidth) / 2;
                const circumference = 2 * Math.PI * radius;
                const dashOffset = circumference - (ringProgress / 100) * circumference;

                return (
                  <button
                    onClick={handleFindShots}
                    disabled={findingShotsLoading}
                    className={`px-3 py-1 rounded text-[11px] font-medium transition-all
                      ${findingShotsLoading
                        ? "bg-purple-700/18 text-purple-400/70 cursor-wait"
                        : "bg-purple-700/14 text-purple-400/70 hover:bg-purple-700/22"
                      }`}
                  >
                    <span className="flex items-center gap-1.5">
                      {findingShotsLoading ? (
                        <>
                          <svg width={size} height={size} className="shrink-0 -rotate-90">
                            <circle
                              cx={size / 2} cy={size / 2} r={radius}
                              fill="none" stroke="currentColor" strokeWidth={strokeWidth}
                              opacity={0.2}
                            />
                            <circle
                              cx={size / 2} cy={size / 2} r={radius}
                              fill="none" stroke="currentColor" strokeWidth={strokeWidth}
                              strokeDasharray={circumference}
                              strokeDashoffset={dashOffset}
                              strokeLinecap="round"
                              className="transition-all duration-500 ease-out"
                            />
                          </svg>
                          Find Shots
                        </>
                      ) : (
                        "Find Shots"
                      )}
                    </span>
                  </button>
                );
              })()}

              {/* Activation controls -- appear after Find Shots has been used */}
              <div
                className="flex items-center gap-1.5 overflow-hidden transition-all duration-300 ease-out"
                style={{
                  maxWidth: hasUsedFindShots ? "300px" : "0px",
                  opacity: hasUsedFindShots ? 1 : 0,
                }}
              >
                <button
                  onClick={allActive ? handleDeactivateAll : handleActivateAll}
                  className="px-2 py-0.5 rounded text-[10px] font-medium transition-colors
                    bg-purple-700/14 text-purple-400/70 hover:bg-purple-700/22 whitespace-nowrap"
                >
                  {allActive ? "Deactivate All" : "Activate All"}
                </button>
                <button
                  onClick={handleInverseActive}
                  className="px-2 py-0.5 rounded text-[10px] font-medium transition-colors
                    bg-purple-700/14 text-purple-400/70 hover:bg-purple-700/22"
                >
                  Inverse
                </button>
              </div>

              {/* Error display */}
              {findShotsError && (
                <span className="text-[10px] text-red-400 truncate max-w-[300px]" title={findShotsError}>
                  {findShotsError.length > 80 ? findShotsError.slice(0, 80) + "..." : findShotsError}
                </span>
              )}
            </div>
          </div>
        </div>
      </div>

      {/* Bottom bar */}
      <div className="frosted border-t border-border px-4 py-3 flex items-center justify-between shrink-0">
        <EstimatedSize
          settings={conversionSettings}
          duration={gifDuration}
        />
        <div className="flex items-center gap-3">
          <button
            onClick={onCancel}
            className="px-4 py-2 text-sm text-text-secondary hover:text-text-primary
              rounded-lg hover:bg-surface-hover transition-colors"
          >
            Cancel
          </button>
          <button
            onClick={handleConvertMp4}
            className="px-5 py-2 bg-surface border border-border text-text-primary rounded-lg text-sm font-medium
              hover:bg-surface-hover active:scale-[0.98] transition-all"
          >
            MP4
          </button>
          <button
            onClick={handleConvert}
            className="px-5 py-2 bg-accent text-white rounded-lg text-sm font-medium
              hover:bg-accent-hover active:scale-[0.98] transition-all
              shadow-sm hover:shadow-md"
          >
            GIF
          </button>
        </div>
      </div>

      {/* About modal */}
      <AboutModal open={aboutOpen} onClose={() => setAboutOpen(false)} />
    </div>
  );
}
