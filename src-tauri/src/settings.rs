use serde::{Deserialize, Serialize};

/// Mirrors the macOS app's Constants.swift defaults
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub output_quality: f64,
    pub output_speed: f64,
    pub output_fps: u32,
    pub loop_gif: bool,
    pub bounce_gif: bool,
    pub loop_count: u32,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            output_quality: 1.0,
            output_speed: 1.0,
            output_fps: 10,
            loop_gif: true,
            bounce_gif: false,
            loop_count: 0,
        }
    }
}

/// Allowed ranges matching Constants.swift
#[allow(dead_code)]
pub const FPS_MIN: u32 = 3;
#[allow(dead_code)]
pub const FPS_MAX: u32 = 50;
#[allow(dead_code)]
pub const SPEED_MIN: f64 = 0.5;
#[allow(dead_code)]
pub const SPEED_MAX: f64 = 5.0;
#[allow(dead_code)]
pub const SPEED_STEP: f64 = 0.25;
#[allow(dead_code)]
pub const QUALITY_MIN: f64 = 0.01;
#[allow(dead_code)]
pub const QUALITY_MAX: f64 = 1.0;
#[allow(dead_code)]
pub const LOOP_COUNT_MAX: u32 = 100;
