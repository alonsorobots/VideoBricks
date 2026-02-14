import { forwardRef, useEffect, useRef, useImperativeHandle, useState, useCallback } from "react";
import type { TrimSegment } from "../lib/types";

interface VideoPlayerProps {
  src: string;
  duration: number;
  segments: TrimSegment[];
  loop: boolean;
  bounce: boolean;
  isPlaying: boolean;
  onPlayingChange: (playing: boolean) => void;
  onTimeUpdate: (time: number) => void;
}

const VideoPlayer = forwardRef<HTMLVideoElement, VideoPlayerProps>(
  (
    {
      src,
      duration,
      segments,
      loop,
      bounce,
      isPlaying,
      onPlayingChange,
      onTimeUpdate,
    },
    ref
  ) => {
    const videoRef = useRef<HTMLVideoElement>(null);
    const isReversingRef = useRef(false);
    const currentSegmentRef = useRef(0);
    const [error, setError] = useState<string | null>(null);

    useImperativeHandle(ref, () => videoRef.current!, []);

    // Handle time update + multi-segment loop/bounce
    // Only enforce segment boundaries when actively playing (isPlaying is true).
    // When scrubbing (isPlaying is false), just report the time without jumping.
    useEffect(() => {
      const video = videoRef.current;
      if (!video) return;
      if (segments.length === 0) return;

      const handleTimeUpdate = () => {
        const time = video.currentTime;
        onTimeUpdate(time);

        // Don't enforce segment boundaries when paused/scrubbing
        if (!isPlaying) return;

        // Find the current segment we should be in
        const segIdx = currentSegmentRef.current;
        const seg = segments[segIdx];
        if (!seg) return;

        if (time >= seg.end) {
          // Move to next segment
          const nextIdx = segIdx + 1;
          if (nextIdx < segments.length) {
            currentSegmentRef.current = nextIdx;
            video.currentTime = segments[nextIdx].start;
          } else {
            // Past last segment
            if (bounce && !isReversingRef.current) {
              isReversingRef.current = true;
              currentSegmentRef.current = 0;
              video.currentTime = segments[0].start;
            } else if (loop) {
              currentSegmentRef.current = 0;
              video.currentTime = segments[0].start;
              isReversingRef.current = false;
            } else {
              video.pause();
              onPlayingChange(false);
            }
          }
        }
      };

      video.addEventListener("timeupdate", handleTimeUpdate);
      return () => video.removeEventListener("timeupdate", handleTimeUpdate);
    }, [segments, loop, bounce, isPlaying, onTimeUpdate, onPlayingChange]);

    // Sync playing state
    useEffect(() => {
      const video = videoRef.current;
      if (!video) return;
      if (segments.length === 0) return;

      if (isPlaying) {
        // Find which segment the current time is in, or start from first
        const time = video.currentTime;
        const inSegIdx = segments.findIndex(
          (s) => time >= s.start && time < s.end
        );
        if (inSegIdx >= 0) {
          currentSegmentRef.current = inSegIdx;
        } else {
          // Not in any segment -- jump to first segment
          currentSegmentRef.current = 0;
          video.currentTime = segments[0].start;
        }
        video.play().catch(() => onPlayingChange(false));
      } else {
        video.pause();
      }
    }, [isPlaying, segments, onPlayingChange]);

    const handleVideoClick = useCallback(() => {
      const video = videoRef.current;
      if (!video) return;
      if (video.paused) {
        video.play();
        onPlayingChange(true);
      } else {
        video.pause();
        onPlayingChange(false);
      }
    }, [onPlayingChange]);

    return (
      <div className="w-full h-full flex items-center justify-center relative bg-bg-neutral">
        <video
          ref={videoRef}
          src={src}
          className="w-full h-full object-contain"
          preload="auto"
          onError={(e) => {
            const vid = e.currentTarget;
            setError(`Video load error: ${vid.error?.message || "unknown"}`);
          }}
          onLoadedData={() => setError(null)}
          onEnded={() => onPlayingChange(false)}
          onClick={handleVideoClick}
        />

        {error && (
          <div className="absolute bottom-2 left-2 right-2 text-[10px] text-red-400 bg-black/80 px-2 py-1 rounded z-20">
            {error}
          </div>
        )}
      </div>
    );
  }
);

VideoPlayer.displayName = "VideoPlayer";
export default VideoPlayer;
