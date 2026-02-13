// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod gifski_bridge;
mod settings;
#[cfg(windows)]
mod taskbar;
mod video;

use std::sync::Mutex;
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .manage(Mutex::new(commands::convert::ConversionState::default()))
        .setup(|app| {
            #[cfg(windows)]
            {
                let window = app.get_webview_window("main").unwrap();
                taskbar::init_taskbar(&window);
            }
            Ok(())
        })
        .on_window_event(|_window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                // Kill all active FFmpeg/child processes so nothing is left behind
                video::ffmpeg::kill_all_children();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::video::validate_video,
            commands::video::get_video_metadata,
            commands::video::get_video_thumbnails,
            commands::video::get_video_thumbnails_range,
            commands::video::find_shots,
            commands::convert::convert_to_gif,
            commands::convert::cancel_conversion,
            commands::convert::estimate_file_size,
            commands::convert::generate_preview_gif,
            commands::settings::get_settings,
            commands::settings::set_settings,
            commands::export::save_gif_file,
            commands::export::save_all_gif_files,
            commands::export::copy_gif_to_clipboard,
            commands::export::convert_to_mp4,
            commands::export::reveal_in_explorer,
            commands::export::export_video,
            commands::export::get_temp_dir,
            commands::export::copy_file,
            commands::export::copy_files_to_directory,
        ])
        .run(tauri::generate_context!())
        .expect("error while running VideoBricks");
}
