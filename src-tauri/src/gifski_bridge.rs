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

/// Frames FFmpeg is expected to produce (before the bounce doubling).
fn estimate_source_frames(settings: &ConversionSettings) -> u32 {
    settings
        .segments
        .iter()
        .map(|seg| {
            let duration = (seg.end - seg.start) / settings.speed;
            (duration * settings.fps as f64).ceil() as u32
        })
        .sum()
}

fn estimate_total_frames(settings: &ConversionSettings) -> u32 {
    let source_frames = estimate_source_frames(settings);
    if settings.bounce && source_frames > 1 {
        source_frames * 2 - 1
    } else {
        source_frames
    }
}

// FFmpeg frame extraction is usually fast; gifski encoding dominates runtime.
const EXTRACTION_WEIGHT: f64 = 0.10;

/// Guard against absurd frame budgets. A 10-minute 4K source left at its
/// default settings (output = source resolution, segment = whole video) asks for
/// tens of thousands of full-resolution frames; even streamed, that is hours of
/// work and a GIF nobody wants. Refuse early with an actionable message rather
/// than appearing to hang.
const MAX_OUTPUT_FRAMES: u32 = 20_000;

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
    let estimated_source = estimate_source_frames(settings).max(1);
    let estimated_total = estimate_total_frames(settings).max(1);

    if estimated_total > MAX_OUTPUT_FRAMES {
        return Err(format!(
            "That would produce {} frames, which is too many to encode as a GIF. \
             Trim the clip, lower the frame rate, or reduce the output size.",
            estimated_total
        ));
    }

    let mut tracker = ProgressTracker::new(estimated_total);
    tracker.set_total_frames(estimated_total);
    report_stage_progress(&tracker, &on_progress, "extracting", 0.0);
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

    // The encoding denominator starts as an estimate and is corrected once
    // extraction has told us the true frame count (extraction and encoding run
    // concurrently now, so we cannot know it up front).
    let total_frames_atomic = Arc::new(AtomicU32::new(estimated_total));

    // Spawn writer thread
    let progress_for_writer = on_progress.clone();
    let tracker_for_writer = tracker.clone();
    let total_for_writer = total_frames_atomic.clone();
    let writer_thread = thread::spawn(move || {
        let mut buf = GifBuffer {
            data: gif_data_clone,
        };
        let mut reporter = ProgressReporter {
            completed: completed_clone,
            total: total_for_writer,
            cancelled: cancelled_clone,
            tracker: tracker_for_writer,
            on_progress: Box::new(move |p| progress_for_writer(p)),
        };
        writer
            .write(&mut buf, &mut reporter)
            .map_err(|e| format!("GIF write failed: {}", e))
    });

    // Stream frames from FFmpeg straight into the collector. Encoding progress
    // is reported by the writer thread; extraction progress is reported here.
    let add_result = stream_frames_to_collector(
        &collector,
        settings,
        estimated_source,
        &tracker,
        &on_progress,
        &total_frames_atomic,
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

/// Pull frames out of FFmpeg one at a time and hand them straight to gifski.
///
/// Nothing is accumulated: peak memory is a single frame plus whatever gifski's
/// own bounded queue holds, instead of the entire decoded sequence.
///
/// Bounce needs every frame a second time in reverse order. Rather than keeping
/// them all in RAM, the forward pass mirrors each frame to a scratch file and
/// the reverse pass reads it back from the end -- which happens to yield
/// ascending gifski indices, so the encoder never has to buffer either.
fn stream_frames_to_collector(
    collector: &gifski::Collector,
    settings: &ConversionSettings,
    estimated_source: u32,
    tracker: &Arc<ProgressTracker>,
    on_progress: &(impl Fn(ConversionProgress) + Send + Clone + 'static),
    total_frames_atomic: &Arc<AtomicU32>,
    cancelled: &Arc<AtomicBool>,
) -> Result<(), String> {
    let frame_interval = 1.0 / settings.fps as f64;

    // Scratch store for the bounce pass, only created when bounce is on.
    let mut scratch: Option<FrameScratch> = if settings.bounce {
        Some(FrameScratch::create()?)
    } else {
        None
    };

    let mut emitted: usize = 0;
    let mut dims: Option<(u32, u32)> = None;
    let mut add_error: Option<String> = None;

    for segment in settings.segments.iter() {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Conversion cancelled".to_string());
        }

        let mut on_frame = |frame: FrameData| -> bool {
            if cancelled.load(Ordering::Relaxed) {
                return false;
            }

            dims = Some((frame.width, frame.height));

            if let Some(s) = scratch.as_mut() {
                if let Err(e) = s.push(&frame.rgba) {
                    add_error = Some(e);
                    return false;
                }
            }

            let index = emitted;
            let pts = index as f64 * frame_interval;
            let pixels = rgba_bytes_to_pixels(frame.rgba);
            let img = ImgVec::new(pixels, frame.width as usize, frame.height as usize);

            if let Err(e) = collector.add_frame_rgba(index, img, pts) {
                add_error = Some(format!("Failed to add frame {}: {}", index, e));
                return false;
            }

            emitted += 1;

            // Genuine per-frame extraction progress, instead of one step per segment.
            report_stage_progress(
                tracker,
                on_progress,
                "extracting",
                emitted as f64 / estimated_source.max(1) as f64,
            );
            true
        };

        let extraction = ffmpeg::extract_frames_streaming(
            &settings.source_path,
            settings.fps as f64,
            Some(segment.start),
            Some(segment.end),
            settings.speed,
            settings.crop,
            settings.width,
            settings.height,
            &mut on_frame,
        );

        // Cancelling kills the FFmpeg child, which surfaces here as a generic
        // spawn/exit failure. Report it as a cancellation so the UI returns to
        // the edit screen instead of showing an error banner.
        if cancelled.load(Ordering::Relaxed) {
            return Err("Conversion cancelled".to_string());
        }
        extraction?;

        if let Some(e) = add_error.take() {
            return Err(e);
        }
    }

    if emitted == 0 {
        return Err("No frames extracted from video".to_string());
    }

    let source_count = emitted;
    let total_frames = if settings.bounce && source_count > 1 {
        source_count * 2 - 1
    } else {
        source_count
    };
    // Correct the encoding denominator now that the real count is known.
    total_frames_atomic.store(total_frames.max(1) as u32, Ordering::Relaxed);

    report_stage_progress(tracker, on_progress, "extracting", 1.0);
    tracker.report("encoding", EXTRACTION_WEIGHT, on_progress);

    // Bounce: replay the scratch file backwards. Output index `total - 1 - i`
    // for source frame i, walked so indices ascend from source_count upward.
    if let (Some(s), Some((w, h))) = (scratch.as_mut(), dims) {
        if source_count > 1 {
            for i in (0..source_count - 1).rev() {
                if cancelled.load(Ordering::Relaxed) {
                    return Err("Conversion cancelled".to_string());
                }
                let reverse_index = total_frames - 1 - i;
                let rgba = s.read(i)?;
                let pixels = rgba_bytes_to_pixels(rgba);
                let img = ImgVec::new(pixels, w as usize, h as usize);
                collector
                    .add_frame_rgba(reverse_index, img, reverse_index as f64 * frame_interval)
                    .map_err(|e| {
                        format!("Failed to add bounce frame {}: {}", reverse_index, e)
                    })?;
            }
        }
    }

    Ok(())
}

/// Disk-backed store of raw RGBA frames, used only for the bounce pass.
/// Keeps the reverse replay off the heap.
struct FrameScratch {
    file: std::fs::File,
    _dir: tempfile::TempDir,
    frame_size: usize,
    count: usize,
}

impl FrameScratch {
    fn create() -> Result<Self, String> {
        let dir = tempfile::Builder::new()
            .prefix("videobricks-bounce-")
            .tempdir()
            .map_err(|e| format!("Failed to create scratch dir: {}", e))?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(true)
            .open(dir.path().join("frames.rgba"))
            .map_err(|e| format!("Failed to create scratch file: {}", e))?;
        Ok(Self {
            file,
            _dir: dir,
            frame_size: 0,
            count: 0,
        })
    }

    fn push(&mut self, rgba: &[u8]) -> Result<(), String> {
        use std::io::Write;
        if self.frame_size == 0 {
            self.frame_size = rgba.len();
        }
        self.file
            .write_all(rgba)
            .map_err(|e| format!("Failed to buffer bounce frame: {}", e))?;
        self.count += 1;
        Ok(())
    }

    fn read(&mut self, index: usize) -> Result<Vec<u8>, String> {
        use std::io::{Read, Seek, SeekFrom};
        if index >= self.count || self.frame_size == 0 {
            return Err("Bounce frame index out of range".to_string());
        }
        self.file
            .seek(SeekFrom::Start((index * self.frame_size) as u64))
            .map_err(|e| format!("Failed to seek bounce scratch: {}", e))?;
        let mut buf = vec![0u8; self.frame_size];
        self.file
            .read_exact(&mut buf)
            .map_err(|e| format!("Failed to read bounce frame: {}", e))?;
        Ok(buf)
    }
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
    total: Arc<AtomicU32>,
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
        let total = self.total.load(Ordering::Relaxed);
        let stage_progress = completed as f64 / total.max(1) as f64;
        self.tracker.report(
            "encoding",
            overall_progress("encoding", stage_progress),
            self.on_progress.as_ref(),
        );
        true // continue
    }

    fn done(&mut self, _msg: &str) {}
}

// ---------------------------------------------------------------------------
// Fast size estimation
// ---------------------------------------------------------------------------

/// Number of frames actually encoded when estimating output size.
const ESTIMATE_SAMPLE_FRAMES: usize = 12;
/// Longest edge used for the sample encode. Bytes-per-pixel is roughly stable
/// across scale for a given source, so a small sample extrapolates well and
/// costs milliseconds instead of minutes.
const ESTIMATE_SAMPLE_MAX_EDGE: u32 = 240;

/// Estimate the size of the GIF these settings would produce.
///
/// The previous implementation ran a **complete** conversion at reduced fps and
/// scaled the byte count up. At the app's defaults that meant decoding the whole
/// video at full resolution -- the single most expensive thing the edit screen
/// did, fired automatically 1.5s after the screen opened, and the main reason
/// large videos made the app unusable.
///
/// Instead: encode a handful of evenly-spaced frames at a small size, measure
/// bytes per frame per pixel, and extrapolate to the real frame count and
/// resolution. Runs in well under a second on a 4K source.
pub fn estimate_gif_size(settings: &ConversionSettings) -> Result<u64, String> {
    let total_frames = estimate_total_frames(settings).max(1);

    let (out_w, out_h) = target_dimensions(settings)?;

    let first = settings.segments.first().ok_or("No segments")?;
    let seg_len = (first.end - first.start).max(0.001);
    if seg_len <= 0.0 {
        return Ok(0);
    }

    // Sample a *short window* at the real frame rate -- not the whole segment at
    // a low one. Keeping the target fps preserves how similar consecutive frames
    // are, which is what drives GIF inter-frame compression, and the window stays
    // small enough that FFmpeg seeks straight to it instead of decoding the file.
    let speed = settings.speed.max(0.01);
    let window_src = ((ESTIMATE_SAMPLE_FRAMES as f64 / settings.fps.max(1) as f64) * speed)
        .min(seg_len);
    // Take it from the middle of the selection, where content is representative.
    let sample_start = (first.start + (seg_len - window_src) / 2.0).max(first.start);
    let sample_end = (sample_start + window_src).min(first.end);

    // Scale the sample down but keep the aspect ratio, so bytes/pixel transfers.
    let scale = (ESTIMATE_SAMPLE_MAX_EDGE as f64 / out_w.max(out_h) as f64).min(1.0);
    let sample_w = ((out_w as f64 * scale).round() as u32).max(2) & !1;
    let sample_h = ((out_h as f64 * scale).round() as u32).max(2) & !1;

    let mut sample = settings.clone();
    sample.segments = vec![TrimSegment {
        start: sample_start,
        end: sample_end,
    }];
    sample.width = Some(sample_w);
    sample.height = Some(sample_h);
    sample.bounce = false;
    sample.segment_mode = "merge".to_string();

    let cancelled = Arc::new(AtomicBool::new(false));
    let data = convert_to_gif(&sample, cancelled, |_| {})?;

    let sample_frames = (((sample_end - sample_start) / speed) * sample.fps as f64)
        .ceil()
        .max(1.0);
    let sample_px = (sample_w as f64) * (sample_h as f64);
    // GIF carries a ~800 byte header/palette overhead that should not be scaled.
    let per_frame = ((data.len() as f64 - 800.0).max(0.0)) / sample_frames;
    let bytes_per_px = per_frame / sample_px;

    let full_px = (out_w as f64) * (out_h as f64);
    let estimated = 800.0 + bytes_per_px * full_px * total_frames as f64;

    Ok(estimated.max(0.0) as u64)
}

/// Resolve the output pixel dimensions these settings imply.
fn target_dimensions(settings: &ConversionSettings) -> Result<(u32, u32), String> {
    if let (Some(w), Some(h)) = (settings.width, settings.height) {
        return Ok((w.max(2), h.max(2)));
    }
    let meta = ffmpeg::get_metadata(&settings.source_path)?;
    let (mut w, mut h) = (meta.width, meta.height);
    if let Some(c) = settings.crop {
        w = ((c.width * meta.width as f64).round() as u32).max(2);
        h = ((c.height * meta.height as f64).round() as u32).max(2);
    }
    Ok((settings.width.unwrap_or(w), settings.height.unwrap_or(h)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Set VIDEOBRICKS_TEST_VIDEO to a real file to run these.
    fn test_video() -> Option<String> {
        std::env::var("VIDEOBRICKS_TEST_VIDEO").ok().filter(|p| std::path::Path::new(p).is_file())
    }

    fn base_settings(path: &str) -> ConversionSettings {
        ConversionSettings {
            source_path: path.to_string(),
            quality: 0.8,
            width: Some(320),
            height: Some(180),
            fps: 10,
            speed: 1.0,
            loop_forever: true,
            loop_count: 0,
            bounce: false,
            segments: vec![TrimSegment { start: 5.0, end: 8.0 }],
            segment_mode: "merge".to_string(),
            crop: None,
        }
    }

    fn is_gif(data: &[u8]) -> bool {
        data.len() > 6 && (&data[..6] == b"GIF89a" || &data[..6] == b"GIF87a")
    }

    #[test]
    fn streams_a_valid_gif() {
        let Some(path) = test_video() else { return };
        let s = base_settings(&path);
        let data = convert_to_gif(&s, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        assert!(is_gif(&data), "not a GIF: {:?}", &data[..8.min(data.len())]);
        assert!(data.len() > 1000, "suspiciously small GIF: {} bytes", data.len());
    }

    /// Bounce replays frames from the disk scratch file; the result must still be
    /// a valid GIF and meaningfully larger than the one-way version.
    #[test]
    fn bounce_round_trips_through_scratch() {
        let Some(path) = test_video() else { return };
        let mut s = base_settings(&path);
        let forward = convert_to_gif(&s, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        s.bounce = true;
        let bounced = convert_to_gif(&s, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        assert!(is_gif(&bounced));
        assert!(
            bounced.len() > forward.len(),
            "bounce ({}) should hold more frames than forward ({})",
            bounced.len(),
            forward.len()
        );
    }

    /// Progress must be monotonic and finish at 1.0.
    #[test]
    fn progress_is_monotonic_and_completes() {
        let Some(path) = test_video() else { return };
        let s = base_settings(&path);
        let seen = Arc::new(std::sync::Mutex::new(Vec::<f64>::new()));
        let sink = seen.clone();
        convert_to_gif(&s, Arc::new(AtomicBool::new(false)), move |p| {
            sink.lock().unwrap().push(p.progress);
        })
        .unwrap();
        let v = seen.lock().unwrap().clone();
        assert!(!v.is_empty(), "no progress reported");
        for w in v.windows(2) {
            assert!(w[1] >= w[0], "progress went backwards: {} -> {}", w[0], w[1]);
        }
        assert!(*v.last().unwrap() > 0.99, "progress ended at {}", v.last().unwrap());
    }

    /// The sample-based estimate should land in the right ballpark of a real
    /// encode -- this is the property that matters, not exactness.
    #[test]
    fn estimate_is_within_an_order_of_magnitude() {
        let Some(path) = test_video() else { return };
        let s = base_settings(&path);
        let actual = convert_to_gif(&s, Arc::new(AtomicBool::new(false)), |_| {})
            .unwrap()
            .len() as f64;
        let est = estimate_gif_size(&s).unwrap() as f64;
        let ratio = est / actual;
        assert!(
            ratio > 0.2 && ratio < 5.0,
            "estimate {} vs actual {} (ratio {:.2})",
            est, actual, ratio
        );
    }

    /// The edit screen fires an estimate at the app's *default* settings:
    /// whole video, output = source resolution. Run with --nocapture to see it.
    #[test]
    fn estimate_at_defaults_is_fast() {
        let Some(path) = test_video() else { return };
        let meta = ffmpeg::get_metadata(&path).unwrap();
        let mut s = base_settings(&path);
        s.width = Some(meta.width);
        s.height = Some(meta.height);
        s.fps = 10;
        s.segments = vec![TrimSegment { start: 0.0, end: meta.duration }];

        let old_rgba_bytes =
            meta.width as u64 * meta.height as u64 * 4 * (meta.duration * 5.0) as u64;

        let t = std::time::Instant::now();
        let est = estimate_gif_size(&s).unwrap();
        let ms = t.elapsed().as_millis();
        println!(
            "
  estimate at defaults ({}x{}, {:.0}s): {} ms -> ~{:.1} MB predicted
               old estimator would first have buffered {:.1} GiB of raw RGBA
",
            meta.width, meta.height, meta.duration, ms,
            est as f64 / 1e6,
            old_rgba_bytes as f64 / 1024.0 / 1024.0 / 1024.0,
        );
        assert!(ms < 10_000, "estimate took {} ms", ms);
    }

    /// Big-job smoke test used to measure peak process memory from outside.
    /// Encodes a long stretch at full source resolution -- the shape of job that
    /// used to exhaust RAM. Run with VIDEOBRICKS_MEMORY_TEST=1.
    #[test]
    fn memory_smoke_full_resolution() {
        if std::env::var("VIDEOBRICKS_MEMORY_TEST").is_err() {
            return;
        }
        let Some(path) = test_video() else { return };
        let meta = ffmpeg::get_metadata(&path).unwrap();
        let mut s = base_settings(&path);
        s.width = Some(meta.width);
        s.height = Some(meta.height);
        s.fps = 10;
        s.segments = vec![TrimSegment { start: 0.0, end: 60.0 }];

        let frames = 60 * 10;
        let unstreamed = meta.width as u64 * meta.height as u64 * 4 * frames;
        println!(
            "
  job: {}x{}, {} frames at full resolution
               buffering every frame would need {:.1} GiB
",
            meta.width, meta.height, frames,
            unstreamed as f64 / 1024.0 / 1024.0 / 1024.0
        );
        let data = convert_to_gif(&s, Arc::new(AtomicBool::new(false)), |_| {}).unwrap();
        println!("  produced GIF: {:.1} MB
", data.len() as f64 / 1e6);
    }

    #[test]
    fn refuses_absurd_frame_counts() {
        let mut s = base_settings("nonexistent.mp4");
        s.segments = vec![TrimSegment { start: 0.0, end: 6000.0 }];
        s.fps = 50;
        let err = convert_to_gif(&s, Arc::new(AtomicBool::new(false)), |_| {}).unwrap_err();
        assert!(err.contains("too many"), "unexpected error: {}", err);
    }
}
