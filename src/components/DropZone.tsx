import { useState, useCallback, useEffect } from "react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

interface DropZoneProps {
  onFileDrop: (path: string) => void;
  loading?: boolean;
}

const VIDEO_EXTENSIONS = new Set([
  "mp4", "mov", "avi", "webm", "mkv", "m4v",
  "wmv", "flv", "gif", "3gp", "ogv", "ts",
]);

export default function DropZone({ onFileDrop, loading }: DropZoneProps) {
  const [isDragOver, setIsDragOver] = useState(false);

  useEffect(() => {
    const webview = getCurrentWebviewWindow();

    const unlisten = webview.onDragDropEvent((event) => {
      if (event.payload.type === "enter" || event.payload.type === "over") {
        setIsDragOver(true);
      } else if (event.payload.type === "drop") {
        setIsDragOver(false);
        const paths = event.payload.paths;
        if (paths && paths.length > 0) {
          const filePath = paths[0];
          // Check extension
          const ext = filePath.split(".").pop()?.toLowerCase() || "";
          if (VIDEO_EXTENSIONS.has(ext)) {
            onFileDrop(filePath);
          }
        }
      } else if (event.payload.type === "leave") {
        setIsDragOver(false);
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [onFileDrop]);

  return (
    <div
      className={`
        relative rounded-2xl border-2 border-dashed
        transition-all duration-200 ease-out
        flex flex-col items-center justify-center
        py-16 px-8 cursor-default
        ${
          isDragOver
            ? "border-accent bg-accent/5 scale-[1.01] shadow-lg"
            : "border-border-strong hover:border-text-tertiary bg-surface"
        }
        ${loading ? "opacity-60 pointer-events-none" : ""}
      `}
    >
      {/* Icon */}
      <div
        className={`
          w-12 h-12 rounded-xl flex items-center justify-center mb-4
          transition-colors duration-200
          ${isDragOver ? "bg-accent/10" : "bg-surface-raised"}
        `}
      >
        {loading ? (
          <svg
            className="w-6 h-6 text-text-tertiary animate-spin"
            fill="none"
            viewBox="0 0 24 24"
          >
            <circle
              className="opacity-25"
              cx="12"
              cy="12"
              r="10"
              stroke="currentColor"
              strokeWidth="4"
            />
            <path
              className="opacity-75"
              fill="currentColor"
              d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"
            />
          </svg>
        ) : (
          <svg
            className={`w-6 h-6 transition-colors ${
              isDragOver ? "text-accent" : "text-text-tertiary"
            }`}
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
            strokeWidth={1.5}
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              d="M3 16.5v2.25A2.25 2.25 0 005.25 21h13.5A2.25 2.25 0 0021 18.75V16.5m-13.5-9L12 3m0 0l4.5 4.5M12 3v13.5"
            />
          </svg>
        )}
      </div>

      {/* Text */}
      <p
        className={`text-sm font-medium transition-colors ${
          isDragOver ? "text-accent" : "text-text-primary"
        }`}
      >
        {loading
          ? "Validating video..."
          : isDragOver
          ? "Drop to open"
          : "Drop a Video Here"}
      </p>
      <p className="text-xs text-text-tertiary mt-1">
        {loading ? "Please wait" : "MP4, MOV, AVI, WebM, MKV"}
      </p>

      {/* Animated border highlight */}
      {isDragOver && (
        <div className="absolute inset-0 rounded-2xl border-2 border-accent animate-pulse pointer-events-none" />
      )}
    </div>
  );
}
