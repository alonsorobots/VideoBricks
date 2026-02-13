use crate::gifski_bridge::{self, ConversionSettings};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{Emitter, Manager};
use std::sync::Mutex;
use serde::Serialize;

/// Shared state for cancellation + result storage
pub struct ConversionState {
    pub cancelled: Arc<AtomicBool>,
    /// All GIF results (1 element for merge, N for split)
    pub gif_data_list: Mutex<Vec<Vec<u8>>>,
}

impl Default for ConversionState {
    fn default() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            gif_data_list: Mutex::new(Vec::new()),
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionProgressEvent {
    pub completed_frames: u32,
    pub total_frames: u32,
    pub progress: f64,
}

/// A single GIF result returned to the frontend
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GifResult {
    pub data_url: String,
    pub file_size: u64,
}

/// Convert video to GIF. Progress is emitted as events.
/// Returns an array of GifResult (1 for merge, N for split).
#[tauri::command]
pub async fn convert_to_gif(
    app: tauri::AppHandle,
    settings: ConversionSettings,
) -> Result<Vec<GifResult>, String> {
    use base64::Engine;

    // Reset cancellation
    let cancelled = Arc::new(AtomicBool::new(false));

    // Store cancellation token in app state
    let state = app.state::<Mutex<ConversionState>>();
    {
        let mut s = state.lock().map_err(|_| "State lock failed")?;
        s.cancelled = cancelled.clone();
        s.gif_data_list.lock().map_err(|_| "Data lock failed")?.clear();
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
                let p = progress.completed_frames as f64 / progress.total_frames.max(1) as f64;
                let _ = app_handle.emit("conversion-progress", ConversionProgressEvent {
                    completed_frames: progress.completed_frames,
                    total_frames: progress.total_frames,
                    progress: p,
                });
                #[cfg(windows)]
                if let Some(ref win) = window {
                    crate::taskbar::set_progress(win, p);
                }
            })
        })
        .await
        .map_err(|e| format!("Task failed: {}", e))??
    } else {
        // Merge mode: all segments into one GIF
        let data = tokio::task::spawn_blocking(move || {
            gifski_bridge::convert_to_gif(&settings, cancelled_clone, move |progress| {
                let p = progress.completed_frames as f64 / progress.total_frames.max(1) as f64;
                let _ = app_handle.emit("conversion-progress", ConversionProgressEvent {
                    completed_frames: progress.completed_frames,
                    total_frames: progress.total_frames,
                    progress: p,
                });
                #[cfg(windows)]
                if let Some(ref win) = window {
                    crate::taskbar::set_progress(win, p);
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

    // Store GIF data for save/copy operations
    {
        let s = state.lock().map_err(|_| "State lock failed")?;
        *s.gif_data_list.lock().map_err(|_| "Data lock failed")? = gif_data_list.clone();
    }

    // Build results with base64 data URLs
    let results: Vec<GifResult> = gif_data_list
        .iter()
        .map(|data| {
            let encoded = base64::engine::general_purpose::STANDARD.encode(data);
            GifResult {
                data_url: format!("data:image/gif;base64,{}", encoded),
                file_size: data.len() as u64,
            }
        })
        .collect();

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

/// Estimate file size by running a partial conversion
#[tauri::command]
pub async fn estimate_file_size(settings: ConversionSettings) -> Result<u64, String> {
    let cancelled = Arc::new(AtomicBool::new(false));

    // Create a modified settings for estimation (fewer frames)
    let mut est_settings = settings.clone();
    let original_fps = settings.fps;
    est_settings.fps = est_settings.fps.min(5); // Lower FPS for estimation
    let est_fps = est_settings.fps;

    let gif_data = tokio::task::spawn_blocking(move || {
        gifski_bridge::convert_to_gif(&est_settings, cancelled, |_| {})
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))??;

    // Scale up the estimate based on the FPS ratio
    let fps_ratio = original_fps as f64 / est_fps.max(1) as f64;
    let estimated = (gif_data.len() as f64 * fps_ratio) as u64;

    Ok(estimated)
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
