use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex as StdMutex;
use std::sync::OnceLock;

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
            #[cfg(windows)]
            {
                // /F = force, /T = kill child tree
                let _ = hidden_command("taskkill")
                    .args(["/F", "/T", "/PID", &pid.to_string()])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }
            #[cfg(not(windows))]
            {
                // Send SIGTERM to the process group to kill child tree
                unsafe {
                    libc::kill(pid as i32, libc::SIGTERM);
                }
            }
        }
    }
}

/// Locate the conda executable.
pub fn find_conda() -> Result<String, String> {
    let mut candidates: Vec<String> = vec!["conda".into()];

    #[cfg(windows)]
    {
        candidates.push("conda.exe".into());
        candidates.push(r"C:\ProgramData\miniforge3\Scripts\conda.exe".into());
        candidates.push(r"C:\ProgramData\Anaconda3\Scripts\conda.exe".into());
        if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
            for subdir in ["miniforge3", "miniconda3", "anaconda3", "mambaforge"] {
                candidates.push(format!(r"{}\{}\Scripts\conda.exe", home, subdir));
            }
        }
    }

    #[cfg(not(windows))]
    {
        candidates.push("/opt/homebrew/bin/conda".into());
        candidates.push("/usr/local/bin/conda".into());
        if let Ok(home) = std::env::var("HOME") {
            for subdir in ["miniforge3", "miniconda3", "anaconda3", "mambaforge"] {
                candidates.push(format!("{}/{}/bin/conda", home, subdir));
                candidates.push(format!("{}/{}/condabin/conda", home, subdir));
            }
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

/// The copy of `name` shipped inside the app, if any. Windows installs it
/// next to the executable; a macOS bundle keeps resources in
/// Contents/Resources, beside the executable's Contents/MacOS.
fn bundled_binary(name: &str) -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let file = if cfg!(windows) { format!("{}.exe", name) } else { name.to_string() };
    [dir.join(&file), dir.join("../Resources").join(&file)]
        .into_iter()
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().to_string())
}

/// Find the ffprobe executable.
/// Checks: bundled with the app, PATH, common install locations
fn find_ffprobe() -> Option<String> {
    if let Some(bundled) = bundled_binary("ffprobe") {
        return Some(bundled);
    }

    // Check PATH
    if hidden_command("ffprobe")
        .arg("-version")
        .output()
        .is_ok()
    {
        return Some("ffprobe".to_string());
    }

    // Common install locations
    #[cfg(windows)]
    let common_paths: &[&str] = &[
        r"C:\ffmpeg\bin\ffprobe.exe",
        r"C:\Program Files\ffmpeg\bin\ffprobe.exe",
        r"C:\tools\ffmpeg\bin\ffprobe.exe",
    ];
    #[cfg(not(windows))]
    let common_paths: &[&str] = &[
        "/opt/homebrew/bin/ffprobe",
        "/usr/local/bin/ffprobe",
        "/usr/bin/ffprobe",
    ];
    for path in common_paths {
        if Path::new(path).exists() {
            return Some(path.to_string());
        }
    }

    None
}

/// Find the ffmpeg executable
fn find_ffmpeg() -> Option<String> {
    if let Some(bundled) = bundled_binary("ffmpeg") {
        return Some(bundled);
    }

    if hidden_command("ffmpeg")
        .arg("-version")
        .output()
        .is_ok()
    {
        return Some("ffmpeg".to_string());
    }

    #[cfg(windows)]
    let common_paths: &[&str] = &[
        r"C:\ffmpeg\bin\ffmpeg.exe",
        r"C:\Program Files\ffmpeg\bin\ffmpeg.exe",
        r"C:\tools\ffmpeg\bin\ffmpeg.exe",
    ];
    #[cfg(not(windows))]
    let common_paths: &[&str] = &[
        "/opt/homebrew/bin/ffmpeg",
        "/usr/local/bin/ffmpeg",
        "/usr/bin/ffmpeg",
    ];
    for path in common_paths {
        if Path::new(path).exists() {
            return Some(path.to_string());
        }
    }

    None
}

/// Resolved binary paths. Locating ffmpeg/ffprobe spawns `-version` probe
/// processes, so the answer is cached for the lifetime of the app instead of
/// being recomputed on every extraction / thumbnail / metadata call.
static FFPROBE_PATH: OnceLock<Option<String>> = OnceLock::new();
static FFMPEG_PATH: OnceLock<Option<String>> = OnceLock::new();

pub fn get_ffprobe() -> Result<String, String> {
    FFPROBE_PATH
        .get_or_init(find_ffprobe)
        .clone()
        .ok_or_else(|| {
            "FFprobe not found. Please install FFmpeg and ensure it is in your PATH.".to_string()
        })
}

pub fn get_ffmpeg() -> Result<String, String> {
    FFMPEG_PATH
        .get_or_init(find_ffmpeg)
        .clone()
        .ok_or_else(|| {
            "FFmpeg not found. Please install FFmpeg and ensure it is in your PATH.".to_string()
        })
}

/// Cache key for probed metadata: path + size + mtime. If the file is replaced
/// on disk the key changes and we re-probe.
type MetaKey = (String, u64, Option<std::time::SystemTime>);

static METADATA_CACHE: StdMutex<Option<HashMap<MetaKey, VideoMetadata>>> = StdMutex::new(None);

fn meta_key(path: &str) -> Option<MetaKey> {
    let m = std::fs::metadata(path).ok()?;
    Some((path.to_string(), m.len(), m.modified().ok()))
}

/// Get video metadata using ffprobe, memoized per file.
///
/// `get_metadata` used to be re-run for every frame extraction, thumbnail batch
/// and export segment -- one process spawn each, which is a visible cost on
/// large files. Results are cached by (path, size, mtime).
pub fn get_metadata(path: &str) -> Result<VideoMetadata, String> {
    let key = meta_key(path);

    if let Some(k) = key.as_ref() {
        if let Ok(guard) = METADATA_CACHE.lock() {
            if let Some(map) = guard.as_ref() {
                if let Some(hit) = map.get(k) {
                    return Ok(hit.clone());
                }
            }
        }
    }

    let meta = probe_metadata(path)?;

    if let Some(k) = key {
        if let Ok(mut guard) = METADATA_CACHE.lock() {
            let map = guard.get_or_insert_with(HashMap::new);
            // Bound the cache; this app only ever works on a handful of files.
            if map.len() > 32 {
                map.clear();
            }
            map.insert(k, meta.clone());
        }
    }

    Ok(meta)
}

fn probe_metadata(path: &str) -> Result<VideoMetadata, String> {
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

/// Extract frames from a video as RGBA pixel data, **streaming** them to a
/// callback one at a time.
///
/// The previous implementation buffered the entire raw RGBA output of FFmpeg in
/// a single `Vec<u8>` and then copied every frame out into its own `Vec`, so
/// peak memory was twice the size of the decoded sequence. At the app's default
/// settings (output = source resolution, segment = whole video) that is
/// 1920*1080*4 bytes * fps * duration -- ~23 GiB for a ten-minute 1080p clip,
/// and ~4x that for 4K. Large videos simply ran the machine out of memory.
///
/// Streaming keeps exactly one frame alive at a time, so peak memory is
/// width*height*4 bytes regardless of how long the video is. The callback may
/// return `false` to abort (cancellation); it also lets us report genuine
/// per-frame extraction progress instead of a single jump per segment.
pub fn extract_frames_streaming(
    path: &str,
    fps: f64,
    start_time: Option<f64>,
    end_time: Option<f64>,
    speed: f64,
    crop: Option<CropRect>,
    output_width: Option<u32>,
    output_height: Option<u32>,
    on_frame: &mut dyn FnMut(FrameData) -> bool,
) -> Result<usize, String> {
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

    // Scale to the user-selected output dimensions (or crop/source size as fallback).
    // Scaling here — not only in gifski — keeps memory use and extraction time reasonable.
    let out_w = output_width
        .or(crop_w)
        .unwrap_or(metadata.width)
        .max(2)
        & !1;
    let out_h = output_height
        .or(crop_h)
        .unwrap_or(metadata.height)
        .max(2)
        & !1;
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
        "-an", "-sn",
        "-vf", &filter_chain,
        "-pix_fmt", "rgba",
        "-f", "rawvideo",
        "-v", "error",
        "pipe:1",
    ]);

    let mut child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn ffmpeg: {}", e))?;

    let pid = child.id();
    register_pid(pid);

    // Drain stderr on a side thread so a chatty FFmpeg can never deadlock by
    // filling the pipe buffer while we are busy reading stdout.
    let stderr_handle = child.stderr.take().map(|stderr| {
        std::thread::spawn(move || {
            use std::io::Read;
            let mut buf = String::new();
            let _ = std::io::BufReader::new(stderr).read_to_string(&mut buf);
            buf
        })
    });

    let frame_size = (out_w as usize) * (out_h as usize) * 4; // RGBA = 4 bytes per pixel
    let mut produced = 0usize;
    let mut aborted = false;
    let mut read_err: Option<String> = None;

    {
        let mut stdout = child.stdout.take().ok_or("No stdout pipe from ffmpeg")?;
        // One reusable staging buffer -- the only large allocation held across
        // the whole loop besides the frame currently handed to the callback.
        let mut buf = vec![0u8; frame_size];

        loop {
            match read_exact_or_eof(&mut stdout, &mut buf) {
                Ok(0) => break, // clean EOF on a frame boundary
                Ok(n) if n < frame_size => break, // trailing partial frame, discard
                Ok(_) => {}
                Err(e) => {
                    read_err = Some(format!("Failed reading ffmpeg output: {}", e));
                    break;
                }
            }

            let frame = FrameData {
                width: out_w,
                height: out_h,
                rgba: buf.clone(),
            };
            produced += 1;

            if !on_frame(frame) {
                aborted = true;
                break;
            }
        }
    }

    if aborted {
        // Consumer asked us to stop: kill FFmpeg rather than draining the rest.
        let _ = child.kill();
    }

    let status = child.wait();
    unregister_pid(pid);
    let stderr_text = stderr_handle
        .and_then(|h| h.join().ok())
        .unwrap_or_default();

    if aborted {
        return Ok(produced);
    }

    if let Some(e) = read_err {
        return Err(e);
    }

    match status {
        Ok(s) if !s.success() => {
            return Err(format!(
                "FFmpeg frame extraction failed: {}",
                stderr_text.trim()
            ));
        }
        Err(e) => return Err(format!("FFmpeg process failed: {}", e)),
        _ => {}
    }

    if produced == 0 {
        return Err("No frames were extracted from the video".to_string());
    }

    Ok(produced)
}

/// Read exactly `buf.len()` bytes, tolerating short reads from the pipe.
/// Returns the number of bytes actually read (0 = clean EOF).
fn read_exact_or_eof<R: std::io::Read>(r: &mut R, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match r.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

/// Generate thumbnail images at evenly spaced intervals for the timeline.
/// If `start_time` and `end_time` are provided, thumbnails span that range;
/// otherwise they span the full video duration.
///
/// Strategy: one short-lived FFmpeg per thumbnail, each using `-ss` *before*
/// `-i` so the demuxer seeks straight to the nearest keyframe instead of
/// decoding the file from the beginning. The seeks run concurrently.
///
/// The old approach ran a single FFmpeg over the whole range with an `fps`
/// filter, which decodes **every frame of the video** just to keep ~20 of them.
/// Measured on this machine (20 thumbnails):
///   1920x1080 / 600s / 851MB : 2736 ms -> 605 ms  (4.5x)
///   3840x2160 / 300s / 508MB : 8542 ms -> 3221 ms (2.7x)
/// and, because each thumbnail finishes independently, the timeline can fill in
/// progressively instead of staying blank until the whole batch is done.
/// Upper bound on thumbnails per request, matching the old behaviour.
pub const MAX_THUMBNAILS: usize = 30;

/// How many FFmpeg seeks to keep in flight. Thumbnail extraction is dominated
/// by disk reads and keyframe-to-target decoding, so oversubscribing cores is
/// fine; this just stops a huge `count` from spawning 30 processes at once on a
/// small machine.
fn thumbnail_concurrency() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(2, 16)
}

/// Generate thumbnails, invoking `on_thumb(index, bmp_bytes)` as each one
/// completes (out of order). Return `false` from the callback to cancel.
pub fn generate_thumbnails_streaming(
    path: &str,
    count: usize,
    thumb_height: u32,
    start_time: Option<f64>,
    end_time: Option<f64>,
    on_thumb: &mut dyn FnMut(usize, &[u8]) -> bool,
) -> Result<usize, String> {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::mpsc;

    let ffmpeg = get_ffmpeg()?;
    let metadata = get_metadata(path)?;

    let count = count.min(MAX_THUMBNAILS);

    let range_start = start_time.unwrap_or(0.0);
    let range_end = end_time.unwrap_or(metadata.duration);
    let range = range_end - range_start;
    if range <= 0.0 || count == 0 {
        return Ok(0);
    }

    // Calculate thumbnail width maintaining aspect ratio
    let aspect = metadata.width as f64 / metadata.height as f64;
    let thumb_width = ((thumb_height as f64 * aspect).round() as u32).max(2) & !1;
    let thumb_height_even = thumb_height.max(2) & !1;

    // Sample the middle of each slice rather than its leading edge, so the last
    // thumbnail is not pinned to the final frame (often black on real footage).
    let interval = range / count as f64;
    let timestamps: Vec<f64> = (0..count)
        .map(|i| (range_start + (i as f64 + 0.5) * interval).min(range_end - 0.001).max(0.0))
        .collect();

    let next = AtomicUsize::new(0);
    let cancelled = AtomicBool::new(false);
    let (tx, rx) = mpsc::channel::<(usize, Vec<u8>)>();

    let workers = thumbnail_concurrency().min(count);
    let mut delivered = 0usize;

    std::thread::scope(|scope| {
        for _ in 0..workers {
            let tx = tx.clone();
            let next = &next;
            let cancelled = &cancelled;
            let ffmpeg = ffmpeg.as_str();
            let timestamps = &timestamps;
            scope.spawn(move || {
                loop {
                    if cancelled.load(Ordering::Relaxed) {
                        return;
                    }
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&ts) = timestamps.get(i) else { return };

                    let mut cmd = hidden_command(ffmpeg);
                    cmd.args([
                        // -ss before -i: demuxer-level seek, no full decode.
                        "-ss", &format!("{:.3}", ts),
                        "-i", path,
                        "-frames:v", "1",
                        "-an", "-sn",
                        "-vf", &format!("scale={}:{}", thumb_width, thumb_height_even),
                        "-f", "image2",
                        "-c:v", "bmp",
                        "-v", "error",
                        "pipe:1",
                    ]);

                    if let Ok(output) = tracked_output(cmd) {
                        if output.status.success() && !output.stdout.is_empty() {
                            if tx.send((i, output.stdout)).is_err() {
                                return;
                            }
                        }
                    }
                }
            });
        }
        // Drop the extra sender so `rx` ends once every worker is done.
        drop(tx);

        for (i, data) in rx {
            if !on_thumb(i, &data) {
                cancelled.store(true, Ordering::Relaxed);
                break;
            }
            delivered += 1;
        }
        // The channel is unbounded, so workers never block on `send`; flagging
        // cancellation just stops them from starting further FFmpeg processes.
        cancelled.store(true, Ordering::Relaxed);
    });

    Ok(delivered)
}

/// Audio counterpart of the video trim and speed filters, so sound stays in
/// sync with the picture. `trim_start`/`trim_end` match the video trim exactly.
fn build_audio_filter_chain(trim_start: f64, trim_end: f64, speed: f64) -> String {
    let mut filters = vec![format!(
        "atrim=start={}:end={},asetpts=PTS-STARTPTS",
        trim_start, trim_end
    )];

    if (speed - 1.0).abs() > 0.01 {
        // atempo only accepts 0.5..2.0 per instance, so chain it for larger changes.
        let mut remaining = speed;
        while remaining > 2.0 {
            filters.push("atempo=2.0".to_string());
            remaining /= 2.0;
        }
        while remaining < 0.5 {
            filters.push("atempo=0.5".to_string());
            remaining /= 0.5;
        }
        filters.push(format!("atempo={}", remaining));
    }

    filters.join(",")
}

/// FFmpeg arguments for the audio side of an export: the first audio track,
/// trimmed and retimed to match the video, or `-an` when the source is silent.
fn audio_export_args(metadata: &VideoMetadata, trim_start: f64, trim_end: f64, speed: f64) -> Vec<String> {
    if !metadata.has_audio {
        return vec!["-an".to_string()];
    }
    [
        "-map", "0:v:0",
        "-map", "0:a:0",
        "-af", &build_audio_filter_chain(trim_start, trim_end, speed),
        "-c:a", "aac",
        "-b:a", "192k",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
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
    cmd.args(["-i", input_path, "-vf", &filter_chain]);
    cmd.args(audio_export_args(&metadata, actual_start, actual_end, speed));
    cmd.args([
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
///
/// `on_segment_progress` receives 0..1 within this segment, parsed from
/// FFmpeg's own `-progress` stream, so long encodes show a moving bar instead of
/// jumping only when a whole segment finishes.
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
    on_segment_progress: &dyn Fn(f64),
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
    ]);
    cmd.args(audio_export_args(metadata, trim_start, trim_end, speed));
    cmd.args([
        "-c:v", "libx264",
        "-preset", "fast",
        "-crf", "18",
        // Machine-readable progress on stdout, parsed below.
        "-progress", "pipe:1",
        "-nostats",
        "-v", "error",
        "-y",
        output_path,
    ]);

    // The encoded segment plays back over `seg_duration / speed` seconds, which
    // is what FFmpeg's out_time counts against.
    let out_duration = (seg_duration / speed.max(0.001)).max(0.001);

    run_with_progress(cmd, out_duration, on_segment_progress)
        .map_err(|e| format!("FFmpeg segment export failed: {}", e))
}

/// Spawn an FFmpeg configured with `-progress pipe:1` and translate its
/// `out_time_us=` lines into a 0..1 fraction of `out_duration` seconds.
fn run_with_progress(
    mut cmd: Command,
    out_duration: f64,
    on_progress: &dyn Fn(f64),
) -> Result<(), String> {
    use std::io::BufRead;

    let mut child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn ffmpeg: {}", e))?;

    let pid = child.id();
    register_pid(pid);

    let stderr_handle = child.stderr.take().map(|stderr| {
        std::thread::spawn(move || {
            use std::io::Read;
            let mut buf = String::new();
            let _ = std::io::BufReader::new(stderr).read_to_string(&mut buf);
            buf
        })
    });

    if let Some(stdout) = child.stdout.take() {
        for line in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
            // FFmpeg emits `key=value` pairs; out_time_us is microseconds of
            // output written so far.
            if let Some(v) = line.strip_prefix("out_time_us=") {
                if let Ok(us) = v.trim().parse::<i64>() {
                    if us >= 0 {
                        on_progress((us as f64 / 1_000_000.0 / out_duration).clamp(0.0, 1.0));
                    }
                }
            } else if line.trim() == "progress=end" {
                on_progress(1.0);
            }
        }
    }

    let status = child.wait().map_err(|e| format!("process wait failed: {}", e));
    unregister_pid(pid);
    let stderr_text = stderr_handle.and_then(|h| h.join().ok()).unwrap_or_default();

    match status? {
        s if s.success() => Ok(()),
        s => Err(format!(
            "exit {}: {}",
            s.code().unwrap_or(-1),
            stderr_text.trim()
        )),
    }
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
    on_progress: &dyn Fn(f64), // overall 0..1
) -> Result<Vec<Mp4Result>, String> {
    let ffmpeg = get_ffmpeg()?;
    let metadata = get_metadata(input_path)?;

    if segments.is_empty() {
        return Err("No segments to export".to_string());
    }

    let total = segments.len();

    // Weight each segment by its duration so the bar advances at a steady rate
    // regardless of how unevenly the segments are sized.
    let seg_durations: Vec<f64> = segments.iter().map(|&(s, e)| (e - s).max(0.0)).collect();
    let duration_total: f64 = seg_durations.iter().sum::<f64>().max(0.001);
    let seg_offset = |i: usize| -> f64 { seg_durations[..i].iter().sum::<f64>() / duration_total };
    let seg_weight = |i: usize| -> f64 { seg_durations[i] / duration_total };

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

            let base_p = seg_offset(i);
            let weight = seg_weight(i);
            export_single_segment(
                &ffmpeg, input_path, &path, &metadata,
                start, end, speed, crop, output_width, output_height,
                &|p| on_progress(base_p + p * weight),
            )?;

            let file_size = std::fs::metadata(&path)
                .map(|m| m.len())
                .unwrap_or(0);

            results.push(Mp4Result {
                file_path: path,
                file_size,
            });

            on_progress(seg_offset(i) + weight);
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

            // Reserve the last 5% of the bar for the concat pass.
            const ENCODE_SHARE: f64 = 0.95;
            let base_p = seg_offset(i) * ENCODE_SHARE;
            let weight = seg_weight(i) * ENCODE_SHARE;
            export_single_segment(
                &ffmpeg, input_path, &temp_path_str, &metadata,
                start, end, speed, crop, output_width, output_height,
                &|p| on_progress(base_p + p * weight),
            )?;

            temp_files.push(temp_path_str);
            on_progress(base_p + weight);
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

        on_progress(1.0);

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

/// Raw frame data extracted from video.
/// Ordering and presentation timestamps are assigned by the consumer, which
/// streams frames across several segments into a single output timeline.
pub struct FrameData {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_video() -> Option<String> {
        std::env::var("VIDEOBRICKS_TEST_VIDEO")
            .ok()
            .filter(|p| Path::new(p).is_file())
    }

    fn is_bmp(d: &[u8]) -> bool {
        d.len() > 14 && d[0] == 0x42 && d[1] == 0x4D
    }

    /// Every requested thumbnail should arrive exactly once, as a valid BMP,
    /// and be distinct from its neighbours (i.e. actually sampled at different
    /// timestamps rather than all landing on the same keyframe).
    #[test]
    fn thumbnails_are_complete_distinct_and_valid() {
        let Some(path) = test_video() else { return };
        const COUNT: usize = 20;
        let mut got: Vec<Option<Vec<u8>>> = vec![None; COUNT];

        let t = std::time::Instant::now();
        let delivered = generate_thumbnails_streaming(&path, COUNT, 40, None, None, &mut |i, d| {
            assert!(is_bmp(d), "thumbnail {} is not a BMP", i);
            assert!(got[i].is_none(), "thumbnail {} delivered twice", i);
            got[i] = Some(d.to_vec());
            true
        })
        .unwrap();
        let ms = t.elapsed().as_millis();

        assert_eq!(delivered, COUNT, "expected {} thumbnails", COUNT);
        assert!(got.iter().all(|g| g.is_some()), "some slots never filled");
        println!("\n  {} thumbnails in {} ms\n", COUNT, ms);

        // Distinct content: a timeline of identical frames would mean seeking is
        // not actually landing at the requested timestamps.
        let distinct: std::collections::HashSet<_> =
            got.iter().flatten().map(|d| d.as_slice()).collect();
        assert!(
            distinct.len() > COUNT / 2,
            "only {} distinct thumbnails out of {}",
            distinct.len(),
            COUNT
        );
    }

    /// Returning `false` from the callback must stop the run promptly.
    #[test]
    fn thumbnail_streaming_can_be_cancelled() {
        let Some(path) = test_video() else { return };
        let mut seen = 0;
        let delivered =
            generate_thumbnails_streaming(&path, 20, 40, None, None, &mut |_, _| {
                seen += 1;
                false // stop after the first
            })
            .unwrap();
        assert_eq!(delivered, 0, "cancelled delivery should not count");
        assert_eq!(seen, 1, "callback ran {} times after cancelling", seen);
    }

    /// Range thumbnails must sample inside the requested window only.
    #[test]
    fn range_thumbnails_differ_from_full_span() {
        let Some(path) = test_video() else { return };
        let meta = get_metadata(&path).unwrap();
        let mut full = Vec::new();
        generate_thumbnails_streaming(&path, 6, 40, None, None, &mut |_, d| {
            full.push(d.to_vec());
            true
        })
        .unwrap();

        let mut ranged = Vec::new();
        let a = meta.duration * 0.1;
        let b = meta.duration * 0.2;
        generate_thumbnails_streaming(&path, 6, 40, Some(a), Some(b), &mut |_, d| {
            ranged.push(d.to_vec());
            true
        })
        .unwrap();

        assert_eq!(ranged.len(), 6);
        assert!(ranged.iter().all(|d| is_bmp(d)));
        assert_ne!(full, ranged, "range request returned the full-span strip");
    }

    /// Metadata is memoized, so a repeat probe must be far cheaper than the first.
    #[test]
    fn metadata_is_cached() {
        let Some(path) = test_video() else { return };
        let first = get_metadata(&path).unwrap();
        let t = std::time::Instant::now();
        let second = get_metadata(&path).unwrap();
        let cached_us = t.elapsed().as_micros();
        assert_eq!(first.width, second.width);
        assert_eq!(first.duration, second.duration);
        assert!(cached_us < 5_000, "cached probe took {} us", cached_us);
    }

    /// MP4 export must report progress that advances and reaches completion.
    #[test]
    fn mp4_export_reports_real_progress() {
        let Some(path) = test_video() else { return };
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out.mp4");

        let seen = std::sync::Mutex::new(Vec::<f64>::new());
        let results = export_mp4_segments(
            &path,
            out.to_str().unwrap(),
            &[(2.0, 6.0)],
            "merge",
            1.0,
            None,
            Some(320),
            Some(180),
            &|p| seen.lock().unwrap().push(p),
        )
        .unwrap();

        assert_eq!(results.len(), 1);
        assert!(results[0].file_size > 0, "export produced an empty file");

        let v = seen.lock().unwrap().clone();
        assert!(v.len() > 2, "only {} progress reports -- not streaming", v.len());
        assert!(
            v.iter().any(|&p| p > 0.0 && p < 1.0),
            "no intermediate progress, only endpoints: {:?}",
            v
        );
        assert!((v.last().unwrap() - 1.0).abs() < 1e-6, "ended at {:?}", v.last());
    }

    /// Build a short synthetic clip (test pattern + tone) so audio export can be
    /// checked without a sample video on disk.
    fn synthetic_clip(dir: &Path, with_audio: bool) -> Option<String> {
        let ffmpeg = get_ffmpeg().ok()?;
        let out = dir.join(if with_audio { "src_a.mp4" } else { "src.mp4" });
        let mut cmd = hidden_command(&ffmpeg);
        cmd.args(["-f", "lavfi", "-i", "testsrc=size=320x240:rate=30:duration=10"]);
        if with_audio {
            cmd.args(["-f", "lavfi", "-i", "sine=frequency=440:duration=10"]);
        }
        cmd.args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-shortest", "-y"]);
        cmd.arg(&out);
        cmd.output().ok()?.status.success().then(|| out.to_string_lossy().to_string())
    }

    /// Duration in seconds of the output's first stream of `kind` ("v" or "a").
    fn stream_duration(path: &str, kind: &str) -> Option<f64> {
        let out = hidden_command(get_ffprobe().ok()?)
            .args(["-v", "error", "-select_streams", &format!("{}:0", kind),
                   "-show_entries", "stream=duration", "-of", "csv=p=0", path])
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    }

    /// Exports keep the soundtrack, trimmed and retimed to match the video, in
    /// split, merged and sped-up exports alike; silent sources still export.
    #[test]
    fn mp4_export_keeps_audio_in_sync() {
        let dir = tempfile::tempdir().unwrap();
        let Some(src) = synthetic_clip(dir.path(), true) else { return };

        // (segments, mode, speed, expected output seconds)
        let cases: [(&[(f64, f64)], &str, f64, f64); 5] = [
            (&[(2.0, 6.0)], "merge", 1.0, 4.0),
            (&[(1.0, 3.0), (5.0, 8.0)], "merge", 1.0, 5.0),
            (&[(1.0, 3.0), (5.0, 8.0)], "merge", 2.0, 2.5),
            (&[(0.0, 9.0)], "merge", 0.25, 36.0),
            (&[(0.0,0.5),(1.0,1.5),(2.0,2.5),(3.0,3.5),(4.0,4.5),(5.0,5.5),(6.0,6.5),(7.0,7.5),(8.0,8.5),(9.0,9.5)], "merge", 1.0, 5.0),
        ];
        for (i, (segs, mode, speed, expected)) in cases.iter().enumerate() {
            let out = dir.path().join(format!("out_{}.mp4", i));
            let out = out.to_str().unwrap();
            export_mp4_segments(&src, out, segs, mode, *speed, None, None, None, &|_| {}).unwrap();
            let v = stream_duration(out, "v").expect("video stream");
            let a = stream_duration(out, "a").unwrap_or_else(|| panic!("case {}: no audio stream", i));
            assert!((v - expected).abs() < 0.15, "case {}: video {}s, expected {}s", i, v, expected);
            assert!((a - expected).abs() < 0.15, "case {}: audio {}s, expected {}s", i, a, expected);
        }

        // Split mode: every file gets its own audio.
        let out = dir.path().join("split.mp4");
        let results = export_mp4_segments(
            &src, out.to_str().unwrap(), &[(1.0, 3.0), (5.0, 8.0)], "split", 1.0,
            None, None, None, &|_| {},
        ).unwrap();
        for (r, expected) in results.iter().zip([2.0, 3.0]) {
            let a = stream_duration(&r.file_path, "a").expect("split file has audio");
            assert!((a - expected).abs() < 0.15, "split audio {}s, expected {}s", a, expected);
        }

        // A source without audio must still export (no audio stream, no error).
        let silent = synthetic_clip(dir.path(), false).unwrap();
        let out = dir.path().join("silent_out.mp4");
        export_mp4_segments(&silent, out.to_str().unwrap(), &[(1.0, 4.0)], "merge", 1.0,
            None, None, None, &|_| {}).unwrap();
        assert!(stream_duration(out.to_str().unwrap(), "a").is_none());
    }
}
