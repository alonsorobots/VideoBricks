use crate::commands::convert::ConversionState;
use crate::commands::convert::ConversionProgressEvent;
use crate::gifski_bridge::ConversionSettings;
use crate::video::ffmpeg;
use std::process::Command;
use std::sync::Mutex;
use tauri::{Emitter, Manager};

/// Snapshot the rendered GIF paths without holding the state lock across I/O.
fn gif_paths(app: &tauri::AppHandle) -> Result<Vec<std::path::PathBuf>, String> {
    let state = app.state::<Mutex<ConversionState>>();
    let s = state.lock().map_err(|_| "State lock failed")?;
    let paths = s.gif_paths.lock().map_err(|_| "Data lock failed")?;
    if paths.is_empty() {
        return Err("No GIF data available. Please convert a video first.".to_string());
    }
    Ok(paths.clone())
}

/// Save a single GIF (first result) to a file
#[tauri::command]
pub async fn save_gif_file(
    app: tauri::AppHandle,
    output_path: String,
) -> Result<u64, String> {
    let paths = gif_paths(&app)?;
    std::fs::copy(&paths[0], &output_path)
        .map_err(|e| format!("Failed to write file: {}", e))
}

/// Save all GIFs to a directory with numbered filenames.
/// Returns the list of saved file paths.
#[tauri::command]
pub async fn save_all_gif_files(
    app: tauri::AppHandle,
    directory: String,
    base_name: String,
) -> Result<Vec<String>, String> {
    let sources = gif_paths(&app)?;

    let dir = std::path::Path::new(&directory);
    if !dir.exists() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Failed to create directory: {}", e))?;
    }

    let mut saved_paths = Vec::new();

    for (i, source) in sources.iter().enumerate() {
        let path = if sources.len() == 1 {
            dir.join(format!("{}.gif", base_name))
        } else {
            dir.join(format!("{}_{:02}.gif", base_name, i + 1))
        };
        std::fs::copy(source, &path)
            .map_err(|e| format!("Failed to write file {}: {}", i + 1, e))?;
        saved_paths.push(path.to_string_lossy().to_string());
    }

    Ok(saved_paths)
}

/// Copy the GIF data to clipboard (as file path via temp file)
#[tauri::command]
pub async fn copy_gif_to_clipboard(
    app: tauri::AppHandle,
) -> Result<String, String> {
    // The rendered GIF already lives in a temp file; hand back its path.
    let paths = gif_paths(&app)?;
    Ok(paths[0].to_string_lossy().to_string())
}

/// Convert video to MP4 with all settings applied.
/// Supports multi-segment merge and split modes.
/// Returns array of Mp4Result with file paths and sizes.
#[tauri::command]
pub async fn convert_to_mp4(
    app: tauri::AppHandle,
    output_path: String,
    settings: ConversionSettings,
) -> Result<Vec<ffmpeg::Mp4Result>, String> {
    let segments: Vec<(f64, f64)> = settings
        .segments
        .iter()
        .map(|s| (s.start, s.end))
        .collect();

    let segment_mode = settings.segment_mode.clone();

    #[cfg(windows)]
    let window = app.get_webview_window("main");

    let app_handle = app.clone();

    let results = tokio::task::spawn_blocking(move || {
        ffmpeg::export_mp4_segments(
            &settings.source_path,
            &output_path,
            &segments,
            &segment_mode,
            settings.speed,
            settings.crop,
            settings.width,
            settings.height,
            &|p: f64| {
                let p = p.clamp(0.0, 1.0);
                let _ = app_handle.emit(
                    "conversion-progress",
                    ConversionProgressEvent {
                        // Percent-scaled so the UI's "n of m" readout stays sane
                        // for MP4, which has no frame accounting of its own.
                        completed_frames: (p * 100.0).round() as u32,
                        total_frames: 100,
                        progress: p,
                        stage: "encoding".to_string(),
                    },
                );
                #[cfg(windows)]
                if let Some(ref win) = window {
                    crate::taskbar::set_progress(win, p);
                }
            },
        )
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))??;

    // Clear taskbar progress
    #[cfg(windows)]
    if let Some(win) = app.get_webview_window("main") {
        crate::taskbar::clear_progress(&win);
    }

    Ok(results)
}

/// Reveal a file in the system file explorer
#[tauri::command]
pub async fn reveal_in_explorer(path: String) -> Result<(), String> {
    #[cfg(windows)]
    {
        Command::new("explorer")
            .args(["/select,", &path])
            .spawn()
            .map_err(|e| format!("Failed to open explorer: {}", e))?;
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .args(["-R", &path])
            .spawn()
            .map_err(|e| format!("Failed to open Finder: {}", e))?;
    }
    #[cfg(target_os = "linux")]
    {
        let parent = std::path::Path::new(&path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| path.clone());
        Command::new("xdg-open")
            .arg(&parent)
            .spawn()
            .map_err(|e| format!("Failed to open file manager: {}", e))?;
    }
    Ok(())
}

/// Get a temp directory for export previews
#[tauri::command]
pub async fn get_temp_dir() -> Result<String, String> {
    let temp = std::env::temp_dir().join("videobricks-export");
    std::fs::create_dir_all(&temp)
        .map_err(|e| format!("Failed to create temp dir: {}", e))?;
    Ok(temp.to_string_lossy().to_string())
}

/// Copy a single file from source to destination
#[tauri::command]
pub async fn copy_file(source: String, destination: String) -> Result<u64, String> {
    // Ensure parent directory exists
    if let Some(parent) = std::path::Path::new(&destination).parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory: {}", e))?;
    }
    let bytes = std::fs::copy(&source, &destination)
        .map_err(|e| format!("Failed to copy file: {}", e))?;
    Ok(bytes)
}

/// Copy multiple files to a directory with a base name.
/// Files are named baseName.ext (single) or baseName_01.ext, baseName_02.ext, ... (multiple).
/// Returns list of destination paths.
#[tauri::command]
pub async fn copy_files_to_directory(
    sources: Vec<String>,
    directory: String,
    base_name: String,
) -> Result<Vec<String>, String> {
    let dir = std::path::Path::new(&directory);
    if !dir.exists() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Failed to create directory: {}", e))?;
    }

    let mut saved_paths = Vec::new();

    for (i, source) in sources.iter().enumerate() {
        // Detect extension from source
        let ext = std::path::Path::new(source)
            .extension()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_else(|| "mp4".to_string());

        let dest = if sources.len() == 1 {
            dir.join(format!("{}.{}", base_name, ext))
        } else {
            dir.join(format!("{}_{:02}.{}", base_name, i + 1, ext))
        };

        std::fs::copy(source, &dest)
            .map_err(|e| format!("Failed to copy file {}: {}", i + 1, e))?;

        saved_paths.push(dest.to_string_lossy().to_string());
    }

    Ok(saved_paths)
}

/// Write all in-memory GIFs to temp files and return their paths.
/// Used by the drag plugin so we have real file paths for native OS drag-out.
#[tauri::command]
pub async fn save_gifs_to_temp(
    app: tauri::AppHandle,
    base_name: String,
) -> Result<Vec<String>, String> {
    // Copy under the user-visible base name so the dragged file is named well.
    let sources = gif_paths(&app)?;

    let temp_dir = std::env::temp_dir().join("videobricks-drag");
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| format!("Failed to create temp dir: {}", e))?;

    let mut paths = Vec::new();

    for (i, source) in sources.iter().enumerate() {
        let path = if sources.len() == 1 {
            temp_dir.join(format!("{}.gif", base_name))
        } else {
            temp_dir.join(format!("{}_{:02}.gif", base_name, i + 1))
        };
        std::fs::copy(source, &path)
            .map_err(|e| format!("Failed to write temp GIF {}: {}", i + 1, e))?;
        paths.push(path.to_string_lossy().to_string());
    }

    Ok(paths)
}

/// Export the video with modifications (not as GIF) - legacy single-segment export
#[tauri::command]
pub async fn export_video(
    input_path: String,
    output_path: String,
    settings: ConversionSettings,
) -> Result<(), String> {
    // Use the first segment start and last segment end for export
    let start_time = settings.segments.first().map(|s| s.start);
    let end_time = settings.segments.last().map(|s| s.end);

    tokio::task::spawn_blocking(move || {
        ffmpeg::export_modified_video(
            &input_path,
            &output_path,
            start_time,
            end_time,
            settings.speed,
            settings.crop,
            settings.width,
            settings.height,
        )
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}
