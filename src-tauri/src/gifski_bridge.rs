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
    pub completed_frames: u32,
    pub total_frames: u32,
}

/// Convert a video to GIF using gifski.
/// Returns the GIF data as bytes.
pub fn convert_to_gif(
    settings: &ConversionSettings,
    cancelled: Arc<AtomicBool>,
    on_progress: impl Fn(ConversionProgress) + Send + 'static,
) -> Result<Vec<u8>, String> {
    // 1. Extract frames from all segments and concatenate
    let mut frames: Vec<FrameData> = Vec::new();

    for segment in &settings.segments {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Conversion cancelled".to_string());
        }

        let seg_frames = ffmpeg::extract_frames(
            &settings.source_path,
            settings.fps as f64,
            Some(segment.start),
            Some(segment.end),
            settings.speed,
            settings.crop,
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
    }

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
    let writer_thread = thread::spawn(move || {
        let mut buf = GifBuffer {
            data: gif_data_clone,
        };
        let mut reporter = ProgressReporter {
            completed: completed_clone,
            total: total_frames as u32,
            cancelled: cancelled_clone,
            on_progress: Box::new(on_progress),
        };
        writer
            .write(&mut buf, &mut reporter)
            .map_err(|e| format!("GIF write failed: {}", e))
    });

    // Add frames to the collector
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

    // Pre-estimate total frames across all segments so we can report unified progress.
    // Each segment's frame count = ceil((end - start) / speed * fps), with bounce doubling.
    let estimated_total: u32 = settings.segments.iter().map(|seg| {
        let duration = (seg.end - seg.start) / settings.speed;
        let source_frames = (duration * settings.fps as f64).ceil() as u32;
        if settings.bounce && source_frames > 1 {
            source_frames * 2 - 1
        } else {
            source_frames
        }
    }).sum();

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

        let gif_data = convert_to_gif(
            &seg_settings,
            cancelled.clone(),
            move |p: ConversionProgress| {
                // Report with global offset so progress is continuous
                progress_cb(ConversionProgress {
                    completed_frames: current_offset + p.completed_frames,
                    total_frames: total,
                });
            },
        )?;

        // After this segment completes, advance the offset by the segment's total frames
        let seg_duration = (segment.end - segment.start) / settings.speed;
        let seg_frames = (seg_duration * settings.fps as f64).ceil() as u32;
        let seg_total = if settings.bounce && seg_frames > 1 {
            seg_frames * 2 - 1
        } else {
            seg_frames
        };
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

        let pixels: Vec<RGBA8> = frame
            .rgba
            .chunks_exact(4)
            .map(|c| RGBA8::new(c[0], c[1], c[2], c[3]))
            .collect();

        let img = ImgVec::new(pixels, frame.width as usize, frame.height as usize);

        collector
            .add_frame_rgba(i, img, frame.presentation_timestamp)
            .map_err(|e| format!("Failed to add frame {}: {}", i, e))?;

        // If bounce, add the reverse frame
        if bounce && i < source_count - 1 {
            let reverse_index = total_frames - i - 1;
            let reverse_timestamp = reverse_index as f64 / fps;

            let pixels2: Vec<RGBA8> = frame
                .rgba
                .chunks_exact(4)
                .map(|c| RGBA8::new(c[0], c[1], c[2], c[3]))
                .collect();

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
    on_progress: Box<dyn Fn(ConversionProgress) + Send>,
}

impl gifski::progress::ProgressReporter for ProgressReporter {
    fn increase(&mut self) -> bool {
        if self.cancelled.load(Ordering::Relaxed) {
            return false; // abort
        }
        let completed = self.completed.fetch_add(1, Ordering::Relaxed) + 1;
        (self.on_progress)(ConversionProgress {
            completed_frames: completed,
            total_frames: self.total,
        });
        true // continue
    }

    fn done(&mut self, _msg: &str) {}
}
