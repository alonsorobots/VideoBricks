import { useState, useCallback, useEffect } from "react";
import { motion } from "framer-motion";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import type { VideoMetadata } from "../lib/types";
import * as api from "../lib/tauri";

const VIDEO_EXTENSIONS = new Set([
  "mp4", "mov", "avi", "webm", "mkv", "m4v",
  "wmv", "flv", "gif", "3gp", "ogv", "ts",
]);

interface StartScreenProps {
  onVideoSelected: (path: string, metadata: VideoMetadata) => void;
}

export default function StartScreen({ onVideoSelected }: StartScreenProps) {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [isDragOver, setIsDragOver] = useState(false);

  const handleFile = useCallback(
    async (path: string) => {
      setLoading(true);
      setError(null);

      try {
        const result = await api.validateVideo(path);
        if (result.isValid && result.metadata) {
          onVideoSelected(path, result.metadata);
        } else {
          setError(result.error || "This file is not a valid video.");
        }
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
      } finally {
        setLoading(false);
      }
    },
    [onVideoSelected]
  );

  // Tauri drag-and-drop
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
          const ext = filePath.split(".").pop()?.toLowerCase() || "";
          if (VIDEO_EXTENSIONS.has(ext)) {
            handleFile(filePath);
          }
        }
      } else if (event.payload.type === "leave") {
        setIsDragOver(false);
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [handleFile]);

  const handleOpenFile = useCallback(async () => {
    const selected = await open({
      multiple: false,
      filters: [
        {
          name: "Video",
          extensions: [
            "mp4", "mov", "avi", "webm", "mkv", "m4v",
            "wmv", "flv", "gif", "3gp", "ogv", "ts",
          ],
        },
      ],
    });

    if (selected) {
      handleFile(selected);
    }
  }, [handleFile]);

  return (
    <div
      className={`h-full relative overflow-hidden cursor-default transition-all duration-200 ${isDragOver ? "ring-4 ring-inset ring-accent/50" : ""
        }`}
    >
      {/* Center content */}
      <div className="h-full flex items-center justify-center relative z-10">
        <motion.div
          initial={{ opacity: 0, y: 8 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.5 }}
          className="relative flex items-center justify-center"
        >
          {/* App logo -- large background */}
          <img
            src="/app-icon.png"
            alt="VideoBricks"
            className="w-[600px] h-[600px]"
            draggable={false}
          />

          {/* Text overlaid centered on the icon */}
          <div className="absolute inset-0 flex flex-col items-center justify-center">
            <p className="text-lg font-medium" style={{ color: "#81858b" }}>
              {loading
                ? "Validating..."
                : isDragOver
                  ? "Drop to open"
                  : "Drop a Video"}
            </p>

            <div className="mt-2 flex items-center gap-2 justify-center">
              <span className="text-lg font-medium" style={{ color: "#81858b" }}>or</span>
              <button
                onClick={handleOpenFile}
                disabled={loading}
                className="text-lg font-medium hover:opacity-80
                  transition-opacity disabled:opacity-50"
                style={{ color: "#81858b" }}
              >
                Open
              </button>
            </div>
          </div>
        </motion.div>

        {/* Error */}
        {error && (
          <motion.p
            initial={{ opacity: 0, y: 4 }}
            animate={{ opacity: 1, y: 0 }}
            className="absolute bottom-8 text-sm text-danger text-center max-w-sm px-4"
          >
            {error}
          </motion.p>
        )}
      </div>
    </div>
  );
}
