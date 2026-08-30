use crate::gifski_bridge::{self, ConversionSettings};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{Emitter, Manager};
use std::sync::Mutex;
use serde::Serialize;

/// Shared state for cancellation + result storage
pub struct ConversionState {
    pub cancelled: Arc<AtomicBool>,
    /// Paths of the rendered GIFs (1 element for merge, N for split).
    ///
    /// These used to be held as `Vec<Vec<u8>>` and handed to the webview as
    /// base64 data URLs. A GIF from a large source is easily hundreds of
    /// megabytes, and base64 inflates it by a third on top of that -- enough to
    /// hang or kill the webview just to show a preview. Now the bytes live in a
    /// temp file and the UI loads them over the asset protocol.
    pub gif_paths: Mutex<Vec<PathBuf>>,
}

impl Default for ConversionState {
    fn default() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            gif_paths: Mutex::new(Vec::new()),
        }
    }
}

/// Directory holding the current conversion's rendered GIFs.
pub fn output_dir() -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join("videobricks-output");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create output dir: {}", e))?;
    Ok(dir)
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionProgressEvent {
    pub completed_frames: u32,
    pub total_frames: u32,
    pub progress: f64,
    pub stage: String,
}

/// A single GIF result returned to the frontend
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GifResult {
    /// Path to the rendered GIF; the UI displays it via the asset protocol.
    pub file_path: String,
    pub file_size: u64,
}

/// Convert video to GIF. Progress is emitted as events.
/// Returns an array of GifResult (1 for merge, N for split).
#[tauri::command]
pub async fn convert_to_gif(
    app: tauri::AppHandle,
    settings: ConversionSettings,
) -> Result<Vec<GifResult>, String> {
    // Reset cancellation
    let cancelled = Arc::new(AtomicBool::new(false));

    // Store cancellation token in app state
    let state = app.state::<Mutex<ConversionState>>();
    {
        let mut s = state.lock().map_err(|_| "State lock failed")?;
        s.cancelled = cancelled.clone();
        s.gif_paths.lock().map_err(|_| "Data lock failed")?.clear();
    }

    let app_handle = app.clone();

    #[cfg(windows)]
    let window = app.get_webview_window("main");

    let cancelled_clone = cancelled.clone();

    let is_split = settings.segment_mode == "split" && settings.segments.len() > 1;

    let gif_data_list: Vec<Vec<u8>> = if is_split {
        // Split mode: convert each segment separately
        tokio::task::spawn_blocking(move || {
            gifski_bridge::convert_to_gifs_split(&settings, cancelled_clone, move |progress| {
                let _ = app_handle.emit("conversion-progress", ConversionProgressEvent {
                    completed_frames: (progress.progress * progress.total_frames as f64).round() as u32,
                    total_frames: progress.total_frames,
                    progress: progress.progress,
                    stage: progress.stage.to_string(),
                });
                #[cfg(windows)]
                if let Some(ref win) = window {
                    crate::taskbar::set_progress(win, progress.progress);
                }
            })
        })
        .await
        .map_err(|e| format!("Task failed: {}", e))??
    } else {
        // Merge mode: all segments into one GIF
        let data = tokio::task::spawn_blocking(move || {
            gifski_bridge::convert_to_gif(&settings, cancelled_clone, move |progress| {
                let _ = app_handle.emit("conversion-progress", ConversionProgressEvent {
                    completed_frames: (progress.progress * progress.total_frames as f64).round() as u32,
                    total_frames: progress.total_frames,
                    progress: progress.progress,
                    stage: progress.stage.to_string(),
                });
                #[cfg(windows)]
                if let Some(ref win) = window {
                    crate::taskbar::set_progress(win, progress.progress);
                }
            })
        })
        .await
        .map_err(|e| format!("Task failed: {}", e))??;
        vec![data]
    };

    // Clear taskbar progress
    #[cfg(windows)]
    if let Some(win) = app.get_webview_window("main") {
        crate::taskbar::clear_progress(&win);
    }

    if cancelled.load(Ordering::Relaxed) {
        return Err("Conversion cancelled".to_string());
    }

    // Write each GIF out and hand the UI file paths instead of base64 blobs.
    // A fresh id per conversion keeps the webview from serving a cached image
    // from a previous run at the same URL.
    let dir = output_dir()?;
    let run_id = uuid::Uuid::new_v4();
    let mut paths = Vec::with_capacity(gif_data_list.len());
    let mut results = Vec::with_capacity(gif_data_list.len());

    for (i, data) in gif_data_list.iter().enumerate() {
        let path = dir.join(format!("{}-{:02}.gif", run_id, i + 1));
        std::fs::write(&path, data)
            .map_err(|e| format!("Failed to write GIF: {}", e))?;
        results.push(GifResult {
            file_path: path.to_string_lossy().to_string(),
            file_size: data.len() as u64,
        });
        paths.push(path);
    }

    {
        let s = state.lock().map_err(|_| "State lock failed")?;
        *s.gif_paths.lock().map_err(|_| "Data lock failed")? = paths;
    }

    Ok(results)
}

#[tauri::command]
pub async fn cancel_conversion(app: tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<Mutex<ConversionState>>();
    let s = state.lock().map_err(|_| "State lock failed")?;
    s.cancelled.store(true, Ordering::Relaxed);

    // Kill any running FFmpeg child processes immediately
    crate::video::ffmpeg::kill_all_children();

    #[cfg(windows)]
    if let Some(win) = app.get_webview_window("main") {
        crate::taskbar::clear_progress(&win);
    }

    Ok(())
}

/// Estimate output file size from a small sample encode.
///
/// This used to run a full conversion at reduced fps, which on the app's
/// defaults meant decoding the entire video at source resolution every time the
/// user nudged a setting. `estimate_gif_size` encodes ~12 small frames instead.
#[tauri::command]
pub async fn estimate_file_size(settings: ConversionSettings) -> Result<u64, String> {
    tokio::task::spawn_blocking(move || gifski_bridge::estimate_gif_size(&settings))
        .await
        .map_err(|e| format!("Task failed: {}", e))?
}

/// Generate a preview GIF (lower quality, smaller) for the edit screen
#[tauri::command]
pub async fn generate_preview_gif(settings: ConversionSettings) -> Result<String, String> {
    use base64::Engine;

    let cancelled = Arc::new(AtomicBool::new(false));

    // Reduced settings for preview
    let mut preview_settings = settings.clone();
    preview_settings.fps = preview_settings.fps.min(8);
    // Scale down dimensions for preview
    if let Some(w) = preview_settings.width {
        preview_settings.width = Some(w.min(320));
    }
    if let Some(h) = preview_settings.height {
        preview_settings.height = Some(h.min(240));
    }

    let gif_data = tokio::task::spawn_blocking(move || {
        gifski_bridge::convert_to_gif(&preview_settings, cancelled, |_| {})
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))??;

    let encoded = base64::engine::general_purpose::STANDARD.encode(&gif_data);
    Ok(format!("data:image/gif;base64,{}", encoded))
}
