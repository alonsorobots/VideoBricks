/** Video metadata from ffprobe */
export interface VideoMetadata {
  duration: number;
  width: number;
  height: number;
  frameRate: number;
  codec: string;
  hasAudio: boolean;
  rotation: number;
  fileSize: number;
}

/** Validation result from the backend */
export interface ValidateResult {
  isValid: boolean;
  error: string | null;
  metadata: VideoMetadata | null;
}

/** Normalized crop rectangle (0-1) */
export interface CropRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** A single trim segment with in/out points */
export interface TrimSegment {
  id: string;
  start: number;
  end: number;
  /** Whether this segment is active (included in render). Defaults to true. */
  active?: boolean;
}

let _segIdCounter = 0;
/** Generate a unique segment ID */
export function newSegmentId(): string {
  return `seg-${++_segIdCounter}-${Date.now().toString(36)}`;
}

/** Conversion settings sent to backend */
export interface ConversionSettings {
  sourcePath: string;
  /** User-editable output base name (no extension) */
  outputName: string;
  quality: number;
  width: number | null;
  height: number | null;
  fps: number;
  speed: number;
  loopForever: boolean;
  loopCount: number;
  bounce: boolean;
  segments: TrimSegment[];
  segmentMode: "merge" | "split";
  crop: CropRect | null;
}

/** App settings persisted to disk */
export interface AppSettings {
  outputQuality: number;
  outputSpeed: number;
  outputFps: number;
  loopGif: boolean;
  bounceGif: boolean;
  loopCount: number;
}

/** Progress event from conversion */
export interface ConversionProgressEvent {
  completedFrames: number;
  totalFrames: number;
  progress: number;
}

/** Per-edit state that should persist across screen transitions */
export interface EditState {
  segments: TrimSegment[];
  cropRect: CropRect;
  outputWidth: number;
  outputHeight: number;
}

/** Rainbow color palette for segments (cycles) */
export const SEGMENT_COLORS = [
  "#FFB74D", // amber/orange
  "#4DD0E1", // cyan
  "#AED581", // light green
  "#BA68C8", // purple
  "#FF8A65", // deep orange
  "#4FC3F7", // light blue
  "#DCE775", // lime
  "#F06292", // pink
];

/** A single GIF result from conversion */
export interface GifResult {
  dataUrl: string;
  fileSize: number;
}

/** A single MP4 result from conversion */
export interface Mp4Result {
  filePath: string;
  fileSize: number;
}

/** The format of the completed export */
export type ExportFormat = "gif" | "mp4";

/** Application screens / routes */
export type AppScreen =
  | { type: "start" }
  | { type: "edit"; videoPath: string; metadata: VideoMetadata }
  | { type: "converting"; settings: ConversionSettings; format: ExportFormat }
  | { type: "completed"; gifs: GifResult[]; mp4s: Mp4Result[]; format: ExportFormat; videoPath: string; metadata: VideoMetadata; outputName?: string };

/** Aspect ratio preset */
export interface AspectRatioPreset {
  label: string;
  ratio: number | null; // null = free
}

/** Dimension mode */
export type DimensionMode = "pixels" | "percent";


export const DEFAULT_SETTINGS: AppSettings = {
  outputQuality: 1.0,
  outputSpeed: 1.0,
  outputFps: 10,
  loopGif: true,
  bounceGif: false,
  loopCount: 0,
};

export const ASPECT_RATIO_PRESETS: AspectRatioPreset[] = [
  { label: "Free", ratio: null },
  { label: "16:9", ratio: 16 / 9 },
  { label: "4:3", ratio: 4 / 3 },
  { label: "1:1", ratio: 1 },
  { label: "3:4", ratio: 3 / 4 },
  { label: "9:16", ratio: 9 / 16 },
];

export const FPS_MIN = 3;
export const FPS_MAX = 50;
export const SPEED_MIN = 0.5;
export const SPEED_MAX = 5.0;
export const SPEED_STEP = 0.25;
