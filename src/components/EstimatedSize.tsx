import { useState, useEffect, useRef } from "react";
import type { ConversionSettings } from "../lib/types";
import * as api from "../lib/tauri";

interface EstimatedSizeProps {
  settings: ConversionSettings;
  duration: number;
}

export default function EstimatedSize({ settings, duration }: EstimatedSizeProps) {
  const [estimatedBytes, setEstimatedBytes] = useState<number | null>(null);
  const [loading, setLoading] = useState(false);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const lastSettingsRef = useRef<string>("");

  useEffect(() => {
    // Debounce estimation
    const key = JSON.stringify(settings);
    if (key === lastSettingsRef.current) return;
    lastSettingsRef.current = key;

    if (debounceRef.current) {
      clearTimeout(debounceRef.current);
    }

    debounceRef.current = setTimeout(async () => {
      setLoading(true);
      try {
        const size = await api.estimateFileSize(settings);
        setEstimatedBytes(size);
      } catch {
        // Silently fail - estimation is best-effort
      } finally {
        setLoading(false);
      }
    }, 1500);

    return () => {
      if (debounceRef.current) {
        clearTimeout(debounceRef.current);
      }
    };
  }, [settings]);

  // Simple naive estimate as fallback
  const naiveEstimate = (() => {
    const w = settings.width || 640;
    const h = settings.height || 480;
    const fps = settings.fps;
    const quality = settings.quality;
    // Very rough: ~50 bytes per pixel per quality per frame, compressed
    const bytesPerFrame = w * h * quality * 0.15;
    const totalFrames = duration * fps;
    return Math.round(bytesPerFrame * totalFrames);
  })();

  const displayBytes = estimatedBytes || naiveEstimate;

  return (
    <div className="flex items-center gap-2">
      {loading && (
        <div className="w-3 h-3 border-2 border-text-tertiary/30 border-t-text-tertiary rounded-full animate-spin" />
      )}
      <span className="text-xs text-text-secondary tabular-nums">
        ~{formatFileSize(displayBytes)}
      </span>
      <span className="text-xs text-text-tertiary">
        {formatDuration(duration)}
      </span>
    </div>
  );
}

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatDuration(seconds: number): string {
  if (seconds < 60) return `${seconds.toFixed(1)}s`;
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}m ${s}s`;
}
