use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;
use std::sync::Mutex as StdMutex;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// Windows flag to prevent child processes from creating visible console windows.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Create a Command that won't spawn a visible console window on Windows.
pub fn hidden_command<S: AsRef<std::ffi::OsStr>>(program: S) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

// ---------------------------------------------------------------------------
// Global child-process tracker -- lets us kill orphaned FFmpeg on app exit
// ---------------------------------------------------------------------------
static ACTIVE_PIDS: StdMutex<Vec<u32>> = StdMutex::new(Vec::new());

pub fn register_pid(pid: u32) {
    if let Ok(mut pids) = ACTIVE_PIDS.lock() {
        pids.push(pid);
    }
}

pub fn unregister_pid(pid: u32) {
    if let Ok(mut pids) = ACTIVE_PIDS.lock() {
        pids.retain(|&p| p != pid);
    }
}

/// Kill every tracked child process. Called on window close / app exit.
pub fn kill_all_children() {
    if let Ok(mut pids) = ACTIVE_PIDS.lock() {
        for pid in pids.drain(..) {
            // /F = force, /T = kill child tree
            let _ = hidden_command("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }
}

/// Locate the conda executable.
pub fn find_conda() -> Result<String, String> {
    // Try common locations
    let mut candidates: Vec<String> = vec![
        "conda".into(),
        "conda.exe".into(),
        r"C:\ProgramData\miniforge3\Scripts\conda.exe".into(),
        r"C:\ProgramData\Anaconda3\Scripts\conda.exe".into(),
    ];
    // Add user-local conda locations dynamically
    if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
        for subdir in ["miniforge3", "miniconda3", "anaconda3", "mambaforge"] {
            candidates.push(format!(r"{}\{}\Scripts\conda.exe", home, subdir));
        }
    }
    for candidate in candidates.iter() {
        let status = hidden_command(candidate)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if status.is_ok() && status.unwrap().success() {
            return Ok(candidate.to_string());
        }
    }
    Err("conda not found. Please install Conda/Miniforge and ensure it is in your PATH.".to_string())
}

/// Spawn a command, track its PID, wait for output, then untrack.
/// This is the safe replacement for `cmd.output()` -- if the app exits
/// mid-process, `kill_all_children()` will clean it up.
pub fn tracked_output(mut cmd: Command) -> Result<std::process::Output, String> {
    let child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn ffmpeg: {}", e))?;

    let pid = child.id();
    register_pid(pid);

    let output = child
        .wait_with_output()
        .map_err(|e| format!("FFmpeg process failed: {}", e))?;

    unregister_pid(pid);
    Ok(output)
}

/// Video metadata extracted via ffprobe
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoMetadata {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
    pub frame_rate: f64,
    pub codec: String,
    pub has_audio: bool,
    pub rotation: i32,
    pub file_size: u64,
}

/// Find the ffprobe executable.
/// Checks: bundled alongside exe, PATH, common install locations
fn find_ffprobe() -> Option<String> {
    // Check if bundled next to our executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let bundled = dir.join("ffprobe.exe");
            if bundled.exists() {
                return Some(bundled.to_string_lossy().to_string());
            }
        }
    }

    // Check PATH
    if hidden_command("ffprobe")
        .arg("-version")
        .output()
        .is_ok()
    {
        return Some("ffprobe".to_string());
    }

    // Common Windows install locations
    let common_paths = [
        r"C:\ffmpeg\bin\ffprobe.exe",
        r"C:\Program Files\ffmpeg\bin\ffprobe.exe",
        r"C:\tools\ffmpeg\bin\ffprobe.exe",
    ];
    for path in &common_paths {
        if Path::new(path).exists() {
            return Some(path.to_string());
        }
    }

    None
}

/// Find the ffmpeg executable
fn find_ffmpeg() -> Option<String> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let bundled = dir.join("ffmpeg.exe");
            if bundled.exists() {
                return Some(bundled.to_string_lossy().to_string());
            }
        }
    }

    if hidden_command("ffmpeg")
        .arg("-version")
        .output()
        .is_ok()
    {
        return Some("ffmpeg".to_string());
    }

    let common_paths = [
        r"C:\ffmpeg\bin\ffmpeg.exe",
        r"C:\Program Files\ffmpeg\bin\ffmpeg.exe",
        r"C:\tools\ffmpeg\bin\ffmpeg.exe",
    ];
    for path in &common_paths {
        if Path::new(path).exists() {
            return Some(path.to_string());
        }
    }

    None
}

pub fn get_ffprobe() -> Result<String, String> {
    find_ffprobe().ok_or_else(|| {
        "FFprobe not found. Please install FFmpeg and ensure it is in your PATH.".to_string()
    })
}

pub fn get_ffmpeg() -> Result<String, String> {
    find_ffmpeg().ok_or_else(|| {
        "FFmpeg not found. Please install FFmpeg and ensure it is in your PATH.".to_string()
    })
}

/// Get video metadata using ffprobe
pub fn get_metadata(path: &str) -> Result<VideoMetadata, String> {
    let ffprobe = get_ffprobe()?;

    let output = hidden_command(&ffprobe)
        .args([
            "-v", "quiet",
            "-print_format", "json",
            "-show_format",
            "-show_streams",
            path,
        ])
        .output()
        .map_err(|e| format!("Failed to run ffprobe: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "ffprobe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("Failed to parse ffprobe output: {}", e))?;

    // Find the video stream
    let streams = json["streams"]
        .as_array()
        .ok_or("No streams found in video")?;

    let video_stream = streams
        .iter()
        .find(|s| s["codec_type"].as_str() == Some("video"))
        .ok_or("No video stream found")?;

    let has_audio = streams
        .iter()
        .any(|s| s["codec_type"].as_str() == Some("audio"));

    let width = video_stream["width"]
        .as_u64()
        .ok_or("Could not read video width")? as u32;

    let height = video_stream["height"]
        .as_u64()
        .ok_or("Could not read video height")? as u32;

    let codec = video_stream["codec_name"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();

    // Parse frame rate from "r_frame_rate" (e.g. "30/1" or "30000/1001")
    let frame_rate = parse_frame_rate(
        video_stream["r_frame_rate"]
            .as_str()
            .unwrap_or("30/1"),
    );

    // Parse duration: try stream duration, then format duration
    let duration = video_stream["duration"]
        .as_str()
        .and_then(|d| d.parse::<f64>().ok())
        .or_else(|| {
            json["format"]["duration"]
                .as_str()
                .and_then(|d| d.parse::<f64>().ok())
        })
        .unwrap_or(0.0);

    // Parse rotation from side_data or tags
    let rotation = video_stream["side_data_list"]
        .as_array()
        .and_then(|sd| {
            sd.iter().find_map(|d| {
                if d["side_data_type"].as_str() == Some("Display Matrix") {
                    d["rotation"].as_i64().map(|r| r as i32)
                } else {
                    None
                }
            })
        })
        .or_else(|| {
            video_stream["tags"]["rotate"]
                .as_str()
                .and_then(|r| r.parse::<i32>().ok())
        })
        .unwrap_or(0);

    let file_size = json["format"]["size"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    Ok(VideoMetadata {
        duration,
        width,
        height,
        frame_rate,
        codec,
        has_audio,
        rotation,
        file_size,
    })
}

fn parse_frame_rate(s: &str) -> f64 {
    if let Some((num, den)) = s.split_once('/') {
        let n: f64 = num.parse().unwrap_or(30.0);
        let d: f64 = den.parse().unwrap_or(1.0);
        if d > 0.0 {
            n / d
        } else {
            30.0
        }
    } else {
        s.parse().unwrap_or(30.0)
    }
}

/// Extract frames from a video as RGBA pixel data.
/// Returns Vec of (presentation_timestamp, width, height, rgba_data).
pub fn extract_frames(
    path: &str,
    fps: f64,
    start_time: Option<f64>,
    end_time: Option<f64>,
    speed: f64,
    crop: Option<CropRect>,
) -> Result<Vec<FrameData>, String> {
    let ffmpeg = get_ffmpeg()?;
    let metadata = get_metadata(path)?;

    let actual_start = start_time.unwrap_or(0.0);
    let actual_end = end_time.unwrap_or(metadata.duration);
    let duration = (actual_end - actual_start) / speed;

    // Calculate expected frame count
    let frame_count = (duration * fps).ceil() as usize;
    if frame_count < 2 {
        return Err("Video is too short - need at least 2 frames".to_string());
    }

    // Use -ss (input seeking) BEFORE -i for fast demuxer-level seeking.
    // This avoids decoding the entire video up to the start point.
    // We seek slightly before the segment start to ensure we don't miss the
    // first keyframe, then use trim filter for frame-accurate boundaries.
    let seek_margin = 1.0_f64; // seconds before segment start to seek to
    let seek_to = (actual_start - seek_margin).max(0.0);
    let seg_duration = actual_end - actual_start;

    // Build ffmpeg filter chain
    let mut filters = Vec::new();

    // Fine-grained trim after the coarse -ss seek for frame-accurate boundaries.
    // After -ss, timestamps are rebased so actual_start becomes (actual_start - seek_to).
    let trim_start = actual_start - seek_to;
    let trim_end = trim_start + seg_duration;
    filters.push(format!(
        "trim=start={}:end={},setpts=PTS-STARTPTS",
        trim_start, trim_end
    ));

    // Speed adjustment
    if (speed - 1.0).abs() > 0.01 {
        filters.push(format!("setpts=PTS/{}", speed));
    }

    // FPS
    filters.push(format!("fps={}", fps));

    // Crop (in pixel coordinates) - ensure even dimensions to avoid stride issues
    let mut crop_w: Option<u32> = None;
    let mut crop_h: Option<u32> = None;
    if let Some(c) = crop {
        // Compute crop dimensions and ensure they are even (many codecs/filters require this)
        let cw = ((c.width * metadata.width as f64).round() as u32).max(2) & !1;
        let ch = ((c.height * metadata.height as f64).round() as u32).max(2) & !1;
        let cx = (c.x * metadata.width as f64).round() as u32;
        let cy = (c.y * metadata.height as f64).round() as u32;
        if cw > 0 && ch > 0 {
            filters.push(format!("crop={}:{}:{}:{}", cw, ch, cx, cy));
            crop_w = Some(cw);
            crop_h = Some(ch);
        }
    }

    // Force exact output dimensions with scale filter to prevent any stride mismatch.
    // FFmpeg's actual decoded dimensions may differ from metadata (coded vs display, SAR, etc.)
    // so we always use an explicit scale at the end to guarantee the dimensions we expect.
    let out_w = (crop_w.unwrap_or(metadata.width)).max(2) & !1; // ensure even and >= 2
    let out_h = (crop_h.unwrap_or(metadata.height)).max(2) & !1;
    filters.push(format!("scale={}:{}:flags=lanczos", out_w, out_h));
    // Explicit pixel format conversion at the end of the filter chain
    filters.push("format=rgba".to_string());

    // Output as raw RGBA
    let filter_chain = filters.join(",");

    let mut cmd = hidden_command(&ffmpeg);
    // -ss before -i = fast demuxer seek (no decoding of skipped frames)
    // -t limits how much we read after the seek point
    let read_duration = seg_duration + seek_margin + 1.0; // enough to cover trim window
    cmd.args([
        "-ss", &format!("{:.3}", seek_to),
        "-t", &format!("{:.3}", read_duration),
        "-i", path,
        "-vf", &filter_chain,
        "-pix_fmt", "rgba",
        "-f", "rawvideo",
        "-v", "error",
        "pipe:1",
    ]);
    let output = tracked_output(cmd)?;

    if !output.status.success() {
        return Err(format!(
            "FFmpeg frame extraction failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let frame_size = (out_w as usize) * (out_h as usize) * 4; // RGBA = 4 bytes per pixel
    let total_bytes = output.stdout.len();
    let actual_frame_count = total_bytes / frame_size;

    if actual_frame_count == 0 {
        return Err("No frames were extracted from the video".to_string());
    }

    let mut frames = Vec::with_capacity(actual_frame_count);
    let frame_duration = 1.0 / fps;

    for i in 0..actual_frame_count {
        let offset = i * frame_size;
        let rgba_data = output.stdout[offset..offset + frame_size].to_vec();
        frames.push(FrameData {
            index: i,
            presentation_timestamp: i as f64 * frame_duration,
            width: out_w,
            height: out_h,
            rgba: rgba_data,
        });
    }

    Ok(frames)
}

/// Generate thumbnail images at evenly spaced intervals for the timeline.
/// If `start_time` and `end_time` are provided, thumbnails span that range;
/// otherwise they span the full video duration.
///
/// Uses a single FFmpeg process with fps filter + image2pipe to output all
/// thumbnails in one shot. Falls back to sequential per-frame extraction
/// if the single-pass approach fails.
pub fn generate_thumbnails(
    path: &str,
    count: usize,
    thumb_height: u32,
    start_time: Option<f64>,
    end_time: Option<f64>,
) -> Result<Vec<Vec<u8>>, String> {
    let ffmpeg = get_ffmpeg()?;
    let metadata = get_metadata(path)?;

    // Cap count to avoid excessive processing
    let count = count.min(30);

    let range_start = start_time.unwrap_or(0.0);
    let range_end = end_time.unwrap_or(metadata.duration);
    let range = range_end - range_start;
    if range <= 0.0 || count == 0 {
        return Ok(Vec::new());
    }

    // Calculate thumbnail width maintaining aspect ratio
    let aspect = metadata.width as f64 / metadata.height as f64;
    let thumb_width = ((thumb_height as f64 * aspect).round() as u32).max(2) & !1;
    let thumb_height_even = thumb_height.max(2) & !1;

    // Try single-pass first (much faster: 1 process instead of N)
    match generate_thumbnails_single_pass(
        &ffmpeg, path, count, thumb_width, thumb_height_even, range_start, range,
    ) {
        Ok(thumbs) if !thumbs.is_empty() => return Ok(thumbs),
        _ => {} // fall through to sequential fallback
    }

    // Fallback: sequential per-frame extraction (cap at 10 to limit wait)
    let fallback_count = count.min(10);
    let interval = range / fallback_count as f64;
    let mut thumbnails = Vec::with_capacity(fallback_count);

    for i in 0..fallback_count {
        let timestamp = range_start + i as f64 * interval;

        let mut cmd = hidden_command(&ffmpeg);
        cmd.args([
            "-ss", &timestamp.to_string(),
            "-i", path,
            "-vframes", "1",
            "-vf", &format!("scale={}:{}", thumb_width, thumb_height_even),
            "-f", "image2",
            "-c:v", "bmp",
            "-v", "error",
            "pipe:1",
        ]);

        if let Ok(output) = tracked_output(cmd) {
            if output.status.success() && !output.stdout.is_empty() {
                thumbnails.push(output.stdout);
            }
        }
    }

    Ok(thumbnails)
}

/// Single-pass thumbnail generation: one FFmpeg process outputs all frames
/// as concatenated BMP images via image2pipe.
fn generate_thumbnails_single_pass(
    ffmpeg_path: &str,
    path: &str,
    count: usize,
    thumb_width: u32,
    thumb_height: u32,
    range_start: f64,
    range: f64,
) -> Result<Vec<Vec<u8>>, String> {
    // fps filter: output exactly `count` frames spread over `range` seconds
    let fps_val = count as f64 / range;
    let filter = format!("fps={},scale={}:{}", fps_val, thumb_width, thumb_height);

    let mut cmd = hidden_command(ffmpeg_path);
    cmd.args([
        "-ss", &range_start.to_string(),
        "-t", &range.to_string(),
        "-i", path,
        "-vf", &filter,
        "-f", "image2pipe",
        "-c:v", "bmp",
        "-v", "error",
        "pipe:1",
    ]);

    let output = tracked_output(cmd)?;

    if !output.status.success() {
        return Err(format!(
            "FFmpeg single-pass thumbnails failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    split_bmp_stream(&output.stdout)
}

/// Split a byte stream of concatenated BMP images into individual BMP buffers.
/// BMP format: bytes 0..2 = "BM", bytes 2..6 = file size (little-endian u32).
fn split_bmp_stream(data: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    let mut images = Vec::new();
    let mut offset = 0;

    while offset + 6 <= data.len() {
        // Verify BMP magic bytes
        if data[offset] != 0x42 || data[offset + 1] != 0x4D {
            break; // not a BMP header, stop
        }

        // Read file size from bytes 2..6 (little-endian u32)
        let file_size = u32::from_le_bytes([
            data[offset + 2],
            data[offset + 3],
            data[offset + 4],
            data[offset + 5],
        ]) as usize;

        if file_size < 14 || offset + file_size > data.len() {
            break; // truncated or invalid
        }

        images.push(data[offset..offset + file_size].to_vec());
        offset += file_size;
    }

    Ok(images)
}

/// Export video with modifications (crop, trim, scale, speed)
pub fn export_modified_video(
    input_path: &str,
    output_path: &str,
    start_time: Option<f64>,
    end_time: Option<f64>,
    speed: f64,
    crop: Option<CropRect>,
    output_width: Option<u32>,
    output_height: Option<u32>,
) -> Result<(), String> {
    let ffmpeg = get_ffmpeg()?;
    let metadata = get_metadata(input_path)?;

    let mut filters = Vec::new();

    // Trim
    let actual_start = start_time.unwrap_or(0.0);
    let actual_end = end_time.unwrap_or(metadata.duration);
    filters.push(format!(
        "trim=start={}:end={},setpts=PTS-STARTPTS",
        actual_start, actual_end
    ));

    // Speed
    if (speed - 1.0).abs() > 0.01 {
        filters.push(format!("setpts=PTS/{}", speed));
    }

    // Crop
    if let Some(c) = crop {
        let cw = (c.width * metadata.width as f64).round() as u32;
        let ch = (c.height * metadata.height as f64).round() as u32;
        let cx = (c.x * metadata.width as f64).round() as u32;
        let cy = (c.y * metadata.height as f64).round() as u32;
        if cw > 0 && ch > 0 {
            filters.push(format!("crop={}:{}:{}:{}", cw, ch, cx, cy));
        }
    }

    // Scale
    if let (Some(w), Some(h)) = (output_width, output_height) {
        filters.push(format!("scale={}:{}", w, h));
    }

    let filter_chain = filters.join(",");

    let mut cmd = hidden_command(&ffmpeg);
    cmd.args([
        "-i", input_path,
        "-vf", &filter_chain,
        "-an", // no audio
        "-c:v", "libx264",
        "-preset", "fast",
        "-crf", "18",
        "-y",
        output_path,
    ]);
    let output = tracked_output(cmd)?;

    if !output.status.success() {
        return Err(format!(
            "FFmpeg export failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(())
}

/// Build the video filter chain for a single segment export.
/// Shared helper for both single and multi-segment MP4 export.
/// `trim_start` and `trim_end` are relative to the -ss seek point (not absolute).
fn build_segment_filter_chain(
    metadata: &VideoMetadata,
    trim_start: f64,
    trim_end: f64,
    speed: f64,
    crop: Option<CropRect>,
    output_width: Option<u32>,
    output_height: Option<u32>,
) -> String {
    let mut filters = Vec::new();

    // Fine-grained trim (relative to the -ss seek point) for frame-accurate boundaries
    filters.push(format!(
        "trim=start={}:end={},setpts=PTS-STARTPTS",
        trim_start, trim_end
    ));

    // Speed
    if (speed - 1.0).abs() > 0.01 {
        filters.push(format!("setpts=PTS/{}", speed));
    }

    // Crop
    if let Some(c) = crop {
        let cw = (c.width * metadata.width as f64).round() as u32;
        let ch = (c.height * metadata.height as f64).round() as u32;
        let cx = (c.x * metadata.width as f64).round() as u32;
        let cy = (c.y * metadata.height as f64).round() as u32;
        if cw > 0 && ch > 0 {
            filters.push(format!("crop={}:{}:{}:{}", cw, ch, cx, cy));
        }
    }

    // Scale
    if let (Some(w), Some(h)) = (output_width, output_height) {
        // Ensure even dimensions for h264
        let w = (w + 1) & !1;
        let h = (h + 1) & !1;
        filters.push(format!("scale={}:{}", w, h));
    } else {
        // Ensure even dimensions even without explicit scale
        filters.push("scale=trunc(iw/2)*2:trunc(ih/2)*2".to_string());
    }

    filters.join(",")
}

/// Export a single segment to an MP4 file.
/// Uses -ss (input seeking) before -i for fast demuxer-level seeking.
fn export_single_segment(
    ffmpeg_path: &str,
    input_path: &str,
    output_path: &str,
    metadata: &VideoMetadata,
    start: f64,
    end: f64,
    speed: f64,
    crop: Option<CropRect>,
    output_width: Option<u32>,
    output_height: Option<u32>,
) -> Result<(), String> {
    let seek_margin = 1.0_f64;
    let seek_to = (start - seek_margin).max(0.0);
    let seg_duration = end - start;

    // Trim offsets relative to the -ss seek point
    let trim_start = start - seek_to;
    let trim_end = trim_start + seg_duration;

    let filter_chain = build_segment_filter_chain(
        metadata, trim_start, trim_end, speed, crop, output_width, output_height,
    );

    // Read enough from the seek point to cover the segment + margin
    let read_duration = seg_duration + seek_margin + 1.0;

    let mut cmd = hidden_command(ffmpeg_path);
    // -ss before -i = fast demuxer seek (avoids decoding skipped frames)
    cmd.args([
        "-ss", &format!("{:.3}", seek_to),
        "-t", &format!("{:.3}", read_duration),
        "-i", input_path,
        "-vf", &filter_chain,
        "-an",
        "-c:v", "libx264",
        "-preset", "fast",
        "-crf", "18",
        "-y",
        output_path,
    ]);
    let output = tracked_output(cmd)?;

    if !output.status.success() {
        return Err(format!(
            "FFmpeg segment export failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

/// Result of an MP4 export
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mp4Result {
    pub file_path: String,
    pub file_size: u64,
}

/// Export video segments to MP4.
/// In merge mode: exports each segment to a temp file, then concatenates into one output.
/// In split mode: exports each segment to a separate numbered file.
/// Returns a list of Mp4Result with file paths and sizes.
pub fn export_mp4_segments(
    input_path: &str,
    output_path: &str,
    segments: &[(f64, f64)],
    segment_mode: &str,
    speed: f64,
    crop: Option<CropRect>,
    output_width: Option<u32>,
    output_height: Option<u32>,
    on_progress: &dyn Fn(usize, usize), // (completed_segments, total_segments)
) -> Result<Vec<Mp4Result>, String> {
    let ffmpeg = get_ffmpeg()?;
    let metadata = get_metadata(input_path)?;

    if segments.is_empty() {
        return Err("No segments to export".to_string());
    }

    let total = segments.len();

    if segment_mode == "split" || total == 1 {
        // Split mode or single segment: export each directly to final path
        let mut results = Vec::new();
        let base = output_path.trim_end_matches(".mp4");

        for (i, &(start, end)) in segments.iter().enumerate() {
            let path = if total == 1 {
                output_path.to_string()
            } else {
                format!("{}_{:02}.mp4", base, i + 1)
            };

            export_single_segment(
                &ffmpeg, input_path, &path, &metadata,
                start, end, speed, crop, output_width, output_height,
            )?;

            let file_size = std::fs::metadata(&path)
                .map(|m| m.len())
                .unwrap_or(0);

            results.push(Mp4Result {
                file_path: path,
                file_size,
            });

            on_progress(i + 1, total);
        }

        Ok(results)
    } else {
        // Merge mode with multiple segments: export each to temp, then concat
        let temp_dir = std::env::temp_dir().join(format!("videobricks-mp4-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_dir)
            .map_err(|e| format!("Failed to create temp dir: {}", e))?;

        let mut temp_files = Vec::new();

        for (i, &(start, end)) in segments.iter().enumerate() {
            let temp_path = temp_dir.join(format!("seg_{:03}.mp4", i));
            let temp_path_str = temp_path.to_string_lossy().to_string();

            export_single_segment(
                &ffmpeg, input_path, &temp_path_str, &metadata,
                start, end, speed, crop, output_width, output_height,
            )?;

            temp_files.push(temp_path_str);
            on_progress(i + 1, total + 1); // +1 for the concat step
        }

        // Build concat list file
        let list_path = temp_dir.join("concat_list.txt");
        let list_content: String = temp_files
            .iter()
            .map(|p| format!("file '{}'", p.replace('\\', "/")))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&list_path, &list_content)
            .map_err(|e| format!("Failed to write concat list: {}", e))?;

        // Concat with ffmpeg
        let mut concat_cmd = hidden_command(&ffmpeg);
        concat_cmd.args([
            "-f", "concat",
            "-safe", "0",
            "-i", &list_path.to_string_lossy(),
            "-c", "copy",
            "-y",
            output_path,
        ]);
        let concat_output = tracked_output(concat_cmd)?;

        if !concat_output.status.success() {
            // Clean up temp files
            let _ = std::fs::remove_dir_all(&temp_dir);
            return Err(format!(
                "FFmpeg concat failed: {}",
                String::from_utf8_lossy(&concat_output.stderr)
            ));
        }

        // Clean up temp files
        let _ = std::fs::remove_dir_all(&temp_dir);

        on_progress(total + 1, total + 1);

        let file_size = std::fs::metadata(output_path)
            .map(|m| m.len())
            .unwrap_or(0);

        Ok(vec![Mp4Result {
            file_path: output_path.to_string(),
            file_size,
        }])
    }
}

/// Normalized crop rectangle (0.0-1.0 for each dimension)
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CropRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Raw frame data extracted from video
pub struct FrameData {
    #[allow(dead_code)]
    pub index: usize,
    pub presentation_timestamp: f64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
