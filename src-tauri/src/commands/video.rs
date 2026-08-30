use crate::video::{ffmpeg, validator};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use tauri::path::BaseDirectory;
use tauri::{Emitter, Manager};

const TRANSNET_SCRIPT: &str = include_str!("../../scripts/transnet_detect.py");

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

/// A single thumbnail as it becomes available, streamed to the UI.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThumbnailProgress {
    /// Identifies the request, so the UI can ignore results from a superseded one.
    request_id: u64,
    /// Position of this thumbnail in the strip.
    index: usize,
    /// How many thumbnails this request will produce in total.
    total: usize,
    data_url: String,
    /// True for the range (detail-trim) strip, false for the main timeline.
    detail: bool,
}

fn encode_bmp_data_url(data: &[u8]) -> String {
    use base64::Engine;
    format!(
        "data:image/bmp;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(data)
    )
}

/// Shared implementation for the two thumbnail commands.
///
/// Each thumbnail is emitted as a `thumbnail-ready` event the moment its FFmpeg
/// process finishes, so the timeline fills in progressively rather than staying
/// empty until the whole strip is done. The full ordered list is still returned
/// for callers that would rather just await it.
async fn thumbnails_impl(
    app: tauri::AppHandle,
    path: String,
    count: usize,
    thumb_height: u32,
    start_time: Option<f64>,
    end_time: Option<f64>,
    request_id: u64,
    detail: bool,
) -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(move || {
        let total = count.min(ffmpeg::MAX_THUMBNAILS);
        let mut slots: Vec<Option<String>> = vec![None; total];

        ffmpeg::generate_thumbnails_streaming(
            &path,
            count,
            thumb_height,
            start_time,
            end_time,
            &mut |index, data| {
                let data_url = encode_bmp_data_url(data);
                let _ = app.emit(
                    "thumbnail-ready",
                    ThumbnailProgress {
                        request_id,
                        index,
                        total,
                        data_url: data_url.clone(),
                        detail,
                    },
                );
                if let Some(slot) = slots.get_mut(index) {
                    *slot = Some(data_url);
                }
                true
            },
        )?;

        // Keep one entry per slot, empty where a thumbnail failed, so indices
        // stay aligned with timeline positions. The UI renders empty slots as
        // placeholders rather than letting the strip shift.
        Ok(slots.into_iter().map(Option::unwrap_or_default).collect())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

/// Returns thumbnails as base64-encoded BMP images (full video duration)
#[tauri::command]
pub async fn get_video_thumbnails(
    app: tauri::AppHandle,
    path: String,
    count: usize,
    thumb_height: u32,
    request_id: Option<u64>,
) -> Result<Vec<String>, String> {
    thumbnails_impl(
        app,
        path,
        count,
        thumb_height,
        None,
        None,
        request_id.unwrap_or(0),
        false,
    )
    .await
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

fn first_existing_file(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find(|p| p.is_file())
}

/// Directories that may contain bundled TransNet files.
/// Packaged apps put resources in Tauri's resource dir (macOS: Contents/Resources,
/// Windows: next to the exe). `tauri dev` uses CARGO_MANIFEST_DIR.
fn transnet_search_dirs(app: &tauri::AppHandle) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Ok(p) = app.path().resource_dir() {
        dirs.push(p);
    }
    if let Ok(p) = app.path().resolve("scripts/transnet_detect.py", BaseDirectory::Resource) {
        if let Some(parent) = p.parent() {
            dirs.push(parent.to_path_buf());
            if let Some(grand) = parent.parent() {
                dirs.push(grand.to_path_buf());
            }
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            dirs.push(dir.to_path_buf());
            // macOS .app: executable is in Contents/MacOS, resources in Contents/Resources
            dirs.push(dir.join("../Resources"));
        }
    }
    dirs.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    dirs.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts"));

    dirs
}

fn find_named_resource(dirs: &[PathBuf], filename: &str) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    for dir in dirs {
        candidates.push(dir.join("scripts").join(filename));
        candidates.push(dir.join(filename));
    }
    first_existing_file(candidates)
}

fn materialize_embedded_script(app: &tauri::AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_cache_dir().ok()?;
    std::fs::create_dir_all(&dir).ok()?;
    let dest = dir.join("transnet_detect.py");
    let needs_write = std::fs::read_to_string(&dest)
        .map(|existing| existing != TRANSNET_SCRIPT)
        .unwrap_or(true);
    if needs_write {
        std::fs::write(&dest, TRANSNET_SCRIPT).ok()?;
    }
    dest.is_file().then_some(dest)
}

fn python_has_transnet_deps(program: &str, extra_args: &[&str]) -> bool {
    let mut cmd = ffmpeg::hidden_command(program);
    cmd.args(extra_args);
    cmd.args(["-c", "import onnxruntime, numpy, ffmpeg"]);
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

/// Returns (program, extra args to insert before `-u script.py ...`).
fn find_python_with_deps() -> Option<(String, Vec<String>)> {
    if python_has_transnet_deps("python3", &[]) {
        return Some(("python3".into(), Vec::new()));
    }
    if python_has_transnet_deps("python", &[]) {
        return Some(("python".into(), Vec::new()));
    }
    #[cfg(windows)]
    {
        if python_has_transnet_deps("py", &["-3"]) {
            return Some(("py".into(), vec!["-3".into()]));
        }
    }
    None
}

fn build_conda_python_cmd() -> Result<Command, String> {
    let conda_exe = ffmpeg::find_conda().map_err(|_| {
        "Shot detection needs Python with onnxruntime, numpy, and ffmpeg-python. Install them with: pip install onnxruntime numpy ffmpeg-python".to_string()
    })?;
    #[cfg(windows)]
    let default_env = "yt_filter";
    #[cfg(not(windows))]
    let default_env = "VideoBricks";
    let env_name = std::env::var("VIDEOBRICKS_CONDA_ENV").unwrap_or_else(|_| default_env.to_string());
    let mut c = ffmpeg::hidden_command(&conda_exe);
    c.args(["run", "-n", &env_name, "python"]);
    Ok(c)
}

fn resolve_transnet_paths(app: &tauri::AppHandle) -> Result<(PathBuf, Option<PathBuf>), String> {
    let dirs = transnet_search_dirs(app);
    let script_path = find_named_resource(&dirs, "transnet_detect.py")
        .or_else(|| materialize_embedded_script(app))
        .ok_or_else(|| {
            "transnet_detect.py not found in the app bundle. Try reinstalling VideoBricks.".to_string()
        })?;
    let onnx_model = find_named_resource(&dirs, "transnetv2.onnx")
        .or_else(|| {
            let beside_script = script_path.with_file_name("transnetv2.onnx");
            beside_script.is_file().then_some(beside_script)
        });
    Ok((script_path, onnx_model))
}

#[tauri::command]
pub async fn find_shots(
    app: tauri::AppHandle,
    path: String,
    threshold: f64,
) -> Result<Vec<SceneBoundary>, String> {
    tokio::task::spawn_blocking(move || {
        use std::io::BufRead;

        let (script_path, onnx_model) = resolve_transnet_paths(&app)?;
        let script_str = script_path
            .to_str()
            .ok_or_else(|| "transnet_detect.py path is not valid UTF-8".to_string())?
            .to_string();

        // Prefer a system Python that already has the ONNX deps. Fall back to
        // `conda run` for developer machines that keep packages in an env.
        let mut cmd = if let Some((program, extra)) = find_python_with_deps() {
            let mut c = ffmpeg::hidden_command(&program);
            c.args(&extra);
            c.arg("-u");
            c.arg(&script_str);
            c
        } else {
            let mut c = build_conda_python_cmd()?;
            c.arg("-u");
            c.arg(&script_str);
            c
        };
        cmd.arg(&path);
        cmd.args(["--threshold", &threshold.to_string()]);
        if let Some(onnx) = onnx_model.as_ref().and_then(|p| p.to_str()) {
            cmd.args(["--model", onnx]);
        }

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
    app: tauri::AppHandle,
    path: String,
    count: usize,
    thumb_height: u32,
    start_time: f64,
    end_time: f64,
    request_id: Option<u64>,
) -> Result<Vec<String>, String> {
    thumbnails_impl(
        app,
        path,
        count,
        thumb_height,
        Some(start_time),
        Some(end_time),
        request_id.unwrap_or(0),
        true,
    )
    .await
}
