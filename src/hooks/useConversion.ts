import { useState, useCallback, useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import type { ConversionSettings, ConversionProgressEvent, GifResult } from "../lib/types";
import * as api from "../lib/tauri";

export type ConversionStatus = "idle" | "converting" | "completed" | "error" | "cancelled";

export function useConversion() {
  const [status, setStatus] = useState<ConversionStatus>("idle");
  const [progress, setProgress] = useState(0);
  const [gifs, setGifs] = useState<GifResult[]>([]);
  const [error, setError] = useState<string | null>(null);
  const startTimeRef = useRef(0);

  useEffect(() => {
    const unlisten = listen<ConversionProgressEvent>(
      "conversion-progress",
      (event) => {
        setProgress(event.payload.progress);
      }
    );
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const convert = useCallback(async (settings: ConversionSettings) => {
    setStatus("converting");
    setProgress(0);
    setError(null);
    setGifs([]);
    startTimeRef.current = Date.now();

    try {
      const results = await api.convertToGif(settings);
      setGifs(results);
      setStatus("completed");
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      if (msg.includes("cancelled")) {
        setStatus("cancelled");
      } else {
        setError(msg);
        setStatus("error");
      }
    }
  }, []);

  const cancel = useCallback(async () => {
    await api.cancelConversion();
    setStatus("cancelled");
  }, []);

  const reset = useCallback(() => {
    setStatus("idle");
    setProgress(0);
    setGifs([]);
    setError(null);
  }, []);

  const elapsedMs = status === "converting" ? Date.now() - startTimeRef.current : 0;

  return {
    status,
    progress,
    gifs,
    error,
    elapsedMs,
    convert,
    cancel,
    reset,
  };
}
