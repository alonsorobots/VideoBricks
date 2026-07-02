use crate::video::ffmpeg::{self, CropRect, FrameData};
use gifski::collector::RGBA8;
use gifski::{self, Repeat, Settings};
use imgref::ImgVec;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;

/// A single trim segment with in/out points
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrimSegment {
    pub start: f64,
    pub end: f64,
}

/// Conversion settings passed from the frontend
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionSettings {
    pub source_path: String,
    pub quality: f64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: u32,
    pub speed: f64,
    pub loop_forever: bool,
    pub loop_count: u32,
    pub bounce: bool,
    pub segments: Vec<TrimSegment>,
    pub segment_mode: String,  // "merge" or "split"
    pub crop: Option<CropRect>,
}

/// Progress callback info
pub struct ConversionProgress {
    pub progress: f64,
    pub total_frames: u32,
    pub stage: &'static str,
}

/// Ensures progress only ever moves forward, even when multiple threads report.
struct ProgressTracker {
    max_permille: AtomicU32,
    total_frames: u32,
}

impl ProgressTracker {
    fn new(total_frames: u32) -> Self {
        Self {
            max_permille: AtomicU32::new(0),
            total_frames: total_frames.max(1),
        }
    }

    fn set_total_frames(&mut self, total_frames: u32) {
        self.total_frames = total_frames.max(1);
    }

    fn report(&self, stage: &'static str, progress: f64, on_progress: &dyn Fn(ConversionProgress)) {
        let permille = (progress.clamp(0.0, 1.0) * 1000.0).round() as u32;
        let mut current = self.max_permille.load(Ordering::Relaxed);
        while permille > current {
            match self.max_permille.compare_exchange_weak(
                current,
                permille,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    let p = permille as f64 / 1000.0;
                    on_progress(ConversionProgress {
                        progress: p,
                        total_frames: self.total_frames,
                        stage,
                    });
                    return;
                }
                Err(v) => current = v,
            }
        }
    }
}

fn estimate_total_frames(settings: &ConversionSettings) -> u32 {
    settings
        .segments
        .iter()
        .map(|seg| {
            let duration = (seg.end - seg.start) / settings.speed;
            let source_frames = (duration * settings.fps as f64).ceil() as u32;
            if settings.bounce && source_frames > 1 {
                source_frames * 2 - 1
            } else {
                source_frames
            }
        })
        .sum()
}

// FFmpeg frame extraction is usually fast; gifski encoding dominates runtime.
const EXTRACTION_WEIGHT: f64 = 0.10;

/// Map per-stage progress (0-1) to an overall 0-1 value.
fn overall_progress(stage: &'static str, stage_progress: f64) -> f64 {
    let stage_progress = stage_progress.clamp(0.0, 1.0);
    match stage {
        "extracting" => stage_progress * EXTRACTION_WEIGHT,
        "encoding" => EXTRACTION_WEIGHT + stage_progress * (1.0 - EXTRACTION_WEIGHT),
        _ => stage_progress,
    }
}

fn report_stage_progress(
    tracker: &ProgressTracker,
    on_progress: &dyn Fn(ConversionProgress),
    stage: &'static str,
    stage_progress: f64,
) {
    tracker.report(stage, overall_progress(stage, stage_progress), on_progress);
}

fn rgba_bytes_to_pixels(rgba: Vec<u8>) -> Vec<RGBA8> {
    debug_assert_eq!(rgba.len() % 4, 0);
    let len = rgba.len() / 4;
    let cap = rgba.capacity() / 4;
    let ptr = rgba.as_ptr() as *mut RGBA8;
    std::mem::forget(rgba);
    unsafe { Vec::from_raw_parts(ptr, len, cap) }
}

/// Convert a video to GIF using gifski.
/// Returns the GIF data as bytes.
pub fn convert_to_gif(
    settings: &ConversionSettings,
    cancelled: Arc<AtomicBool>,
    on_progress: impl Fn(ConversionProgress) + Send + Clone + 'static,
) -> Result<Vec<u8>, String> {
    let estimated_total = estimate_total_frames(settings).max(1);
    let mut tracker = ProgressTracker::new(estimated_total);
    report_stage_progress(&tracker, &on_progress, "extracting", 0.0);

    // 1. Extract frames from all segments and concatenate
    let mut frames: Vec<FrameData> = Vec::new();
    let segment_count = settings.segments.len().max(1);

    for (segment_index, segment) in settings.segments.iter().enumerate() {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Conversion cancelled".to_string());
        }

        report_stage_progress(
            &tracker,
            &on_progress,
            "extracting",
            segment_index as f64 / segment_count as f64,
        );

        let seg_frames = ffmpeg::extract_frames(
            &settings.source_path,
            settings.fps as f64,
            Some(segment.start),
            Some(segment.end),
            settings.speed,
            settings.crop,
            settings.width,
            settings.height,
        )?;

        // Re-timestamp frames so they follow sequentially after previous segments
        let time_offset = if frames.is_empty() {
            0.0
        } else {
            frames.last().unwrap().presentation_timestamp + (1.0 / settings.fps as f64)
        };

        for mut f in seg_frames {
            // Shift presentation_timestamp to be sequential
            let original_start = f.presentation_timestamp;
            f.presentation_timestamp = time_offset + (original_start
                - frames.first().map_or(original_start, |_| 0.0));
            frames.push(f);
        }

        report_stage_progress(
            &tracker,
            &on_progress,
            "extracting",
            (segment_index + 1) as f64 / segment_count as f64,
        );
    }

    report_stage_progress(&tracker, &on_progress, "extracting", 1.0);

    // Fix: re-assign presentation timestamps sequentially based on fps
    let frame_interval = 1.0 / settings.fps as f64;
    for (i, frame) in frames.iter_mut().enumerate() {
        frame.presentation_timestamp = i as f64 * frame_interval;
    }

    if frames.is_empty() {
        return Err("No frames extracted from video".to_string());
    }

    // Calculate total frame count (bounce doubles frames minus 1)
    let source_count = frames.len();
    let total_frames = if settings.bounce {
        source_count * 2 - 1
    } else {
        source_count
    };
    let total_frames_u32 = total_frames.max(1) as u32;
    tracker.set_total_frames(total_frames_u32);
    // Switch stage label; progress stays at the extraction ceiling (~10%).
    tracker.report("encoding", EXTRACTION_WEIGHT, &on_progress);

    let tracker = Arc::new(tracker);

    let repeat = if settings.loop_forever {
        Repeat::Infinite
    } else if settings.loop_count == 0 {
        Repeat::Finite(0) // play once
    } else {
        Repeat::Finite(settings.loop_count as u16)
    };

    let quality = (settings.quality * 100.0).round().clamp(1.0, 100.0) as u8;

    let gif_settings = Settings {
        width: settings.width,
        height: settings.height,
        quality,
        fast: false,
        repeat,
    };

    let (collector, writer) =
        gifski::new(gif_settings).map_err(|e| format!("Failed to create gifski encoder: {}", e))?;

    // Set up the output buffer
    let gif_data = Arc::new(std::sync::Mutex::new(Vec::new()));
    let gif_data_clone = gif_data.clone();

    let completed = Arc::new(AtomicU32::new(0));
    let completed_clone = completed.clone();
    let cancelled_clone = cancelled.clone();

    // Spawn writer thread
    let progress_for_writer = on_progress.clone();
    let tracker_for_writer = tracker.clone();
    let writer_thread = thread::spawn(move || {
        let mut buf = GifBuffer {
            data: gif_data_clone,
        };
        let mut reporter = ProgressReporter {
            completed: completed_clone,
            total: total_frames_u32,
            cancelled: cancelled_clone,
            tracker: tracker_for_writer,
            on_progress: Box::new(move |p| progress_for_writer(p)),
        };
        writer
            .write(&mut buf, &mut reporter)
            .map_err(|e| format!("GIF write failed: {}", e))
    });

    // Add frames to the collector (encoding progress is reported by the writer thread)
    let add_result = add_frames_to_collector(
        &collector,
        &frames,
        settings.bounce,
        total_frames,
        &cancelled,
    );

    // Drop the collector to signal we're done adding frames
    drop(collector);

    // Wait for writer to finish
    let write_result = writer_thread
        .join()
        .map_err(|_| "Writer thread panicked".to_string())?;

    // Check for errors
    add_result?;
    write_result?;

    // Return the GIF data
    let data = gif_data
        .lock()
        .map_err(|_| "Failed to get GIF data")?;
    Ok(data.clone())
}

/// Convert each segment to a separate GIF. Returns one Vec<u8> per segment.
/// Progress is reported as a single continuous bar across all segments.
pub fn convert_to_gifs_split(
    settings: &ConversionSettings,
    cancelled: Arc<AtomicBool>,
    on_progress: impl Fn(ConversionProgress) + Send + Clone + 'static,
) -> Result<Vec<Vec<u8>>, String> {
    let mut results = Vec::new();

    let estimated_total = estimate_total_frames(settings).max(1);

    // Shared offset: how many frames have been completed in prior segments
    let frame_offset = Arc::new(AtomicU32::new(0));

    for segment in &settings.segments {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Conversion cancelled".to_string());
        }

        let mut seg_settings = settings.clone();
        seg_settings.segments = vec![segment.clone()];

        let offset = frame_offset.clone();
        let total = estimated_total;
        let progress_cb = on_progress.clone();
        let current_offset = offset.load(Ordering::Relaxed);
        let seg_duration = (segment.end - segment.start) / settings.speed;
        let seg_frames = (seg_duration * settings.fps as f64).ceil() as u32;
        let seg_total = if settings.bounce && seg_frames > 1 {
            seg_frames * 2 - 1
        } else {
            seg_frames
        };

        let gif_data = convert_to_gif(
            &seg_settings,
            cancelled.clone(),
            move |p: ConversionProgress| {
                let global = (current_offset as f64 + p.progress * seg_total as f64)
                    / total.max(1) as f64;
                progress_cb(ConversionProgress {
                    progress: global,
                    total_frames: total,
                    stage: p.stage,
                });
            },
        )?;

        frame_offset.fetch_add(seg_total, Ordering::Relaxed);

        results.push(gif_data);
    }

    Ok(results)
}

fn add_frames_to_collector(
    collector: &gifski::Collector,
    frames: &[FrameData],
    bounce: bool,
    total_frames: usize,
    cancelled: &Arc<AtomicBool>,
) -> Result<(), String> {
    let source_count = frames.len();
    let fps = if frames.len() > 1 {
        1.0 / (frames[1].presentation_timestamp - frames[0].presentation_timestamp)
    } else {
        10.0
    };

    for (i, frame) in frames.iter().enumerate() {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Conversion cancelled".to_string());
        }

        let pixels = rgba_bytes_to_pixels(frame.rgba.clone());

        let img = ImgVec::new(pixels, frame.width as usize, frame.height as usize);

        collector
            .add_frame_rgba(i, img, frame.presentation_timestamp)
            .map_err(|e| format!("Failed to add frame {}: {}", i, e))?;

        // If bounce, add the reverse frame
        if bounce && i < source_count - 1 {
            let reverse_index = total_frames - i - 1;
            let reverse_timestamp = reverse_index as f64 / fps;

            let pixels2 = rgba_bytes_to_pixels(frame.rgba.clone());

            let img2 = ImgVec::new(pixels2, frame.width as usize, frame.height as usize);

            collector
                .add_frame_rgba(reverse_index, img2, reverse_timestamp)
                .map_err(|e| format!("Failed to add bounce frame {}: {}", reverse_index, e))?;
        }
    }

    Ok(())
}

/// Buffer that collects GIF output into a Vec<u8>
struct GifBuffer {
    data: Arc<std::sync::Mutex<Vec<u8>>>,
}

impl Write for GifBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut data = self
            .data
            .lock()
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "lock poisoned"))?;
        data.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Progress reporter for gifski
struct ProgressReporter {
    completed: Arc<AtomicU32>,
    total: u32,
    cancelled: Arc<AtomicBool>,
    tracker: Arc<ProgressTracker>,
    on_progress: Box<dyn Fn(ConversionProgress) + Send>,
}

impl gifski::progress::ProgressReporter for ProgressReporter {
    fn increase(&mut self) -> bool {
        if self.cancelled.load(Ordering::Relaxed) {
            return false; // abort
        }
        let completed = self.completed.fetch_add(1, Ordering::Relaxed) + 1;
        let stage_progress = completed as f64 / self.total.max(1) as f64;
        self.tracker.report(
            "encoding",
            overall_progress("encoding", stage_progress),
            self.on_progress.as_ref(),
        );
        true // continue
    }

    fn done(&mut self, _msg: &str) {}
}
