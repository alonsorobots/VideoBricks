import { useRef, useState, useCallback, useEffect, useMemo } from "react";
import type { TrimSegment } from "../lib/types";
import { SEGMENT_COLORS } from "../lib/types";

interface TrimSliderProps {
  duration: number;
  segments: TrimSegment[];
  onSegmentsChange: (segments: TrimSegment[]) => void;
  currentTime: number;
  thumbnails: string[];
  detailThumbnails: string[] | null;
  onSeek: (time: number) => void;
  onScrubStart?: () => void;
  onScrubEnd?: () => void;
  onRequestDetailThumbnails?: (startTime: number, endTime: number) => void;
  onClearDetailThumbnails?: () => void;
}

type DragTarget =
  | { type: "start" | "end"; segmentIndex: number }
  | { type: "shared"; leftIndex: number; rightIndex: number; startX: number; resolved: boolean }
  | { type: "playhead" }
  | null;

/** Live tracking during drag overshoot */
interface DeleteIntent {
  segmentIndex: number;
  progress: number;        // 0..1 normalized for delete threshold
  anchorPct: number;       // the opposite handle's % position
  segExtentPct: number;    // the dragged handle's original % position (far edge)
  cursorPct: number;       // dampened cursor position as timeline %
  direction: "right" | "left";
}

/** Snap-back / delete animation state */
interface SnapBack {
  segmentIndex: number;
  leftPct: number;         // starting left edge of carpet
  rightPct: number;        // starting right edge of carpet
  direction: "right" | "left";
  deleteAfter: boolean;    // if true, remove segment after animation completes
  initialOpacity: number;  // starting opacity (mirrors the "coming in" value)
}

/** Live tracking during drag overshoot past adjacent segment */
interface MergeIntent {
  segmentIndex: number;       // the segment being dragged
  targetSegmentIndex: number; // the adjacent segment to merge with
  progress: number;           // 0..1 normalized for merge threshold
  anchorPct: number;          // the adjacent segment's handle % position (boundary)
  cursorPct: number;          // dampened cursor position as timeline %
  direction: "right" | "left";
}

/** Snap-back animation for merge (reuses shape of SnapBack but with merge semantics) */
interface MergeSnapBack {
  segmentIndex: number;
  targetSegmentIndex: number;
  leftPct: number;
  rightPct: number;
  direction: "right" | "left";
  commitAfter: boolean;       // if true, merge segments after animation completes
  initialOpacity: number;
}

const HANDLE_SNAP_PX = 10;
const HANDLE_WIDTH_PX = 10; // slightly thinner handles (was 12 = w-3)
const DELETE_THRESHOLD_PX = 100;
const MERGE_THRESHOLD_PX = 100;

/** Detail trim overlay state */
interface DetailMode {
  viewStart: number;   // start of zoomed time window
  viewEnd: number;     // end of zoomed time window
  handleTime: number;  // current handle time within zoomed view
  segmentIndex: number;
  edge: "start" | "end";
}

const DWELL_MS = 600;

export default function TrimSlider({
  duration,
  segments,
  onSegmentsChange,
  currentTime,
  thumbnails,
  detailThumbnails,
  onSeek,
  onScrubStart,
  onScrubEnd,
  onRequestDetailThumbnails,
  onClearDetailThumbnails,
}: TrimSliderProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [dragging, setDragging] = useState<DragTarget>(null);
  const [deleteIntent, setDeleteIntent] = useState<DeleteIntent | null>(null);
  const [snapBack, setSnapBack] = useState<SnapBack | null>(null);
  // Track if snap-back has started its animation (triggers width -> 0)
  const [snapStarted, setSnapStarted] = useState(false);

  // Merge intent: green carpet when handle crosses adjacent segment
  const [mergeIntent, setMergeIntent] = useState<MergeIntent | null>(null);
  const [mergeSnapBack, setMergeSnapBack] = useState<MergeSnapBack | null>(null);
  const [mergeSnapStarted, setMergeSnapStarted] = useState(false);

  // Two-phase merge: fade absorbed segment before removing it
  const [mergeFading, setMergeFading] = useState<string | null>(null); // absorbed segment id
  const segmentsRef = useRef(segments);
  segmentsRef.current = segments;
  const onSegmentsChangeRef = useRef(onSegmentsChange);
  onSegmentsChangeRef.current = onSegmentsChange;

  // Detail trim mode: overlay with zoomed thumbnails + playhead
  const [detailMode, setDetailMode] = useState<DetailMode | null>(null);
  const detailModeRef = useRef<DetailMode | null>(null);
  const dwellTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const prefetchTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const lastMouseXRef = useRef<number>(0);
  // "Pending" detail mode: computed early for prefetch, shown only once thumbnails arrive
  const pendingDetailRef = useRef<DetailMode | null>(null);

  // Proximity-based blending for adjacent handles.
  // Computes a 0-1 "proximity" for each handle that has a nearby neighbor,
  // driving smooth gradient blending as handles approach / separate.
  const handleProximity = useMemo(() => {
    const info: Record<string, { proximity: number; neighborColor: string; neighborIndex: number }> = {};
    const cw = containerRef.current?.getBoundingClientRect().width ?? 1;
    for (let i = 0; i < segments.length - 1; i++) {
      const leftActive = segments[i].active !== false;
      const rightActive = segments[i + 1].active !== false;
      // Don't blend when exactly one side is deactivated
      if (leftActive !== rightActive) continue;
      const endPx = (segments[i].end / duration) * cw;
      const startPx = (segments[i + 1].start / duration) * cw;
      const gapPx = startPx - endPx;
      // Blend starts when handles are within HANDLE_WIDTH_PX of each other
      if (gapPx < HANDLE_WIDTH_PX) {
        const proximity = Math.max(0, Math.min(1, 1 - gapPx / HANDLE_WIDTH_PX));
        const leftColor = SEGMENT_COLORS[i % SEGMENT_COLORS.length];
        const rightColor = SEGMENT_COLORS[(i + 1) % SEGMENT_COLORS.length];
        info[`end-${i}`] = { proximity, neighborColor: rightColor, neighborIndex: i + 1 };
        info[`start-${i + 1}`] = { proximity, neighborColor: leftColor, neighborIndex: i };
      }
    }
    return info;
  }, [segments, duration]);

  const timeToPercent = useCallback(
    (time: number) => (duration > 0 ? (time / duration) * 100 : 0),
    [duration]
  );

  const percentToTime = useCallback(
    (percent: number) => (percent / 100) * duration,
    [duration]
  );

  const getPercentFromEvent = useCallback((clientX: number) => {
    const container = containerRef.current;
    if (!container) return 0;
    const rect = container.getBoundingClientRect();
    const x = clientX - rect.left;
    return Math.max(0, Math.min(100, (x / rect.width) * 100));
  }, []);

  const getPxFromEvent = useCallback((clientX: number) => {
    const container = containerRef.current;
    if (!container) return 0;
    const rect = container.getBoundingClientRect();
    return clientX - rect.left;
  }, []);

  const getContainerWidth = useCallback(() => {
    const container = containerRef.current;
    return container ? container.getBoundingClientRect().width : 1;
  }, []);

  // Rubber-band dampening: carpet follows closely at first, exponentially resists further out
  // Returns dampened px for a given raw overshoot px
  const rubberBandPx = useCallback((rawPx: number): number => {
    const a = DELETE_THRESHOLD_PX * 0.55; // dampening factor — lower = more resistance
    return a * (1 - Math.exp(-rawPx / a));
  }, []);

  // Compute the detail view window for a given handle position
  const computeDetailWindow = useCallback(
    (handleTime: number, segmentIndex: number, edge: "start" | "end"): DetailMode => {
      const handlePct = handleTime / duration; // 0..1 proportion in full timeline
      const windowSize = duration / 2;
      let vStart = handleTime - handlePct * windowSize;
      let vEnd = vStart + windowSize;
      if (vStart < 0) { vStart = 0; vEnd = Math.min(duration, windowSize); }
      if (vEnd > duration) { vEnd = duration; vStart = Math.max(0, duration - windowSize); }
      return { viewStart: vStart, viewEnd: vEnd, handleTime, segmentIndex, edge };
    },
    [duration]
  );

  // Prefetch: start loading thumbnails early (called ~200ms into dwell)
  const prefetchDetailThumbnails = useCallback(
    (handleTime: number, segmentIndex: number, edge: "start" | "end") => {
      const dm = computeDetailWindow(handleTime, segmentIndex, edge);
      pendingDetailRef.current = dm;
      onRequestDetailThumbnails?.(dm.viewStart, dm.viewEnd);
    },
    [computeDetailWindow, onRequestDetailThumbnails]
  );

  // Activate: show overlay immediately at DWELL_MS, regardless of thumbnail readiness.
  // Thumbnails will crossfade in when they arrive.
  const activateDetailMode = useCallback(
    (handleTime: number, segmentIndex: number, edge: "start" | "end") => {
      if (detailModeRef.current) return;
      const dm = pendingDetailRef.current ?? computeDetailWindow(handleTime, segmentIndex, edge);
      // Update handle time to current position
      dm.handleTime = handleTime;
      detailModeRef.current = dm;
      setDetailMode(dm);
    },
    [computeDetailWindow]
  );

  // Exit detail trim mode
  const exitDetailMode = useCallback(() => {
    if (dwellTimerRef.current) { clearTimeout(dwellTimerRef.current); dwellTimerRef.current = null; }
    if (prefetchTimerRef.current) { clearTimeout(prefetchTimerRef.current); prefetchTimerRef.current = null; }
    detailModeRef.current = null;
    pendingDetailRef.current = null;
    setDetailMode(null);
    onClearDetailThumbnails?.();
  }, [onClearDetailThumbnails]);

  // Helper to start both prefetch + dwell timers for detail mode
  const startDetailTimers = useCallback(
    (handleTime: number, segmentIndex: number, edge: "start" | "end", clientX: number) => {
      if (prefetchTimerRef.current) clearTimeout(prefetchTimerRef.current);
      if (dwellTimerRef.current) clearTimeout(dwellTimerRef.current);
      lastMouseXRef.current = clientX;
      // Prefetch thumbnails early (200ms)
      prefetchTimerRef.current = setTimeout(() => {
        if (!detailModeRef.current) {
          prefetchDetailThumbnails(handleTime, segmentIndex, edge);
        }
      }, 200);
      // Activate overlay at dwell threshold (600ms) -- only if thumbs are ready
      dwellTimerRef.current = setTimeout(() => {
        if (!detailModeRef.current) {
          activateDetailMode(handleTime, segmentIndex, edge);
        }
      }, DWELL_MS);
    },
    [prefetchDetailThumbnails, activateDetailMode]
  );

  const handleHandleMouseDown = useCallback(
    (e: React.MouseEvent, segmentIndex: number, edge: "start" | "end") => {
      e.preventDefault();
      e.stopPropagation();
      setDragging({ type: edge, segmentIndex });
      setDeleteIntent(null);
      setSnapBack(null);
      exitDetailMode();

      const seg = segments[segmentIndex];
      const handleTime = edge === "start" ? seg.start : seg.end;
      startDetailTimers(handleTime, segmentIndex, edge, e.clientX);
    },
    [exitDetailMode, startDetailTimers, segments]
  );

  const handleSharedHandleMouseDown = useCallback(
    (e: React.MouseEvent, leftIndex: number, rightIndex: number) => {
      e.preventDefault();
      e.stopPropagation();
      setDragging({ type: "shared", leftIndex, rightIndex, startX: e.clientX, resolved: false });
      setDeleteIntent(null);
      setSnapBack(null);
      exitDetailMode();
    },
    [exitDetailMode]
  );

  const handleTrackMouseDown = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      const percent = getPercentFromEvent(e.clientX);
      const time = percentToTime(percent);
      const container = containerRef.current;

      if (container) {
        const rect = container.getBoundingClientRect();
        const clickX = e.clientX - rect.left;

        let closest: DragTarget = null;
        let closestDist = HANDLE_SNAP_PX;

        for (let i = 0; i < segments.length; i++) {
          const startPx = (timeToPercent(segments[i].start) / 100) * rect.width;
          const endPx = (timeToPercent(segments[i].end) / 100) * rect.width;

          const distToStart = Math.abs(clickX - startPx);
          if (distToStart < closestDist) {
            closestDist = distToStart;
            // If this start handle significantly overlaps its neighbor, use shared drag
            const blend = handleProximity[`start-${i}`];
            if (blend && blend.proximity > 0.5) {
              closest = { type: "shared", leftIndex: blend.neighborIndex, rightIndex: i, startX: e.clientX, resolved: false };
            } else {
              closest = { type: "start", segmentIndex: i };
            }
          }
          const distToEnd = Math.abs(clickX - endPx);
          if (distToEnd < closestDist) {
            closestDist = distToEnd;
            // If this end handle significantly overlaps its neighbor, use shared drag
            const blend = handleProximity[`end-${i}`];
            if (blend && blend.proximity > 0.5) {
              closest = { type: "shared", leftIndex: i, rightIndex: blend.neighborIndex, startX: e.clientX, resolved: false };
            } else {
              closest = { type: "end", segmentIndex: i };
            }
          }
        }

        if (closest) {
          setDragging(closest);
          setDeleteIntent(null);
          setSnapBack(null);
          exitDetailMode();

          // Start detail timers for individual handles (not shared -- direction not yet known)
          if (closest.type !== "shared") {
            const seg = segments[closest.segmentIndex];
            const handleTime = closest.type === "start" ? seg.start : seg.end;
            startDetailTimers(handleTime, closest.segmentIndex, closest.type, e.clientX);
          }
          return;
        }
      }

      setDragging({ type: "playhead" });
      onScrubStart?.();
      onSeek(Math.max(0, Math.min(duration, time)));
    },
    [getPercentFromEvent, percentToTime, timeToPercent, segments, duration, onSeek, onScrubStart, startDetailTimers, handleProximity, exitDetailMode]
  );

  useEffect(() => {
    if (!dragging) return;

    const handleMouseMove = (e: MouseEvent) => {
      const percent = getPercentFromEvent(e.clientX);
      const time = percentToTime(percent);

      if (dragging.type === "playhead") {
        onSeek(Math.max(0, Math.min(duration, time)));
        return;
      }

      // Resolve shared handle: determine direction on first move
      if (dragging.type === "shared" && !dragging.resolved) {
        const dx = e.clientX - dragging.startX;
        if (Math.abs(dx) < 2) return; // wait for meaningful movement

        if (dx > 0) {
          // Moving right -> adjusting right segment's start
          const newDrag = { type: "start" as const, segmentIndex: dragging.rightIndex };
          setDragging(newDrag);
          const rightSeg = segments[dragging.rightIndex];
          startDetailTimers(rightSeg.start, dragging.rightIndex, "start", e.clientX);
        } else {
          // Moving left -> adjusting left segment's end
          const newDrag = { type: "end" as const, segmentIndex: dragging.leftIndex };
          setDragging(newDrag);
          const leftSeg = segments[dragging.leftIndex];
          startDetailTimers(leftSeg.end, dragging.leftIndex, "end", e.clientX);
        }
        return;
      }

      if (dragging.type === "shared") return; // shouldn't happen, but guard

      const idx = dragging.segmentIndex;
      const seg = segments[idx];

      // --- Detail trim mode: scrub at 2x precision within overlay ---
      const dm = detailModeRef.current;
      if (dm && dm.segmentIndex === idx && dm.edge === dragging.type) {
        const detailPct = getPercentFromEvent(e.clientX);
        const detailRange = dm.viewEnd - dm.viewStart;
        const detailTime = dm.viewStart + (detailPct / 100) * detailRange;

        let newTime: number;
        if (dragging.type === "start") {
          const minStart = idx > 0 ? segments[idx - 1].end : 0;
          newTime = Math.max(minStart, Math.min(detailTime, seg.end - 0.1));
          const updated = [...segments];
          updated[idx] = { ...seg, start: newTime };
          onSegmentsChange(updated);
        } else {
          const maxEnd = idx < segments.length - 1 ? segments[idx + 1].start : duration;
          newTime = Math.min(maxEnd, Math.max(detailTime, seg.start + 0.1));
          const updated = [...segments];
          updated[idx] = { ...seg, end: newTime };
          onSegmentsChange(updated);
        }
        // Update overlay playhead
        const updatedDm = { ...dm, handleTime: newTime };
        detailModeRef.current = updatedDm;
        setDetailMode(updatedDm);
        onSeek(newTime);

        // Reset dwell timer (in case they stop again -- but we're already in detail)
        if (dwellTimerRef.current) clearTimeout(dwellTimerRef.current);
        lastMouseXRef.current = e.clientX;
        return;
      }

      // --- Normal handle drag ---
      const canDelete = segments.length > 1;
      const cw = getContainerWidth();

      if (dragging.type === "start") {
        const hasPrev = idx > 0;
        const minStart = hasPrev ? segments[idx - 1].end : 0;
        const anchorPct = timeToPercent(seg.end);
        const segExtentPct = timeToPercent(seg.start);

        if (canDelete && time > seg.end) {
          // 1) Delete intent: start handle crossed past own end handle
          setMergeIntent(null);
          const endPx = (anchorPct / 100) * cw;
          const cursorPx = getPxFromEvent(e.clientX);
          const overshootPx = Math.max(0, cursorPx - endPx);
          const progress = Math.min(1, overshootPx / DELETE_THRESHOLD_PX);
          const dampenedPx = rubberBandPx(overshootPx);
          const dampenedCursorPct = anchorPct + (dampenedPx / cw) * 100;

          if (progress >= 1.0) {
            const currentOpacity = Math.min(1, 0.6 + progress * 0.8);
            setDeleteIntent(null);
            setDragging(null);
            setSnapBack({ segmentIndex: idx, leftPct: segExtentPct, rightPct: dampenedCursorPct, direction: "right", deleteAfter: true, initialOpacity: currentOpacity });
            setSnapStarted(false);
            requestAnimationFrame(() => { requestAnimationFrame(() => setSnapStarted(true)); });
            if (dwellTimerRef.current) clearTimeout(dwellTimerRef.current);
            if (prefetchTimerRef.current) clearTimeout(prefetchTimerRef.current);
            return;
          }
          setDeleteIntent({ segmentIndex: idx, progress, anchorPct, segExtentPct, cursorPct: dampenedCursorPct, direction: "right" });
          if (dwellTimerRef.current) clearTimeout(dwellTimerRef.current);
          if (prefetchTimerRef.current) clearTimeout(prefetchTimerRef.current);
        } else if (hasPrev && time < minStart) {
          // 2) Merge intent: start handle crossed past previous segment's out-point
          setDeleteIntent(null);
          const boundaryPx = (timeToPercent(minStart) / 100) * cw;
          const cursorPx = getPxFromEvent(e.clientX);
          const overshootPx = Math.max(0, boundaryPx - cursorPx);
          const progress = Math.min(1, overshootPx / MERGE_THRESHOLD_PX);
          const dampenedPx = rubberBandPx(overshootPx);
          const boundaryPct = timeToPercent(minStart);
          const dampenedCursorPct = boundaryPct - (dampenedPx / cw) * 100;

          if (progress >= 1.0) {
            const currentOpacity = Math.min(1, 0.6 + progress * 0.8);
            setMergeIntent(null);
            setDragging(null);
            setMergeSnapBack({ segmentIndex: idx, targetSegmentIndex: idx - 1, leftPct: dampenedCursorPct, rightPct: boundaryPct, direction: "left", commitAfter: true, initialOpacity: currentOpacity });
            setMergeSnapStarted(false);
            requestAnimationFrame(() => { requestAnimationFrame(() => setMergeSnapStarted(true)); });
            if (dwellTimerRef.current) clearTimeout(dwellTimerRef.current);
            if (prefetchTimerRef.current) clearTimeout(prefetchTimerRef.current);
            return;
          }
          setMergeIntent({ segmentIndex: idx, targetSegmentIndex: idx - 1, progress, anchorPct: boundaryPct, cursorPct: dampenedCursorPct, direction: "left" });
          if (dwellTimerRef.current) clearTimeout(dwellTimerRef.current);
          if (prefetchTimerRef.current) clearTimeout(prefetchTimerRef.current);
        } else {
          // 3) Normal drag
          setDeleteIntent(null);
          setMergeIntent(null);
          const newStart = Math.max(minStart, Math.min(time, seg.end - 0.1));
          const updated = [...segments];
          updated[idx] = { ...seg, start: newStart };
          onSegmentsChange(updated);
          onSeek(newStart);

          // Dwell detection: reset timers on every movement
          startDetailTimers(newStart, idx, "start", e.clientX);
        }
      } else {
        const hasNext = idx < segments.length - 1;
        const maxEnd = hasNext ? segments[idx + 1].start : duration;
        const anchorPct = timeToPercent(seg.start);
        const segExtentPct = timeToPercent(seg.end);

        if (canDelete && time < seg.start) {
          // 1) Delete intent: end handle crossed past own start handle
          setMergeIntent(null);
          const startPx = (anchorPct / 100) * cw;
          const cursorPx = getPxFromEvent(e.clientX);
          const overshootPx = Math.max(0, startPx - cursorPx);
          const progress = Math.min(1, overshootPx / DELETE_THRESHOLD_PX);
          const dampenedPx = rubberBandPx(overshootPx);
          const dampenedCursorPct = anchorPct - (dampenedPx / cw) * 100;

          if (progress >= 1.0) {
            const currentOpacity = Math.min(1, 0.6 + progress * 0.8);
            setDeleteIntent(null);
            setDragging(null);
            setSnapBack({ segmentIndex: idx, leftPct: dampenedCursorPct, rightPct: segExtentPct, direction: "left", deleteAfter: true, initialOpacity: currentOpacity });
            setSnapStarted(false);
            requestAnimationFrame(() => { requestAnimationFrame(() => setSnapStarted(true)); });
            if (dwellTimerRef.current) clearTimeout(dwellTimerRef.current);
            if (prefetchTimerRef.current) clearTimeout(prefetchTimerRef.current);
            return;
          }
          setDeleteIntent({ segmentIndex: idx, progress, anchorPct, segExtentPct, cursorPct: dampenedCursorPct, direction: "left" });
          if (dwellTimerRef.current) clearTimeout(dwellTimerRef.current);
          if (prefetchTimerRef.current) clearTimeout(prefetchTimerRef.current);
        } else if (hasNext && time > maxEnd) {
          // 2) Merge intent: end handle crossed past next segment's in-point
          setDeleteIntent(null);
          const boundaryPx = (timeToPercent(maxEnd) / 100) * cw;
          const cursorPx = getPxFromEvent(e.clientX);
          const overshootPx = Math.max(0, cursorPx - boundaryPx);
          const progress = Math.min(1, overshootPx / MERGE_THRESHOLD_PX);
          const dampenedPx = rubberBandPx(overshootPx);
          const boundaryPct = timeToPercent(maxEnd);
          const dampenedCursorPct = boundaryPct + (dampenedPx / cw) * 100;

          if (progress >= 1.0) {
            const currentOpacity = Math.min(1, 0.6 + progress * 0.8);
            setMergeIntent(null);
            setDragging(null);
            setMergeSnapBack({ segmentIndex: idx, targetSegmentIndex: idx + 1, leftPct: boundaryPct, rightPct: dampenedCursorPct, direction: "right", commitAfter: true, initialOpacity: currentOpacity });
            setMergeSnapStarted(false);
            requestAnimationFrame(() => { requestAnimationFrame(() => setMergeSnapStarted(true)); });
            if (dwellTimerRef.current) clearTimeout(dwellTimerRef.current);
            if (prefetchTimerRef.current) clearTimeout(prefetchTimerRef.current);
            return;
          }
          setMergeIntent({ segmentIndex: idx, targetSegmentIndex: idx + 1, progress, anchorPct: boundaryPct, cursorPct: dampenedCursorPct, direction: "right" });
          if (dwellTimerRef.current) clearTimeout(dwellTimerRef.current);
          if (prefetchTimerRef.current) clearTimeout(prefetchTimerRef.current);
        } else {
          // 3) Normal drag
          setDeleteIntent(null);
          setMergeIntent(null);
          const newEnd = Math.min(maxEnd, Math.max(time, seg.start + 0.1));
          const updated = [...segments];
          updated[idx] = { ...seg, end: newEnd };
          onSegmentsChange(updated);
          onSeek(newEnd);

          // Dwell detection: reset timers on every movement
          startDetailTimers(newEnd, idx, "end", e.clientX);
        }
      }
    };

    const handleMouseUp = () => {
      // Clear dwell & prefetch timers
      if (dwellTimerRef.current) { clearTimeout(dwellTimerRef.current); dwellTimerRef.current = null; }
      if (prefetchTimerRef.current) { clearTimeout(prefetchTimerRef.current); prefetchTimerRef.current = null; }

      if (dragging.type === "shared") {
        // Released without moving -- just cancel
        setDragging(null);
        return;
      }

      if (dragging.type === "playhead") {
        onScrubEnd?.();
      } else if (deleteIntent) {
        const currentOpacity = Math.min(1, 0.6 + deleteIntent.progress * 0.8);
        const leftPct = deleteIntent.direction === "right"
          ? deleteIntent.segExtentPct
          : deleteIntent.cursorPct;
        const rightPct = deleteIntent.direction === "right"
          ? deleteIntent.cursorPct
          : deleteIntent.segExtentPct;
        setSnapBack({
          segmentIndex: deleteIntent.segmentIndex,
          leftPct,
          rightPct,
          direction: deleteIntent.direction,
          deleteAfter: false,
          initialOpacity: currentOpacity,
        });
        setSnapStarted(false);
        requestAnimationFrame(() => { requestAnimationFrame(() => setSnapStarted(true)); });
      } else if (mergeIntent) {
        // Snap-back without committing merge (user didn't drag far enough)
        const currentOpacity = Math.min(1, 0.6 + mergeIntent.progress * 0.8);
        const leftPct = mergeIntent.direction === "left"
          ? mergeIntent.cursorPct
          : mergeIntent.anchorPct;
        const rightPct = mergeIntent.direction === "left"
          ? mergeIntent.anchorPct
          : mergeIntent.cursorPct;
        setMergeSnapBack({
          segmentIndex: mergeIntent.segmentIndex,
          targetSegmentIndex: mergeIntent.targetSegmentIndex,
          leftPct,
          rightPct,
          direction: mergeIntent.direction,
          commitAfter: false,
          initialOpacity: currentOpacity,
        });
        setMergeSnapStarted(false);
        requestAnimationFrame(() => { requestAnimationFrame(() => setMergeSnapStarted(true)); });
      }
      // Exit detail mode on release -- the segment has already been updated
      exitDetailMode();
      setDeleteIntent(null);
      setMergeIntent(null);
      setDragging(null);
    };

    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);
    return () => {
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
    };
  }, [dragging, deleteIntent, mergeIntent, segments, duration, getPercentFromEvent, getPxFromEvent, getContainerWidth, percentToTime, timeToPercent, rubberBandPx, startDetailTimers, exitDetailMode, onSegmentsChange, onSeek, onScrubEnd]);

  // Clear snap-back after transition ends; delete segment if flagged
  // Filter to 'width' so it only fires once (left and opacity also transition)
  const handleSnapTransitionEnd = useCallback((e: React.TransitionEvent) => {
    if (e.propertyName !== "width") return;
    if (snapBack?.deleteAfter) {
      const filtered = segments.filter((_, i) => i !== snapBack.segmentIndex);
      onSegmentsChange(filtered);
    }
    setSnapBack(null);
    setSnapStarted(false);
  }, [snapBack, segments, onSegmentsChange]);

  // Clear merge snap-back after transition ends; begin two-phase merge if flagged
  const handleMergeSnapTransitionEnd = useCallback((e: React.TransitionEvent) => {
    if (e.propertyName !== "width") return;
    if (mergeSnapBack?.commitAfter) {
      const [a, b] = [mergeSnapBack.segmentIndex, mergeSnapBack.targetSegmentIndex].sort();
      const absorbedId = segments[b].id;

      // Phase 1: expand surviving segment to cover both, keep absorbed temporarily
      const expanded = {
        id: segments[a].id,
        start: segments[a].start,
        end: segments[b].end,
      };
      const updated = [...segments];
      updated[a] = expanded;
      onSegmentsChange(updated);

      // Trigger fade-out on absorbed segment (CSS transition handles the visual)
      setMergeFading(absorbedId);

      // Phase 2: after fade completes, remove absorbed segment
      setTimeout(() => {
        const current = segmentsRef.current;
        const filtered = current.filter(s => s.id !== absorbedId);
        if (filtered.length !== current.length) {
          onSegmentsChangeRef.current(filtered);
        }
        setMergeFading(null);
      }, 450);
    }
    setMergeSnapBack(null);
    setMergeSnapStarted(false);
  }, [mergeSnapBack, segments, onSegmentsChange]);

  // Build dim regions (exclude segment being deleted so visuals vanish instantly)
  const dimRegions = useMemo(() => {
    const regions: { left: string; width: string }[] = [];
    let cursor = 0;
    const visible = (snapBack?.deleteAfter)
      ? segments.filter((_, i) => i !== snapBack.segmentIndex)
      : segments;
    const sorted = [...visible].sort((a, b) => a.start - b.start);

    for (const seg of sorted) {
      const startPct = timeToPercent(seg.start);
      const cursorPct = timeToPercent(cursor);
      if (startPct > cursorPct) {
        regions.push({
          left: `${cursorPct}%`,
          width: `${startPct - cursorPct}%`,
        });
      }
      cursor = seg.end;
    }
    const endPct = timeToPercent(cursor);
    if (endPct < 100) {
      regions.push({ left: `${endPct}%`, width: `${100 - endPct}%` });
    }
    return regions;
  }, [segments, snapBack, timeToPercent]);

  const playheadPercent = Math.max(0, Math.min(100, timeToPercent(currentTime)));
  const totalDuration = segments.reduce((sum, s) => sum + (s.end - s.start), 0);

  // Compute the red delete tab geometry — spans entire segment + overshoot
  const deleteTab = useMemo(() => {
    if (deleteIntent) {
      const { segExtentPct, cursorPct, direction, progress } = deleteIntent;
      if (direction === "right") {
        // Carpet: from segment start (far edge) through to dampened cursor
        return { left: segExtentPct, width: Math.max(0, cursorPct - segExtentPct), direction, progress };
      } else {
        // Carpet: from dampened cursor through to segment end (far edge)
        return { left: cursorPct, width: Math.max(0, segExtentPct - cursorPct), direction, progress };
      }
    }
    return null;
  }, [deleteIntent]);

  // Compute the green merge tab geometry — spans from boundary to dampened cursor
  const mergeTab = useMemo(() => {
    if (mergeIntent) {
      const { anchorPct, cursorPct, direction, progress } = mergeIntent;
      if (direction === "right") {
        return { left: anchorPct, width: Math.max(0, cursorPct - anchorPct), direction, progress };
      } else {
        return { left: cursorPct, width: Math.max(0, anchorPct - cursorPct), direction, progress };
      }
    }
    return null;
  }, [mergeIntent]);

  return (
    <div
      ref={containerRef}
      className="relative flex-1 h-10 rounded-lg overflow-hidden cursor-pointer select-none"
      onMouseDown={handleTrackMouseDown}
    >
      {/* Thumbnail strip background */}
      <div className="absolute inset-0 flex">
        {thumbnails.length > 0
          ? thumbnails.map((thumb, i) => (
            <img
              key={i}
              src={thumb}
              alt=""
              className="h-full flex-1 object-cover"
              draggable={false}
            />
          ))
          : <div className="w-full h-full bg-gradient-to-r from-surface to-surface-raised" />
        }
      </div>

      {/* Dimmed areas outside all segments */}
      {dimRegions.map((r, i) => (
        <div
          key={`dim-${i}`}
          className="absolute inset-y-0 bg-black/50"
          style={{ left: r.left, width: r.width }}
        />
      ))}

      {/* Red delete tab -- "carpet" spans full segment + overshoot, above other handles but below active */}
      {deleteTab && (
        <div
          className="absolute inset-y-0 flex items-center justify-center pointer-events-none z-[11]"
          style={{
            left: `${deleteTab.left}%`,
            width: `${deleteTab.width}%`,
            backgroundColor: "rgb(220, 38, 38)",
            opacity: Math.min(1, 0.6 + deleteTab.progress * 0.8),
            borderRadius: "6px",
          }}
        >
          {deleteTab.progress > 0.3 && (
            <svg
              className="w-4 h-4"
              viewBox="0 0 24 24"
              fill="none"
              stroke="#fecaca"
              strokeWidth={2.5}
              strokeLinecap="round"
              style={{ opacity: Math.min(1, (deleteTab.progress - 0.3) / 0.4) }}
            >
              <line x1="6" y1="6" x2="18" y2="18" />
              <line x1="18" y1="6" x2="6" y2="18" />
            </svg>
          )}
        </div>
      )}

      {/* Snap-back animation tab (red / delete) */}
      {snapBack && (() => {
        const fullWidth = snapBack.rightPct - snapBack.leftPct;
        // "right": collapse right edge back (left stays, width -> 0)
        // "left": collapse left edge back (left -> rightPct, width -> 0)
        const left = snapBack.direction === "right"
          ? snapBack.leftPct
          : (snapStarted ? snapBack.rightPct : snapBack.leftPct);
        const width = snapStarted ? 0 : fullWidth;
        return (
          <div
            className="absolute inset-y-0 flex items-center justify-center pointer-events-none z-[15]"
            style={{
              left: `${left}%`,
              width: `${width}%`,
              backgroundColor: "rgb(220, 38, 38)",
              opacity: snapStarted ? 0 : snapBack.initialOpacity,
              borderRadius: "6px",
              transition: "width 0.4s cubic-bezier(0.175, 0.885, 0.32, 1.275), left 0.4s cubic-bezier(0.175, 0.885, 0.32, 1.275), opacity 0.4s cubic-bezier(0.175, 0.885, 0.32, 1.275)",
            }}
            onTransitionEnd={handleSnapTransitionEnd}
          />
        );
      })()}

      {/* Green merge tab -- "carpet" spans from boundary to dampened cursor, above other handles but below active */}
      {mergeTab && (
        <div
          className="absolute inset-y-0 flex items-center justify-center pointer-events-none z-[11]"
          style={{
            left: `${mergeTab.left}%`,
            width: `${mergeTab.width}%`,
            backgroundColor: "rgb(34, 197, 94)",
            opacity: Math.min(1, 0.6 + mergeTab.progress * 0.8),
            borderRadius: "6px",
          }}
        >
          {mergeTab.progress > 0.3 && (
            <svg
              className="w-4 h-4"
              viewBox="0 0 24 24"
              fill="none"
              stroke="#bbf7d0"
              strokeWidth={2.5}
              strokeLinecap="round"
              style={{ opacity: Math.min(1, (mergeTab.progress - 0.3) / 0.4) }}
            >
              <line x1="12" y1="6" x2="12" y2="18" />
              <line x1="6" y1="12" x2="18" y2="12" />
            </svg>
          )}
        </div>
      )}

      {/* Snap-back animation tab (green / merge) */}
      {mergeSnapBack && (() => {
        const fullWidth = mergeSnapBack.rightPct - mergeSnapBack.leftPct;
        const left = mergeSnapBack.direction === "right"
          ? mergeSnapBack.leftPct
          : (mergeSnapStarted ? mergeSnapBack.rightPct : mergeSnapBack.leftPct);
        const width = mergeSnapStarted ? 0 : fullWidth;
        return (
          <div
            className="absolute inset-y-0 flex items-center justify-center pointer-events-none z-[15]"
            style={{
              left: `${left}%`,
              width: `${width}%`,
              backgroundColor: "rgb(34, 197, 94)",
              opacity: mergeSnapStarted ? 0 : mergeSnapBack.initialOpacity,
              borderRadius: "6px",
              transition: "width 0.4s cubic-bezier(0.175, 0.885, 0.32, 1.275), left 0.4s cubic-bezier(0.175, 0.885, 0.32, 1.275), opacity 0.4s cubic-bezier(0.175, 0.885, 0.32, 1.275)",
            }}
            onTransitionEnd={handleMergeSnapTransitionEnd}
          />
        );
      })()}

      {/* Per-segment overlays, borders, and handles */}
      {segments.map((seg, i) => {
        const color = SEGMENT_COLORS[i % SEGMENT_COLORS.length];
        const startPct = timeToPercent(seg.start);
        const endPct = timeToPercent(seg.end);
        const widthPct = endPct - startPct;

        // Hide this segment immediately when its delete animation starts
        const isBeingDeleted = snapBack?.deleteAfter && snapBack.segmentIndex === i;
        if (isBeingDeleted) return null;

        // Is this segment being fade-absorbed by a merge?
        const isMergeFading = mergeFading === seg.id;

        // The actively dragged segment's handles sit ABOVE the carpet;
        // all other handles sit BELOW it.
        const isActiveSegment =
          (deleteIntent?.segmentIndex === i) ||
          (mergeIntent?.segmentIndex === i) ||
          (dragging && dragging.type !== "playhead" && dragging.type !== "shared" && dragging.segmentIndex === i);
        const handleZ = isActiveSegment ? "z-[13]" : "z-10";

        return (
          <div key={seg.id}>
            {/* Segment color overlay */}
            <div
              className="absolute inset-y-0"
              style={{
                left: `${startPct}%`,
                width: `${widthPct}%`,
                backgroundColor: color,
                opacity: isMergeFading ? 0 : (seg.active === false ? 0.06 : 0.18),
                transition: "background-color 0.4s ease, opacity 0.4s ease",
                cursor: "pointer",
              }}
              onDoubleClick={() => {
                const updated = segments.map((s, idx) =>
                  idx === i ? { ...s, active: s.active === false ? true : false } : s
                );
                onSegmentsChange(updated);
              }}
            />

            {/* Segment border top/bottom */}
            <div
              className="absolute inset-y-0 pointer-events-none"
              style={{
                left: `${startPct}%`,
                width: `${widthPct}%`,
                borderTop: `2px solid ${color}`,
                borderBottom: `2px solid ${color}`,
                opacity: isMergeFading ? 0 : (seg.active === false ? 0.3 : 1),
                transition: "border-color 0.4s ease, opacity 0.4s ease",
              }}
            />

            {/* Start handle -- gradient blends with neighbor when overlapping */}
            {(() => {
              const blend = handleProximity[`start-${i}`];
              const prox = blend ? blend.proximity : 0;
              // Gradient: neighbor color on the left (overlap side) -> own color
              const bg = prox > 0
                ? `linear-gradient(to right, ${blend!.neighborColor}, ${color} ${Math.round(prox * 100)}%)`
                : color;
              // When significantly overlapping, use shared handler for direction-based resolution
              const useShared = blend && prox > 0.5;
              return (
                <div
                  className={`absolute inset-y-0 cursor-ew-resize ${handleZ}
                    flex items-center justify-center rounded-md hover:opacity-90`}
                  style={{
                    width: `${HANDLE_WIDTH_PX}px`,
                    left: `calc(${startPct}% - ${HANDLE_WIDTH_PX / 2}px)`,
                    background: bg,
                    opacity: isMergeFading ? 0 : (seg.active === false ? 0.1 : 1),
                    transition: "opacity 0.4s ease",
                    pointerEvents: isMergeFading ? "none" : undefined,
                  }}
                  onMouseDown={(e) => useShared
                    ? handleSharedHandleMouseDown(e, blend!.neighborIndex, i)
                    : handleHandleMouseDown(e, i, "start")
                  }
                >
                  <div className="w-0.5 h-3.5 bg-black/15 rounded-full" />
                  <div className="w-0.5 h-3.5 bg-black/15 rounded-full ml-0.5" />
                </div>
              );
            })()}

            {/* End handle -- gradient blends with neighbor when overlapping */}
            {(() => {
              const blend = handleProximity[`end-${i}`];
              const prox = blend ? blend.proximity : 0;
              // Gradient: own color -> neighbor color on the right (overlap side)
              const solidStop = prox > 0 ? Math.round((1 - prox) * 100) : 100;
              const bg = prox > 0
                ? `linear-gradient(to right, ${color} ${solidStop}%, ${blend!.neighborColor})`
                : color;
              // When significantly overlapping, use shared handler for direction-based resolution
              const useShared = blend && prox > 0.5;
              return (
                <div
                  className={`absolute inset-y-0 cursor-ew-resize ${handleZ}
                    flex items-center justify-center rounded-md hover:opacity-90`}
                  style={{
                    width: `${HANDLE_WIDTH_PX}px`,
                    left: `calc(${endPct}% - ${HANDLE_WIDTH_PX / 2}px)`,
                    background: bg,
                    opacity: isMergeFading ? 0 : (seg.active === false ? 0.1 : 1),
                    transition: "opacity 0.4s ease",
                    pointerEvents: isMergeFading ? "none" : undefined,
                  }}
                  onMouseDown={(e) => useShared
                    ? handleSharedHandleMouseDown(e, i, blend!.neighborIndex)
                    : handleHandleMouseDown(e, i, "end")
                  }
                >
                  <div className="w-0.5 h-3.5 bg-black/15 rounded-full" />
                  <div className="w-0.5 h-3.5 bg-black/15 rounded-full ml-0.5" />
                </div>
              );
            })()}
          </div>
        );
      })}

      {/* No separate shared handles needed -- proximity-driven gradients on
          individual handles create smooth blending as they approach / separate. */}

      {/* Playhead */}
      <div
        className="absolute inset-y-0 w-0.5 bg-orange-400 z-20 shadow-md pointer-events-none"
        style={{ left: `${playheadPercent}%` }}
      >
        <div className="absolute -top-0.5 left-1/2 -translate-x-1/2 w-2.5 h-2.5 rounded-full bg-orange-400 shadow border border-orange-300" />
      </div>

      {/* Time labels */}
      <div className="absolute bottom-0 left-1 text-[9px] text-white/80 tabular-nums drop-shadow pointer-events-none">
        {formatTime(currentTime)}
      </div>
      <div className="absolute bottom-0 right-1 text-[9px] text-white/80 tabular-nums drop-shadow pointer-events-none">
        {formatTime(totalDuration)}
      </div>

      {/* Detail trim overlay -- zoomed thumbnails + handle + playhead on top of everything */}
      {detailMode && (() => {
        const { viewStart, viewEnd, handleTime, segmentIndex, edge } = detailMode;
        const detailRange = viewEnd - viewStart;
        const handlePct = detailRange > 0 ? ((handleTime - viewStart) / detailRange) * 100 : 50;
        const clampedPct = Math.max(0, Math.min(100, handlePct));
        const hasDetailThumbs = detailThumbnails && detailThumbnails.length > 0;
        const handleColor = SEGMENT_COLORS[segmentIndex % SEGMENT_COLORS.length];

        // Show the segment region within the zoomed view
        const seg = segments[segmentIndex];
        const segStartPct = detailRange > 0 ? ((seg.start - viewStart) / detailRange) * 100 : 0;
        const segEndPct = detailRange > 0 ? ((seg.end - viewStart) / detailRange) * 100 : 100;

        // Compute CSS transform to show the detail window range using the main thumbnails
        const mainThumbCount = thumbnails.length;
        const totalTimelinePx = mainThumbCount > 0 ? mainThumbCount : 1;
        // Map detail window to main thumbnail positions (0..1 of timeline)
        const detailStartFrac = viewStart / duration;
        const detailEndFrac = viewEnd / duration;
        const detailFracRange = detailEndFrac - detailStartFrac;
        // Scale: how much to zoom the main strip to fill the overlay
        const placeholderScaleX = detailFracRange > 0 ? 1 / detailFracRange : 1;
        // Translate: shift so the detail window start aligns with left edge
        const placeholderTx = -(detailStartFrac * placeholderScaleX * 100);

        return (
          <div className="absolute inset-0 z-[30] rounded-lg overflow-hidden pointer-events-none">
            {/* Placeholder: CSS-scaled main thumbnails (instant, blurry but recognizable) */}
            <div
              className="absolute inset-0 flex"
              style={{
                transform: `scaleX(${placeholderScaleX}) translateX(${placeholderTx}%)`,
                transformOrigin: "0 0",
                width: `${100 / placeholderScaleX}%`,
                opacity: hasDetailThumbs ? 0 : 1,
                transition: "opacity 200ms ease-out",
              }}
            >
              {thumbnails.map((thumb, i) => (
                <img
                  key={`placeholder-${i}`}
                  src={thumb}
                  alt=""
                  className="h-full flex-1 object-cover"
                  draggable={false}
                />
              ))}
            </div>

            {/* Real detail thumbnails: fade in on top when they arrive */}
            {hasDetailThumbs && (
              <div
                className="absolute inset-0 flex detail-thumb-fadein"
                style={{ animation: "detailFadeIn 200ms ease-out forwards" }}
              >
                {detailThumbnails.map((thumb, i) => (
                  <img
                    key={`detail-${i}`}
                    src={thumb}
                    alt=""
                    className="h-full flex-1 object-cover"
                    draggable={false}
                  />
                ))}
              </div>
            )}

            {/* Subtle loading indicator while thumbnails load */}
            {!hasDetailThumbs && (
              <div className="absolute top-0 left-0 right-0 h-0.5 overflow-hidden">
                <div
                  className="h-full bg-accent/60 rounded-full"
                  style={{
                    width: "30%",
                    animation: "detailLoadingSlide 1s ease-in-out infinite",
                  }}
                />
              </div>
            )}

            {/* Segment region tint + borders in zoomed view */}
            <div
              className="absolute inset-y-0"
              style={{
                left: `${segStartPct}%`,
                width: `${segEndPct - segStartPct}%`,
                backgroundColor: handleColor,
                opacity: 0.18,
              }}
            />
            <div
              className="absolute inset-y-0 pointer-events-none"
              style={{
                left: `${segStartPct}%`,
                width: `${segEndPct - segStartPct}%`,
                borderTop: `2px solid ${handleColor}`,
                borderBottom: `2px solid ${handleColor}`,
              }}
            />

            {/* The handle widget being dragged */}
            <div
              className="absolute inset-y-0 w-3 flex items-center justify-center rounded-md z-10"
              style={{
                left: `calc(${clampedPct}% - 6px)`,
                backgroundColor: handleColor,
              }}
            >
              <div className="w-0.5 h-3.5 bg-black/15 rounded-full" />
              <div className="w-0.5 h-3.5 bg-black/15 rounded-full ml-0.5" />
            </div>

            {/* Playhead line at handle position */}
            <div
              className="absolute inset-y-0 w-0.5 bg-orange-400 z-20 shadow-md"
              style={{ left: `${clampedPct}%` }}
            >
              <div className="absolute -top-0.5 left-1/2 -translate-x-1/2 w-2.5 h-2.5 rounded-full bg-orange-400 shadow border border-orange-300" />
            </div>

            {/* Purple dim outside of segment region */}
            {segStartPct > 0 && (
              <div
                className="absolute inset-y-0 pointer-events-none"
                style={{ left: 0, width: `${segStartPct}%`, backgroundColor: "rgba(30, 7, 66, 0.64)" }}
              />
            )}
            {segEndPct < 100 && (
              <div
                className="absolute inset-y-0 pointer-events-none"
                style={{ left: `${segEndPct}%`, width: `${100 - segEndPct}%`, backgroundColor: "rgba(109, 40, 217, 0.25)" }}
              />
            )}

            {/* Centered label */}
            <div className="absolute inset-0 flex items-center justify-center pointer-events-none">
              <span className="text-xs text-white/65 font-semibold tracking-widest drop-shadow pointer-events-none">
                detail trim mode
              </span>
            </div>

            {/* Time range labels */}
            <div className="absolute bottom-0 left-1 text-[9px] text-white/80 tabular-nums drop-shadow pointer-events-none">
              {formatTime(handleTime)}
            </div>
            <div className="absolute bottom-0 right-1 text-[9px] text-white/80 tabular-nums drop-shadow pointer-events-none">
              {formatTime(viewStart)} - {formatTime(viewEnd)}
            </div>
          </div>
        );
      })()}
    </div>
  );
}

function formatTime(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  const ms = Math.floor((seconds % 1) * 10);
  return `${m}:${s.toString().padStart(2, "0")}.${ms}`;
}
