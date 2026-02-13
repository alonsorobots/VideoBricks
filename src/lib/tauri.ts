import { invoke } from "@tauri-apps/api/core";
import type {
  ValidateResult,
  VideoMetadata,
  ConversionSettings,
  AppSettings,
  GifResult,
  Mp4Result,
} from "./types";

/** Validate a video file for GIF conversion */
export async function validateVideo(path: string): Promise<ValidateResult> {
  return invoke("validate_video", { path });
}

/** Get video metadata via ffprobe */
export async function getVideoMetadata(path: string): Promise<VideoMetadata> {
  return invoke("get_video_metadata", { path });
}

/** Get video thumbnails as base64 data URLs */
export async function getVideoThumbnails(
  path: string,
  count: number,
  thumbHeight: number
): Promise<string[]> {
  return invoke("get_video_thumbnails", { path, count, thumbHeight });
}

/** Get video thumbnails for a specific time range (detail trim mode) */
export async function getVideoThumbnailsRange(
  path: string,
  count: number,
  thumbHeight: number,
  startTime: number,
  endTime: number
): Promise<string[]> {
  return invoke("get_video_thumbnails_range", {
    path,
    count,
    thumbHeight,
    startTime,
    endTime,
  });
}

/** Convert video to GIF. Returns array of GIF results. Progress via events. */
export async function convertToGif(
  settings: ConversionSettings
): Promise<GifResult[]> {
  return invoke("convert_to_gif", { settings });
}

/** Cancel an ongoing conversion */
export async function cancelConversion(): Promise<void> {
  return invoke("cancel_conversion");
}

/** Estimate file size for current settings */
export async function estimateFileSize(
  settings: ConversionSettings
): Promise<number> {
  return invoke("estimate_file_size", { settings });
}

/** Generate a preview GIF (lower quality) */
export async function generatePreviewGif(
  settings: ConversionSettings
): Promise<string> {
  return invoke("generate_preview_gif", { settings });
}

/** Get persisted settings */
export async function getSettings(): Promise<AppSettings> {
  return invoke("get_settings");
}

/** Save settings to disk */
export async function setSettings(settings: AppSettings): Promise<void> {
  return invoke("set_settings", { settings });
}

/** Save a single GIF to a file path. Returns file size. */
export async function saveGifFile(outputPath: string): Promise<number> {
  return invoke("save_gif_file", { outputPath });
}

/** Save all GIFs to a directory. Returns list of saved paths. */
export async function saveAllGifFiles(
  directory: string,
  baseName: string
): Promise<string[]> {
  return invoke("save_all_gif_files", { directory, baseName });
}

/** Copy GIF to clipboard. Returns temp file path. */
export async function copyGifToClipboard(): Promise<string> {
  return invoke("copy_gif_to_clipboard");
}

/** Reveal a file in the system file explorer */
export async function revealInExplorer(path: string): Promise<void> {
  return invoke("reveal_in_explorer", { path });
}

/** Convert video to MP4. Saves directly to disk. Returns array of Mp4Result. */
export async function convertToMp4(
  outputPath: string,
  settings: ConversionSettings
): Promise<Mp4Result[]> {
  return invoke("convert_to_mp4", { outputPath, settings });
}

/** Export modified video (not GIF) - legacy */
export async function exportVideo(
  inputPath: string,
  outputPath: string,
  settings: ConversionSettings
): Promise<void> {
  return invoke("export_video", { inputPath, outputPath, settings });
}

/** Get a temp directory for export previews */
export async function getTempDir(): Promise<string> {
  return invoke("get_temp_dir");
}

/** Copy a single file from source to destination. Returns bytes copied. */
export async function copyFile(
  source: string,
  destination: string
): Promise<number> {
  return invoke("copy_file", { source, destination });
}

/** Copy multiple files to a directory with numbered naming. Returns list of destination paths. */
export async function copyFilesToDirectory(
  sources: string[],
  directory: string,
  baseName: string
): Promise<string[]> {
  return invoke("copy_files_to_directory", { sources, directory, baseName });
}

/** Write all in-memory GIFs to temp files. Returns list of temp file paths (for drag-out). */
export async function saveGifsToTemp(baseName: string): Promise<string[]> {
  return invoke("save_gifs_to_temp", { baseName });
}

/** Run TransNetV2 shot boundary detection on a video. Returns array of scene boundaries. */
export async function findShots(
  path: string,
  threshold: number = 0.35
): Promise<{ start: number; end: number }[]> {
  return invoke("find_shots", { path, threshold });
}
