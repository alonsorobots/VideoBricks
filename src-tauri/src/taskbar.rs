use tauri::WebviewWindow;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::{ITaskbarList3, TaskbarList, TBPF_NOPROGRESS, TBPF_NORMAL};

use std::sync::Mutex;

// Wrapper to make ITaskbarList3 Send + Sync (COM pointers are safe to pass between threads
// when properly initialized)
struct TaskbarWrapper(ITaskbarList3);
unsafe impl Send for TaskbarWrapper {}
unsafe impl Sync for TaskbarWrapper {}

static TASKBAR: Mutex<Option<TaskbarWrapper>> = Mutex::new(None);

/// Initialize the taskbar COM interface.
/// Must be called once on startup from the main thread.
pub fn init_taskbar(_window: &WebviewWindow) {
    let mut lock = TASKBAR.lock().unwrap();
    if lock.is_none() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            if let Ok(taskbar) = CoCreateInstance::<_, ITaskbarList3>(&TaskbarList, None, CLSCTX_ALL)
            {
                *lock = Some(TaskbarWrapper(taskbar));
            }
        }
    }
}

fn get_hwnd(window: &WebviewWindow) -> Option<HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = window.window_handle().ok()?;
    let raw = handle.as_raw();
    match raw {
        RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut _)),
        _ => None,
    }
}

/// Set taskbar progress (0.0 to 1.0)
pub fn set_progress(window: &WebviewWindow, progress: f64) {
    if let Ok(lock) = TASKBAR.lock() {
        if let Some(ref wrapper) = *lock {
            if let Some(hwnd) = get_hwnd(window) {
                let completed = (progress * 1000.0) as u64;
                unsafe {
                    let _ = wrapper.0.SetProgressState(hwnd, TBPF_NORMAL);
                    let _ = wrapper.0.SetProgressValue(hwnd, completed, 1000);
                }
            }
        }
    }
}

/// Clear taskbar progress
pub fn clear_progress(window: &WebviewWindow) {
    if let Ok(lock) = TASKBAR.lock() {
        if let Some(ref wrapper) = *lock {
            if let Some(hwnd) = get_hwnd(window) {
                unsafe {
                    let _ = wrapper.0.SetProgressState(hwnd, TBPF_NOPROGRESS);
                }
            }
        }
    }
}
