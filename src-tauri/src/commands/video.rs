use crate::video::{ffmpeg, validator};
use serde::{Deserialize, Serialize};
use tauri::Emitter;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateResult {
    pub is_valid: bool,
    pub error: Option<String>,
    pub metadata: Option<ffmpeg::VideoMetadata>,
}

#[tauri::command]
pub async fn validate_video(path: String) -> Result<ValidateResult, String> {
    let result = tokio::task::spawn_blocking(move || validator::validate_video(&path))
        .await
        .map_err(|e| format!("Task failed: {}", e))?;

    Ok(ValidateResult {
        is_valid: result.is_valid,
        error: result.error,
        metadata: result.metadata,
    })
}

#[tauri::command]
pub async fn get_video_metadata(path: String) -> Result<ffmpeg::VideoMetadata, String> {
    tokio::task::spawn_blocking(move || ffmpeg::get_metadata(&path))
        .await
        .map_err(|e| format!("Task failed: {}", e))?
}

/// Returns thumbnails as base64-encoded BMP images (full video duration)
#[tauri::command]
pub async fn get_video_thumbnails(
    path: String,
    count: usize,
    thumb_height: u32,
) -> Result<Vec<String>, String> {
    use base64::Engine;

    tokio::task::spawn_blocking(move || {
        let thumbnails = ffmpeg::generate_thumbnails(&path, count, thumb_height, None, None)?;
        Ok(thumbnails
            .into_iter()
            .map(|data| {
                let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
                format!("data:image/bmp;base64,{}", encoded)
            })
            .collect())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

// ---------------------------------------------------------------------------
// Find Shots (TransNetV2)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct SceneBoundary {
    pub start: f64,
    pub end: f64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FindShotsProgress {
    stage: String,       // "extracting" | "loading" | "analyzing" | "done"
    progress: f64,       // 0..1
    current: u64,
    total: u64,
}

#[tauri::command]
pub async fn find_shots(
    app: tauri::AppHandle,
    path: String,
    threshold: f64,
) -> Result<Vec<SceneBoundary>, String> {
    tokio::task::spawn_blocking(move || {
        use std::io::BufRead;

        // Resolve the transnet_detect.py script path
        let script_path = {
            let exe_dir = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()));
            let candidates = [
                exe_dir.as_ref().map(|d| d.join("scripts").join("transnet_detect.py")),
                Some(std::path::PathBuf::from(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/scripts/transnet_detect.py"
                ))),
            ];
            candidates
                .into_iter()
                .flatten()
                .find(|p| p.exists())
                .ok_or_else(|| "transnet_detect.py script not found".to_string())?
        };

        // Always use conda env (yt_filter) which has numpy, onnxruntime, etc.
        // If the ONNX model exists next to the script, pass --model so the
        // script uses ONNX Runtime instead of the heavier PyTorch backend.
        let onnx_model = script_path.with_file_name("transnetv2.onnx");
        let conda_exe = ffmpeg::find_conda()?;

        let mut cmd = {
            let mut c = ffmpeg::hidden_command(&conda_exe);
            c.args([
                "run",
                "-n", "yt_filter",
                "python", "-u",
                script_path.to_str().unwrap_or("transnet_detect.py"),
                &path,
                "--threshold",
                &threshold.to_string(),
            ]);
            if onnx_model.exists() {
                c.args(["--model", onnx_model.to_str().unwrap_or("transnetv2.onnx")]);
            }
            c
        };

        // Spawn with piped stdout + stderr so we can stream stderr for progress
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn: {}", e))?;
        let pid = child.id();
        ffmpeg::register_pid(pid);

        // Emit initial status
        let _ = app.emit("find-shots-progress", FindShotsProgress {
            stage: "loading".into(), progress: 0.0, current: 0, total: 0,
        });

        // Stream stderr in a background thread to parse progress
        let stderr = child.stderr.take().ok_or("No stderr pipe")?;
        let app_for_stderr = app.clone();
        let stderr_thread = std::thread::spawn(move || {
            let reader = std::io::BufReader::new(stderr);
            let mut all_stderr = String::new();
            for line in reader.lines() {
                let line = match line {
                    Ok(l) => l,
                    Err(_) => break,
                };
                all_stderr.push_str(&line);
                all_stderr.push('\n');

                // Parse progress: "[TransNetV2] Extracting frames from ..."
                if line.contains("Extracting frames") {
                    let _ = app_for_stderr.emit("find-shots-progress", FindShotsProgress {
                        stage: "extracting".into(), progress: 0.0, current: 0, total: 0,
                    });
                }
                // Parse: "[TransNetV2] Extracted 2710 frames"
                else if line.contains("Extracted") && line.contains("frames") {
                    let _ = app_for_stderr.emit("find-shots-progress", FindShotsProgress {
                        stage: "analyzing".into(), progress: 0.0, current: 0, total: 0,
                    });
                }
                // Parse: "[TransNetV2] Processing video frames 50/2710"
                else if line.contains("Processing video frames") {
                    if let Some(frac_str) = line.split("frames").nth(1) {
                        let parts: Vec<&str> = frac_str.trim().split('/').collect();
                        if parts.len() == 2 {
                            let current: u64 = parts[0].trim().parse().unwrap_or(0);
                            let total: u64 = parts[1].trim().parse().unwrap_or(1);
                            let p = if total > 0 { current as f64 / total as f64 } else { 0.0 };
                            let _ = app_for_stderr.emit("find-shots-progress", FindShotsProgress {
                                stage: "analyzing".into(), progress: p, current, total,
                            });
                        }
                    }
                }
                // Parse: "[TransNetV2] Found 38 scenes"
                else if line.contains("Found") && line.contains("scenes") {
                    let _ = app_for_stderr.emit("find-shots-progress", FindShotsProgress {
                        stage: "done".into(), progress: 1.0, current: 0, total: 0,
                    });
                }
            }
            all_stderr
        });

        // Read all stdout (JSON result)
        let stdout = child.stdout.take().ok_or("No stdout pipe")?;
        let stdout_str: String = std::io::BufReader::new(stdout)
            .lines()
            .filter_map(|l| l.ok())
            .collect::<Vec<_>>()
            .join("\n");

        let status = child.wait().map_err(|e| format!("Process wait failed: {}", e))?;
        ffmpeg::unregister_pid(pid);

        let _stderr_str = stderr_thread.join().unwrap_or_default();

        if !status.success() {
            return Err(format!(
                "TransNetV2 failed (exit {}): {}",
                status.code().unwrap_or(-1),
                _stderr_str.chars().take(1000).collect::<String>()
            ));
        }

        // Find JSON array in stdout
        let json_line = stdout_str
            .lines()
            .rev()
            .find(|line| {
                let trimmed = line.trim();
                trimmed.starts_with('[') && trimmed.ends_with(']')
            })
            .unwrap_or("[]");

        let scenes: Vec<SceneBoundary> = serde_json::from_str(json_line)
            .map_err(|e| format!("JSON parse error: {} -- raw: {:?}", e, &json_line[..json_line.len().min(200)]))?;

        Ok(scenes)
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

/// Returns thumbnails for a specific time range (detail trim mode)
#[tauri::command]
pub async fn get_video_thumbnails_range(
    path: String,
    count: usize,
    thumb_height: u32,
    start_time: f64,
    end_time: f64,
) -> Result<Vec<String>, String> {
    use base64::Engine;

    tokio::task::spawn_blocking(move || {
        let thumbnails = ffmpeg::generate_thumbnails(
            &path, count, thumb_height, Some(start_time), Some(end_time),
        )?;
        Ok(thumbnails
            .into_iter()
            .map(|data| {
                let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
                format!("data:image/bmp;base64,{}", encoded)
            })
            .collect())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}
