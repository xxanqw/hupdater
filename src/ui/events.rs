use std::sync::{Mutex, atomic::{AtomicIsize, Ordering}};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

pub const WM_UI_EVENT: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 1;

static UI_HWND: AtomicIsize = AtomicIsize::new(0);
static EVENT_QUEUE: Mutex<Vec<UiEvent>> = Mutex::new(Vec::new());

#[derive(Debug, Clone)]
pub enum UiEvent {
    SetInstallStatus(String),
    SetTaskInfo(String),
    SetControlsEnabled(bool),
    SetIsBusy(bool),
    SetInterceptorEnabled(bool),
    SetInstallScope(String),
    SetHeliumInstalled(bool),
}

pub fn set_ui_hwnd(hwnd: HWND) {
    UI_HWND.store(hwnd.0, Ordering::SeqCst);
}

pub fn post_ui_event(event: UiEvent) {
    let hwnd_val = UI_HWND.load(Ordering::SeqCst);
    if hwnd_val == 0 {
        return;
    }
    let hwnd = HWND(hwnd_val);
    if let Ok(mut queue) = EVENT_QUEUE.lock() {
        queue.push(event);
        unsafe {
            let _ = PostMessageW(hwnd, WM_UI_EVENT, WPARAM(0), LPARAM(0));
        }
    }
}

pub fn drain_events() -> Vec<UiEvent> {
    EVENT_QUEUE.lock().map(|mut q| q.drain(..).collect()).unwrap_or_default()
}
