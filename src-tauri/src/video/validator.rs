use super::ffmpeg::{self, VideoMetadata};

/// Validation result
#[derive(Debug)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub error: Option<String>,
    pub metadata: Option<VideoMetadata>,
}

/// Validate a video file for GIF conversion.
/// Port of VideoValidator.swift
pub fn validate_video(path: &str) -> ValidationResult {
    // Check file exists
    if !std::path::Path::new(path).exists() {
        return ValidationResult {
            is_valid: false,
            error: Some("File does not exist.".to_string()),
            metadata: None,
        };
    }

    // Check file size > 0
    match std::fs::metadata(path) {
        Ok(meta) => {
            if meta.len() == 0 {
                return ValidationResult {
                    is_valid: false,
                    error: Some("File is empty.".to_string()),
                    metadata: None,
                };
            }
        }
        Err(e) => {
            return ValidationResult {
                is_valid: false,
                error: Some(format!("Cannot read file: {}", e)),
                metadata: None,
            };
        }
    }

    // Get metadata via ffprobe
    let metadata = match ffmpeg::get_metadata(path) {
        Ok(m) => m,
        Err(e) => {
            return ValidationResult {
                is_valid: false,
                error: Some(format!("Cannot read video metadata: {}", e)),
                metadata: None,
            };
        }
    };

    // Check dimensions (minimum 4x4, matching macOS app)
    if metadata.width < 4 || metadata.height < 4 {
        return ValidationResult {
            is_valid: false,
            error: Some(format!(
                "Video dimensions too small: {}x{}. Minimum is 4x4.",
                metadata.width, metadata.height
            )),
            metadata: Some(metadata),
        };
    }

    // Check duration
    if metadata.duration <= 0.0 {
        return ValidationResult {
            is_valid: false,
            error: Some("Video has no duration or could not be read.".to_string()),
            metadata: Some(metadata),
        };
    }

    // Check we have at least enough for 2 frames
    let min_frame_duration = 2.0 / metadata.frame_rate.max(1.0);
    if metadata.duration < min_frame_duration {
        return ValidationResult {
            is_valid: false,
            error: Some(
                "Video is too short. An animated GIF requires at least 2 frames.".to_string(),
            ),
            metadata: Some(metadata),
        };
    }

    ValidationResult {
        is_valid: true,
        error: None,
        metadata: Some(metadata),
    }
}
