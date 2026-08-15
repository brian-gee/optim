#![windows_subsystem = "windows"]

mod calc;
mod config;
mod font;
mod watch;
mod frecency;
mod hidden;
mod index;
mod matcher;
mod services;
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

/// Adds or removes the HKCU Run entry that starts optim at logon.
fn set_autostart(enable: bool) -> Result<()> {
    use windows::Win32::System::Registry::{
        RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ,
    };
    unsafe {
        let subkey = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
        let name = w!("optim");
        if enable {
            let exe = std::env::current_exe().map_err(|_| windows::core::Error::empty())?;
            let path16: Vec<u16> = exe
                .to_string_lossy()
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                subkey,
                name,
                REG_SZ.0,
                Some(path16.as_ptr() as _),
                (path16.len() * 2) as u32,
            )
            .ok()?;
        } else {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, subkey, name).ok()?;
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    // Watch-queue edits arrive as short-lived processes: mpv's tab menu shells
    // out to these rather than writing the history itself, so optim stays the
    // only writer. They must run before the single-instance check below.
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--watch-forget") {
        if let Some(target) = args.get(i + 1) {
            watch::forget(target);
        }
        return Ok(());
    }
    if args.iter().any(|a| a == "--watch-clear") {
        watch::clear();
        return Ok(());
    }
    if std::env::args().any(|a| a == "--install-autostart") {
        return set_autostart(true);
    }
    if std::env::args().any(|a| a == "--uninstall-autostart") {
        return set_autostart(false);
    }
    if std::env::args().any(|a| a == "--version") {
        use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_OK};
        let text: Vec<u16> = concat!("optim ", env!("CARGO_PKG_VERSION"))
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        unsafe {
            MessageBoxW(
                None,
                windows::core::PCWSTR(text.as_ptr()),
                w!("optim"),
                MB_OK,
            );
        }
        return Ok(());
    }
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

        // The watch queue keeps every video until it's explicitly dropped;
        // the startup sweep only collects files no history entry points at.
        // Recording the exe path lets mpv's tab menu call back into optim.
        std::thread::spawn(|| {
            watch::record_exe_path();
            watch::sweep();
        });

        // Watch both Start Menu program folders so new apps appear live.
        for base in [
            std::env::var("APPDATA").ok(),
            std::env::var("ProgramData").ok(),
        ]
        .into_iter()
        .flatten()
        {
            let dir = format!("{base}\\Microsoft\\Windows\\Start Menu\\Programs");
            std::thread::spawn(move || index::watch_dir(dir, hwnd_val));
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}
