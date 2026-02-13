import { useState, useCallback, useMemo, useRef } from "react";
import { motion } from "framer-motion";
import { save, open } from "@tauri-apps/plugin-dialog";
import { convertFileSrc } from "@tauri-apps/api/core";
import type { GifResult, Mp4Result, ExportFormat } from "../lib/types";
import { SEGMENT_COLORS } from "../lib/types";
import * as api from "../lib/tauri";

interface CompletedScreenProps {
  gifs: GifResult[];
  mp4s: Mp4Result[];
  format: ExportFormat;
  sourceVideoPath: string;
  /** User-defined output name (no extension). Falls back to source filename if empty. */
  outputName?: string;
  onNewConversion: () => void;
  onBackToEdit: () => void;
}

export default function CompletedScreen({
  gifs,
  mp4s,
  format,
  sourceVideoPath,
  outputName: outputNameProp,
  onNewConversion,
  onBackToEdit,
}: CompletedScreenProps) {
  const [saving, setSaving] = useState(false);
  const [copied, setCopied] = useState(false);
  const [savedPath, setSavedPath] = useState<string | null>(null);

  const isGif = format === "gif";
  const isMp4 = format === "mp4";

  // Unified items list
  const items = isGif ? gifs : mp4s;
  const isMultiple = items.length > 1;
  const MAX_VISIBLE = 6;
  const visibleItems = isMultiple ? items.slice(0, MAX_VISIBLE) : items;
  const hasOverflow = items.length > MAX_VISIBLE;

  // Use user-defined output name, or derive from source video filename
  const baseName = useMemo(() => {
    if (outputNameProp) return outputNameProp;
    const filename = sourceVideoPath.split("\\").pop()?.split("/").pop() || "animation";
    return filename.replace(/\.[^.]+$/, "");
  }, [outputNameProp, sourceVideoPath]);

  const ext = isGif ? "gif" : "mp4";
  const defaultSaveName = `${baseName}.${ext}`;

  // Total file size
  const totalSize = items.reduce((sum, item) => sum + item.fileSize, 0);

  // Use refs for the save callbacks so drag handlers always see the latest version
  const savingRef = useRef(false);

  // --- Single-file save ---
  const handleSaveSingle = useCallback(async () => {
    if (savingRef.current) return;
    savingRef.current = true;
    setSaving(true);

    try {
      if (isGif) {
        const path = await save({
          filters: [{ name: "GIF Image", extensions: ["gif"] }],
          defaultPath: defaultSaveName,
        });
        if (!path) return;
        await api.saveGifFile(path);
        setSavedPath(path);
      } else {
        // MP4: copy from temp to user-chosen location
        const path = await save({
          filters: [{ name: "MP4 Video", extensions: ["mp4"] }],
          defaultPath: defaultSaveName,
        });
        if (!path) return;
        await api.copyFile(mp4s[0].filePath, path);
        setSavedPath(path);
      }
    } catch (err) {
      console.error("Save failed:", err);
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  }, [isGif, defaultSaveName, mp4s]);

  // --- Save All to folder ---
  const handleSaveAll = useCallback(async () => {
    if (savingRef.current) return;
    savingRef.current = true;
    setSaving(true);

    try {
      const dir = await open({
        directory: true,
        title: `Choose folder for ${ext.toUpperCase()} files`,
      });
      if (!dir) return;

      if (isGif) {
        const paths = await api.saveAllGifFiles(dir as string, baseName);
        setSavedPath(`${paths.length} files saved`);
      } else {
        // MP4: copy all temp files to chosen directory
        const sources = mp4s.map((m) => m.filePath);
        const paths = await api.copyFilesToDirectory(sources, dir as string, baseName);
        setSavedPath(`${paths.length} files saved`);
      }
    } catch (err) {
      console.error("Save all failed:", err);
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  }, [isGif, baseName, ext, mp4s]);

  // Store latest save functions in refs so drag handlers are never stale
  const handleSaveSingleRef = useRef(handleSaveSingle);
  handleSaveSingleRef.current = handleSaveSingle;
  const handleSaveAllRef = useRef(handleSaveAll);
  handleSaveAllRef.current = handleSaveAll;

  // --- GIF drag handler ---
  const handleGifDragStart = useCallback(
    (e: React.DragEvent<HTMLImageElement>, index: number) => {
      if (isMultiple) {
        // In split mode: initiate Save All to Folder dialog
        e.preventDefault();
        handleSaveAllRef.current();
        return;
      }
      const name = defaultSaveName;
      e.dataTransfer.setData(
        "DownloadURL",
        `image/gif:${name}:${gifs[index].dataUrl}`
      );
    },
    [defaultSaveName, gifs, isMultiple]
  );

  // --- MP4 drag handler ---
  const handleMp4DragStart = useCallback(
    (e: React.DragEvent<HTMLVideoElement>) => {
      // DownloadURL doesn't reliably support asset:// URLs for MP4
      // so we intercept and open a save dialog instead
      e.preventDefault();
      if (isMultiple) {
        handleSaveAllRef.current();
      } else {
        handleSaveSingleRef.current();
      }
    },
    [isMultiple]
  );

  // --- Generic click-to-save handler for thumbnails ---
  const handleThumbnailClick = useCallback(() => {
    if (isMultiple) {
      handleSaveAllRef.current();
    } else {
      handleSaveSingleRef.current();
    }
  }, [isMultiple]);

  const handleCopy = useCallback(async () => {
    try {
      await api.copyGifToClipboard();
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error("Copy failed:", err);
    }
  }, []);

  // MP4: reveal file in system explorer
  const handleRevealInExplorer = useCallback(async (filePath: string) => {
    try {
      await api.revealInExplorer(filePath);
    } catch (err) {
      console.error("Failed to open explorer:", err);
    }
  }, []);

  // --- Thumbnail rendering ---
  const renderThumbnail = (item: GifResult | Mp4Result, index: number) => {
    const isLast = isMultiple && hasOverflow && index === visibleItems.length - 1;
    const fadeOpacity = isMultiple && hasOverflow
      ? Math.min(0.8, (index / (visibleItems.length - 1)) * 0.8)
      : 0;

    return (
      <div
        key={index}
        className={isMultiple
          ? "relative rounded-lg overflow-hidden shadow-xl cursor-pointer"
          : "relative max-w-md max-h-[340px] rounded-xl overflow-hidden shadow-2xl cursor-pointer"
        }
        style={isMultiple ? {
          borderBottom: `3px solid ${SEGMENT_COLORS[index % SEGMENT_COLORS.length]}`,
        } : undefined}
        onClick={handleThumbnailClick}
      >
        <div className="checkerboard">
          {isGif ? (
            <img
              src={(item as GifResult).dataUrl}
              alt={isMultiple ? `GIF segment ${index + 1}` : "Converted GIF"}
              className={isMultiple ? "max-h-[200px] object-contain" : "max-w-full max-h-[340px] object-contain"}
              draggable
              onDragStart={(e) => handleGifDragStart(e, index)}
            />
          ) : (
            <video
              src={convertFileSrc((item as Mp4Result).filePath)}
              className={isMultiple ? "max-h-[200px] object-contain" : "max-w-full max-h-[340px] object-contain"}
              autoPlay
              loop
              muted
              playsInline
              draggable
              onDragStart={handleMp4DragStart}
            />
          )}
        </div>

        {/* Segment number badge */}
        {isMultiple && (
          <div
            className="absolute bottom-1 left-1 text-[9px] font-medium px-1.5 py-0.5 rounded z-10"
            style={{
              backgroundColor: SEGMENT_COLORS[index % SEGMENT_COLORS.length],
              color: "#000",
            }}
          >
            {index + 1}
          </div>
        )}

        {/* Progressive fade overlay for overflow */}
        {isMultiple && fadeOpacity > 0 && (
          <div
            className="absolute inset-0 pointer-events-none flex items-center justify-center"
            style={{ backgroundColor: `rgba(37, 41, 49, ${fadeOpacity})` }}
          >
            {isLast && (
              <span className="text-text-secondary text-lg font-semibold">
                +{items.length - MAX_VISIBLE} more
              </span>
            )}
          </div>
        )}
      </div>
    );
  };

  return (
    <div className="h-full flex flex-col items-center justify-center p-8">
      {/* Thumbnail(s) */}
      <motion.div
        initial={{ scale: 0.9, opacity: 0 }}
        animate={{ scale: 1, opacity: 1 }}
        transition={{ duration: 0.4, type: "spring", stiffness: 200 }}
        className={isMultiple
          ? "flex flex-wrap items-center justify-center gap-3 max-w-full"
          : undefined
        }
      >
        {visibleItems.map((item, i) => renderThumbnail(item, i))}
      </motion.div>

      {/* File size */}
      <motion.p
        initial={{ opacity: 0, y: 4 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ delay: 0.2 }}
        className="mt-4 text-sm text-text-secondary tabular-nums"
      >
        {isMultiple
          ? `${items.length} ${ext.toUpperCase()}s -- ${formatFileSize(totalSize)} total`
          : formatFileSize(totalSize)
        }
      </motion.p>

      {/* Saved path */}
      {savedPath && (
        <motion.p
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          className="mt-1 text-xs text-success"
        >
          {savedPath.includes("files saved")
            ? savedPath
            : `Saved to ${savedPath.split("\\").pop()?.split("/").pop()}`
          }
        </motion.p>
      )}

      {/* Action buttons */}
      <motion.div
        initial={{ opacity: 0, y: 8 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ delay: 0.3, duration: 0.3 }}
        className="mt-6 flex items-center gap-3"
      >
        <button
          onClick={isMultiple ? handleSaveAll : handleSaveSingle}
          disabled={saving}
          className="px-5 py-2.5 bg-accent text-white rounded-lg text-sm font-medium
            hover:bg-accent-hover active:scale-[0.98] transition-all
            shadow-sm hover:shadow-md disabled:opacity-50"
        >
          {saving
            ? "Saving..."
            : isMultiple
              ? "Save All to Folder"
              : "Save to File"
          }
        </button>

        {isGif && !isMultiple && (
          <button
            onClick={handleCopy}
            className="px-5 py-2.5 border border-border rounded-lg text-sm font-medium
              text-text-primary hover:bg-surface active:scale-[0.98] transition-all"
          >
            {copied ? "Copied!" : "Copy to Clipboard"}
          </button>
        )}

        {isMp4 && !isMultiple && (
          <button
            onClick={() => {
              const filePath = mp4s[0]?.filePath;
              if (filePath) handleRevealInExplorer(filePath);
            }}
            className="px-5 py-2.5 border border-border rounded-lg text-sm font-medium
              text-text-primary hover:bg-surface active:scale-[0.98] transition-all"
          >
            Show in Explorer
          </button>
        )}
      </motion.div>

      {/* Back to edit + New conversion */}
      <motion.div
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ delay: 0.5 }}
        className="mt-6 flex items-center gap-4"
      >
        <button
          onClick={onBackToEdit}
          className="text-sm text-text-secondary hover:text-text-primary transition-colors
            flex items-center gap-1.5"
        >
          <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M15 19l-7-7 7-7" />
          </svg>
          Back to Edit
        </button>

        <span className="text-text-tertiary">|</span>

        <button
          onClick={onNewConversion}
          className="text-sm text-accent hover:text-accent-hover transition-colors"
        >
          New Conversion
        </button>
      </motion.div>

      {/* Drag hint */}
      <motion.p
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ delay: 0.7 }}
        className="mt-4 text-[10px] text-text-tertiary"
      >
        {isMultiple
          ? `Drag or click any thumbnail to save all ${items.length} files`
          : isGif
            ? `Drag the image to save as ${defaultSaveName}`
            : `Drag the video to save as ${defaultSaveName}`
        }
      </motion.p>
    </div>
  );
}

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
