import { useState, useCallback } from "react";
import type { VideoMetadata, ValidateResult } from "../lib/types";
import * as api from "../lib/tauri";

export function useVideoMeta() {
  const [metadata, setMetadata] = useState<VideoMetadata | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const validate = useCallback(async (path: string): Promise<ValidateResult> => {
    setLoading(true);
    setError(null);
    try {
      const result = await api.validateVideo(path);
      if (result.isValid && result.metadata) {
        setMetadata(result.metadata);
      } else {
        setError(result.error || "Invalid video file");
      }
      setLoading(false);
      return result;
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setError(msg);
      setLoading(false);
      return { isValid: false, error: msg, metadata: null };
    }
  }, []);

  const reset = useCallback(() => {
    setMetadata(null);
    setError(null);
    setLoading(false);
  }, []);

  return { metadata, loading, error, validate, reset };
}
