import { useState, useEffect, useCallback, useRef } from "react";
import { motion } from "framer-motion";
import CircularProgress from "../components/CircularProgress";

interface ConversionScreenProps {
  progress: number; // 0-1
  format?: "gif" | "mp4";
  onCancel: () => void;
}

export default function ConversionScreen({
  progress,
  format = "gif",
  onCancel,
}: ConversionScreenProps) {
  const startTimeRef = useRef(Date.now());
  const [eta, setEta] = useState<string | null>(null);
  const [showEta, setShowEta] = useState(false);

  // Calculate ETA
  useEffect(() => {
    const elapsed = (Date.now() - startTimeRef.current) / 1000;

    // Show ETA after 3 seconds, only if total estimated > 10s
    if (elapsed > 3 && progress > 0) {
      const totalEstimated = elapsed / progress;
      const remaining = totalEstimated - elapsed;

      if (totalEstimated > 10) {
        setShowEta(true);
        if (remaining < 60) {
          setEta(`About ${Math.ceil(remaining)}s remaining`);
        } else {
          const mins = Math.ceil(remaining / 60);
          setEta(`About ${mins}m remaining`);
        }
      }
    }
  }, [progress]);

  // Escape to cancel
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onCancel();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onCancel]);

  const percentage = Math.round(progress * 100);

  return (
    <div className="h-full flex flex-col items-center justify-center p-8">
      {/* Progress ring */}
      <motion.div
        initial={{ scale: 0.8, opacity: 0 }}
        animate={{ scale: 1, opacity: 1 }}
        transition={{ duration: 0.4, ease: "easeOut" }}
      >
        <CircularProgress progress={progress} size={160} strokeWidth={6} />
      </motion.div>

      {/* Converting label */}
      <motion.div
        initial={{ opacity: 0, y: 8 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ delay: 0.2, duration: 0.3 }}
        className="mt-6 text-center"
      >
        <p className="text-lg font-medium text-text-primary">
          {format === "mp4" ? "Exporting MP4..." : "Converting to GIF..."}
        </p>
        <p className="text-sm text-text-secondary mt-1 tabular-nums">
          {percentage}%
        </p>
      </motion.div>

      {/* ETA */}
      {showEta && eta && (
        <motion.p
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          className="mt-3 text-xs text-text-tertiary"
        >
          {eta}
        </motion.p>
      )}

      {/* Cancel button */}
      <motion.button
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ delay: 0.4 }}
        onClick={onCancel}
        className="mt-8 px-5 py-2 text-sm text-text-secondary
          rounded-lg hover:bg-surface-hover active:bg-surface-raised
          transition-colors border border-border"
      >
        Cancel
      </motion.button>

      <motion.p
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        transition={{ delay: 0.6 }}
        className="mt-3 text-[10px] text-text-tertiary"
      >
        Press Escape to cancel
      </motion.p>
    </div>
  );
}
