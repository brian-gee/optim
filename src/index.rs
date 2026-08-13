use windows::core::{w, Result, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{
    CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::{
    BHID_EnumItems, IEnumShellItems, IShellItem, SHCreateItemFromParsingName, ShellExecuteExW,
    SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING,
};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, SW_SHOWNORMAL};

use crate::window::WM_APP_INDEXED;

pub struct AppEntry {
    pub name: String,
    pub name_lower: String,
    /// Null-terminated UTF-16 of `shell:AppsFolder\<parsing name>` — ready for ShellExecuteExW.
    pub launch_id: Vec<u16>,
}

/// Runs on a background thread; posts a Box<Vec<AppEntry>> to the UI thread when done.
pub fn run_index(hwnd_val: isize) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let apps = enumerate().unwrap_or_default();
        let boxed = Box::new(apps);
        let hwnd = HWND(hwnd_val as *mut _);
        if PostMessageW(
            Some(hwnd),
            WM_APP_INDEXED,
            WPARAM(0),
            LPARAM(Box::into_raw(boxed) as isize),
        )
        .is_err()
        {
            // Window is gone; nothing to deliver to.
        }
        CoUninitialize();
    }
}

fn display_name(item: &IShellItem, sigdn: windows::Win32::UI::Shell::SIGDN) -> Option<String> {
    unsafe {
        let pw = item.GetDisplayName(sigdn).ok()?;
        let s = pw.to_string().ok();
        CoTaskMemFree(Some(pw.0 as _));
        s
    }
}

fn enumerate() -> Result<Vec<AppEntry>> {
    unsafe {
        let folder: IShellItem = SHCreateItemFromParsingName(w!("shell:AppsFolder"), None)?;
        let items: IEnumShellItems = folder.BindToHandler(None, &BHID_EnumItems)?;
        let mut out = Vec::with_capacity(256);
        loop {
            let mut slot: [Option<IShellItem>; 1] = [None];
            let mut fetched = 0u32;
            let _ = items.Next(&mut slot, Some(&mut fetched));
            if fetched == 0 {
                break;
            }
            let Some(item) = slot[0].take() else { break };
            let Some(name) = display_name(&item, SIGDN_NORMALDISPLAY) else { continue };
            let Some(parsing) = display_name(&item, SIGDN_PARENTRELATIVEPARSING) else { continue };
            if name.is_empty() {
                continue;
            }
            let mut launch_id: Vec<u16> = "shell:AppsFolder\\".encode_utf16().collect();
            launch_id.extend(parsing.encode_utf16());
            launch_id.push(0);
            out.push(AppEntry {
                name_lower: name.to_lowercase(),
                name,
                launch_id,
            });
        }
        Ok(out)
    }
}

pub fn launch(entry: &AppEntry) {
    unsafe {
        let mut sei = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOASYNC,
            lpFile: PCWSTR(entry.launch_id.as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        let _ = ShellExecuteExW(&mut sei);
    }
}
