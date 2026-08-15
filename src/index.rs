use windows::core::{w, Interface, Result, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadDirectoryChangesW, FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY,
    FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Com::{
    CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};
use windows::Win32::UI::Shell::{
    BHID_EnumItems, IEnumShellItems, IShellItem, IShellItemImageFactory,
    SHCreateItemFromParsingName, ShellExecuteExW, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW,
    SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING, SIIGBF_ICONONLY, SIIGBF_RESIZETOFIT,
};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, SW_SHOWNORMAL};

use crate::window::WM_APP_INDEXED;

/// Icon edge length in physical pixels as extracted; scaled at draw time.
pub const ICON_SIZE: i32 = 32;

pub struct AppEntry {
    pub name: String,
    pub name_lower: String,
    /// The shell's parsing name on its own. Unique and stable per app, so it
    /// is what the hidden list keys on — display names collide and change.
    pub key: String,
    /// Null-terminated UTF-16 of `shell:AppsFolder\<parsing name>` — ready for ShellExecuteExW.
    pub launch_id: Vec<u16>,
    /// 32x32 top-down premultiplied BGRA, if the shell could produce one.
    pub icon: Option<Vec<u8>>,
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
        // Icon extraction drags shell DLL pages into the working set; give
        // them back so resident memory reflects what optim actually uses.
        let _ = SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
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
                key: parsing,
                launch_id,
                icon: get_icon(&item),
            });
        }
        Ok(out)
    }
}

fn get_icon(item: &IShellItem) -> Option<Vec<u8>> {
    unsafe {
        let factory: IShellItemImageFactory = item.cast().ok()?;
        let hbm = factory
            .GetImage(
                SIZE { cx: ICON_SIZE, cy: ICON_SIZE },
                SIIGBF_ICONONLY | SIIGBF_RESIZETOFIT,
            )
            .ok()?;

        let hdc = CreateCompatibleDC(None);
        let mut bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: ICON_SIZE,
                biHeight: -ICON_SIZE, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; (ICON_SIZE * ICON_SIZE * 4) as usize];
        let lines = GetDIBits(
            hdc,
            hbm,
            0,
            ICON_SIZE as u32,
            Some(buf.as_mut_ptr() as _),
            &mut bi,
            DIB_RGB_COLORS,
        );
        let _ = DeleteDC(hdc);
        let _ = DeleteObject(HGDIOBJ(hbm.0));
        if lines == 0 {
            return None;
        }
        // Bitmaps without an alpha channel come back all-zero in A; treat as opaque.
        if buf.chunks_exact(4).all(|p| p[3] == 0) {
            for p in buf.chunks_exact_mut(4) {
                p[3] = 255;
            }
        }
        Some(buf)
    }
}

/// Blocks on directory changes under a Start Menu programs folder and
/// re-indexes (debounced) so new/renamed .lnk apps appear while optim runs.
pub fn watch_dir(dir: String, hwnd_val: isize) {
    unsafe {
        let path16: Vec<u16> = dir.encode_utf16().chain(std::iter::once(0)).collect();
        let Ok(handle) = CreateFileW(
            PCWSTR(path16.as_ptr()),
            FILE_LIST_DIRECTORY.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        ) else {
            return;
        };
        loop {
            let mut buf = [0u8; 8192];
            let mut returned = 0u32;
            if ReadDirectoryChangesW(
                handle,
                buf.as_mut_ptr() as _,
                buf.len() as u32,
                true,
                FILE_NOTIFY_CHANGE_FILE_NAME
                    | FILE_NOTIFY_CHANGE_DIR_NAME
                    | FILE_NOTIFY_CHANGE_LAST_WRITE,
                Some(&mut returned),
                None,
                None,
            )
            .is_err()
            {
                return;
            }
            // Debounce installer bursts, then rebuild the index on this thread.
            std::thread::sleep(std::time::Duration::from_secs(2));
            run_index(hwnd_val);
        }
    }
}

pub fn launch(entry: &AppEntry) {
    launch_impl(entry, false);
}

/// Elevated launch (UAC prompt). MSIX/UWP apps ignore the verb — they run
/// in their own container and can't be elevated this way.
pub fn launch_admin(entry: &AppEntry) {
    launch_impl(entry, true);
}

fn launch_impl(entry: &AppEntry, admin: bool) {
    unsafe {
        let mut sei = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOASYNC,
            lpVerb: if admin { w!("runas") } else { PCWSTR::null() },
            lpFile: PCWSTR(entry.launch_id.as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        let _ = ShellExecuteExW(&mut sei);
    }
}
