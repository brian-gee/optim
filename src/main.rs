#![windows_subsystem = "windows"]

mod index;
mod matcher;
mod window;

use windows::core::{w, Result};
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, FindWindowW, GetMessageW, PostMessageW, TranslateMessage, MSG,
};

use window::{App, WINDOW_CLASS, WM_APP_SHOW};

fn main() -> Result<()> {
    unsafe {
        // Single instance: if optim is already running, tell it to show itself and exit.
        let _mutex = CreateMutexW(None, true, w!("optim_single_instance_mutex"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if let Ok(existing) = FindWindowW(WINDOW_CLASS, None) {
                if existing != HWND::default() {
                    let _ = PostMessageW(Some(existing), WM_APP_SHOW, WPARAM(0), LPARAM(0));
                }
            }
            return Ok(());
        }

        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;

        let _app = App::create()?;

        let hwnd_val = _app.hwnd_val();
        std::thread::spawn(move || index::run_index(hwnd_val));

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}
