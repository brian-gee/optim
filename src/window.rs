use std::ffi::c_void;

use windows::core::{w, Result, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap, ID2D1Factory, ID2D1HwndRenderTarget, ID2D1SolidColorBrush,
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BITMAP_PROPERTIES,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_HWND_RENDER_TARGET_PROPERTIES,
    D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

use crate::calc;
use crate::config::{self, Config};
use crate::frecency;
use crate::index::{launch, AppEntry, ICON_SIZE};
use crate::matcher;
use std::collections::HashMap;
use std::time::Instant;
use windows::Win32::System::Power::SetSuspendState;
use windows::Win32::System::Shutdown::LockWorkStation;
use windows::Win32::UI::Input::KeyboardAndMouse::{UnregisterHotKey, HOT_KEY_MODIFIERS};

/// Fire-and-forget `shutdown.exe` with the given switches.
fn shutdown_exe(args: PCWSTR) {
    unsafe {
        ShellExecuteW(None, w!("open"), w!("shutdown.exe"), args, None, SW_SHOWNORMAL);
    }
}
use windows::Win32::UI::Shell::{
    ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, DestroyWindow, LoadIconW, TrackPopupMenu,
    IDI_APPLICATION, MF_SEPARATOR, MF_STRING, SW_SHOWNORMAL, TPM_NONOTIFY, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, WM_RBUTTONUP,
};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_METRICS,
};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetMonitorInfoW, InvalidateRect, MonitorFromPoint,
    MONITORINFO, MONITOR_DEFAULTTONEAREST, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, RegisterHotKey, MOD_ALT, MOD_NOREPEAT, VK_CONTROL, VK_DELETE, VK_DOWN,
    VK_END, VK_ESCAPE, VK_HOME, VK_LEFT, VK_RETURN, VK_RIGHT, VK_SPACE, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, LoadCursorW, PostQuitMessage, RegisterClassW,
    SetWindowLongPtrW, GetWindowLongPtrW, SetWindowPos, ShowWindow, SetForegroundWindow,
    CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, HWND_TOPMOST, IDC_ARROW,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, SW_HIDE, SW_SHOWNA, WM_ACTIVATE, WM_APP, WM_CHAR,
    WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_HOTKEY, WM_KEYDOWN, WM_NCCREATE, WM_PAINT,
    WM_SIZE, WNDCLASSW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

pub const WINDOW_CLASS: PCWSTR = w!("optim_window");
pub const WM_APP_SHOW: u32 = WM_APP + 1;
pub const WM_APP_INDEXED: u32 = WM_APP + 2;
const WM_APP_TRAY: u32 = WM_APP + 3;
const HOTKEY_ID: i32 = 1;
const MOD_NOREPEAT_BIT: u32 = 0x4000;

// Tray menu command ids.
const CMD_OPEN_CONFIG: usize = 1;
const CMD_RELOAD_CONFIG: usize = 2;
const CMD_REFRESH_APPS: usize = 3;
const CMD_EXIT: usize = 4;

/// Actions reachable both from the tray menu and as typed commands.
#[derive(Clone, Copy, PartialEq)]
enum Action {
    OpenConfig,
    ReloadConfig,
    RefreshApps,
    Quit,
    Restart,
    Shutdown,
    Sleep,
    Lock,
    SignOut,
}

/// Typed commands: (display name, extra match keywords, action).
/// Keywords let "exit"/"settings"/"reboot" find their commands too.
const COMMANDS: [(&str, &str, Action); 9] = [
    ("optim: Open Config", "optim open config settings edit", Action::OpenConfig),
    ("optim: Reload Config", "optim reload config settings", Action::ReloadConfig),
    ("optim: Refresh Apps", "optim refresh apps index rescan", Action::RefreshApps),
    ("optim: Quit", "optim quit exit close", Action::Quit),
    ("Restart", "restart reboot system power", Action::Restart),
    ("Shut Down", "shut down shutdown power off system", Action::Shutdown),
    ("Sleep", "sleep suspend system power", Action::Sleep),
    ("Lock", "lock workstation system", Action::Lock),
    ("Sign Out", "sign out log off logoff system", Action::SignOut),
];

/// A result row: an installed app, a built-in command, or the `>` terminal runner.
#[derive(Clone, Copy)]
enum Hit {
    App(usize),
    Cmd(usize),
    Term,
}

// Logical layout (scaled by DPI at render time).
const PAD: f32 = 18.0;
const INPUT_H: f32 = 60.0;
const INPUT_FONT: f32 = 20.0;
const ROW_H: f32 = 42.0;
const ROW_FONT: f32 = 15.0;
const BOTTOM_PAD: f32 = 8.0;

fn col(v: u32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((v >> 16) & 0xFF) as f32 / 255.0,
        g: ((v >> 8) & 0xFF) as f32 / 255.0,
        b: (v & 0xFF) as f32 / 255.0,
        a: 1.0,
    }
}

struct Gfx {
    rt: ID2D1HwndRenderTarget,
    fg: ID2D1SolidColorBrush,
    dim: ID2D1SolidColorBrush,
    sel: ID2D1SolidColorBrush,
    input_fmt: IDWriteTextFormat,
    row_fmt: IDWriteTextFormat,
    /// Per-app D2D bitmaps, keyed by index into `App::apps`. Device-bound, so
    /// this dies with the render target and is cleared on re-index.
    icons: HashMap<usize, Option<ID2D1Bitmap>>,
}

pub struct App {
    hwnd: HWND,
    d2d_factory: ID2D1Factory,
    dwrite: IDWriteFactory,
    gfx: Option<Gfx>,
    query: String,
    caret: usize, // byte offset into query, always on a char boundary
    scale: f32,
    apps: Vec<AppEntry>,
    matches: Vec<Hit>, // best first
    calc: Option<String>, // formatted result; occupies row 0 when present
    sel: usize,
    cfg: Config,
    frec: HashMap<String, u32>,
    last_index: Instant,
    /// Private collection holding the bundled Iosevka; None → system fonts only.
    iosevka: Option<windows::Win32::Graphics::DirectWrite::IDWriteFontCollection>,
}

impl App {
    pub fn create() -> Result<Box<App>> {
        unsafe {
            let hinstance = GetModuleHandleW(None)?;
            let wc = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(wndproc),
                hInstance: hinstance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                lpszClassName: WINDOW_CLASS,
                ..Default::default()
            };
            RegisterClassW(&wc);

            let d2d_factory: ID2D1Factory =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let iosevka = crate::font::iosevka_collection(&dwrite);

            let mut app = Box::new(App {
                hwnd: HWND::default(),
                d2d_factory,
                dwrite,
                gfx: None,
                query: String::new(),
                caret: 0,
                scale: 1.0,
                apps: Vec::new(),
                matches: Vec::new(),
                calc: None,
                sel: 0,
                cfg: config::load(),
                frec: frecency::load(),
                last_index: Instant::now(),
                iosevka,
            });

            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                WINDOW_CLASS,
                w!("optim"),
                WS_POPUP,
                0, 0, 640, 60,
                None,
                None,
                Some(hinstance.into()),
                Some(&*app as *const App as *const c_void),
            )?;
            app.hwnd = hwnd;
            app.scale = GetDpiForWindow(hwnd) as f32 / 96.0;

            let corner = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner as *const _ as *const c_void,
                std::mem::size_of_val(&corner) as u32,
            );

            app.register_hotkey()?;
            app.add_tray_icon();

            Ok(app)
        }
    }

    fn register_hotkey(&self) -> Result<()> {
        unsafe {
            let mods = HOT_KEY_MODIFIERS(self.cfg.hotkey_mods | MOD_NOREPEAT_BIT);
            if RegisterHotKey(Some(self.hwnd), HOTKEY_ID, mods, self.cfg.hotkey_vk).is_err() {
                // Config combo unavailable — fall back to Alt+Space so optim stays reachable.
                RegisterHotKey(
                    Some(self.hwnd),
                    HOTKEY_ID,
                    MOD_ALT | MOD_NOREPEAT,
                    VK_SPACE.0 as u32,
                )?;
            }
            Ok(())
        }
    }

    fn add_tray_icon(&self) {
        unsafe {
            let mut nid = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: self.hwnd,
                uID: 1,
                uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
                uCallbackMessage: WM_APP_TRAY,
                hIcon: LoadIconW(None, IDI_APPLICATION).unwrap_or_default(),
                ..Default::default()
            };
            for (i, u) in "optim".encode_utf16().enumerate() {
                nid.szTip[i] = u;
            }
            let _ = Shell_NotifyIconW(NIM_ADD, &nid);
        }
    }

    fn tray_menu(&mut self) {
        unsafe {
            let Ok(menu) = CreatePopupMenu() else { return };
            let _ = AppendMenuW(menu, MF_STRING, CMD_OPEN_CONFIG, w!("Open Config"));
            let _ = AppendMenuW(menu, MF_STRING, CMD_RELOAD_CONFIG, w!("Reload Config"));
            let _ = AppendMenuW(menu, MF_STRING, CMD_REFRESH_APPS, w!("Refresh Apps"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
            let _ = AppendMenuW(menu, MF_STRING, CMD_EXIT, w!("Exit"));

            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let _ = SetForegroundWindow(self.hwnd); // so the menu dismisses on outside click
            let cmd = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY,
                pt.x,
                pt.y,
                None,
                self.hwnd,
                None,
            );
            let _ = DestroyMenu(menu);

            match cmd.0 as usize {
                CMD_OPEN_CONFIG => self.run_action(Action::OpenConfig),
                CMD_RELOAD_CONFIG => self.run_action(Action::ReloadConfig),
                CMD_REFRESH_APPS => self.run_action(Action::RefreshApps),
                CMD_EXIT => self.run_action(Action::Quit),
                _ => {}
            }
        }
    }

    fn run_action(&mut self, action: Action) {
        unsafe {
            match action {
                Action::OpenConfig => {
                    let path16: Vec<u16> = config::path()
                        .to_string_lossy()
                        .encode_utf16()
                        .chain(std::iter::once(0))
                        .collect();
                    if self.cfg.editor.is_empty() {
                        // System default handler for .ini.
                        ShellExecuteW(
                            None,
                            w!("open"),
                            PCWSTR(path16.as_ptr()),
                            None,
                            None,
                            SW_SHOWNORMAL,
                        );
                    } else {
                        // Configured editor with the config path as argument.
                        let editor16: Vec<u16> = self
                            .cfg
                            .editor
                            .encode_utf16()
                            .chain(std::iter::once(0))
                            .collect();
                        let arg: String = format!("\"{}\"", config::path().to_string_lossy());
                        let arg16: Vec<u16> =
                            arg.encode_utf16().chain(std::iter::once(0)).collect();
                        ShellExecuteW(
                            None,
                            w!("open"),
                            PCWSTR(editor16.as_ptr()),
                            PCWSTR(arg16.as_ptr()),
                            None,
                            SW_SHOWNORMAL,
                        );
                    }
                }
                Action::ReloadConfig => self.reload_config(),
                Action::RefreshApps => {
                    let hwnd_val = self.hwnd_val();
                    std::thread::spawn(move || crate::index::run_index(hwnd_val));
                }
                Action::Quit => {
                    let _ = DestroyWindow(self.hwnd);
                }
                Action::Restart => shutdown_exe(w!("/r /t 0")),
                Action::Shutdown => shutdown_exe(w!("/s /t 0")),
                Action::SignOut => shutdown_exe(w!("/l")),
                Action::Sleep => {
                    let _ = SetSuspendState(false, false, false);
                }
                Action::Lock => {
                    let _ = LockWorkStation();
                }
            }
        }
    }

    fn reload_config(&mut self) {
        unsafe {
            self.cfg = config::load();
            self.gfx = None; // brushes and text formats rebuild from new config
            let _ = UnregisterHotKey(Some(self.hwnd), HOTKEY_ID);
            let _ = self.register_hotkey();
            self.apply_size();
            self.invalidate();
        }
    }

    pub fn hwnd_val(&self) -> isize {
        self.hwnd.0 as isize
    }

    fn px(&self, v: f32) -> f32 {
        v * self.scale
    }

    fn total_rows(&self) -> usize {
        self.matches.len() + self.calc.is_some() as usize
    }

    fn desired_size(&self) -> (i32, i32) {
        let rows = self.total_rows();
        let mut h = INPUT_H + rows as f32 * ROW_H;
        if rows > 0 {
            h += BOTTOM_PAD;
        }
        (self.px(self.cfg.width) as i32, self.px(h) as i32)
    }

    fn update_matches(&mut self) {
        self.matches.clear();
        self.sel = 0;
        // `>` prefix: everything after it is a shell command, nothing else matches.
        if self.query.trim_start().starts_with('>') {
            self.calc = None;
            if !self.terminal_command().is_empty() {
                self.matches.push(Hit::Term);
            }
            self.apply_size();
            self.invalidate();
            return;
        }
        self.calc = calc::eval(self.query.trim()).map(calc::format);
        let q = self.query.trim().to_lowercase();
        if !q.is_empty() {
            let mut scored: Vec<(i32, Hit)> = self
                .apps
                .iter()
                .enumerate()
                .filter_map(|(i, a)| {
                    matcher::score(&q, &a.name_lower).map(|s| {
                        let boost = self.frec.get(&a.name).copied().unwrap_or(0).min(10) * 4;
                        (s + boost as i32, Hit::App(i))
                    })
                })
                .collect();
            scored.extend(COMMANDS.iter().enumerate().filter_map(|(i, (_, keys, _))| {
                matcher::score(&q, keys).map(|s| (s, Hit::Cmd(i)))
            }));
            scored.sort_unstable_by_key(|&(s, _)| std::cmp::Reverse(s));
            self.matches
                .extend(scored.iter().take(self.cfg.max_rows).map(|&(_, h)| h));
        }
        self.apply_size();
        self.invalidate();
    }

    /// The command text after a `>` prefix, trimmed.
    fn terminal_command(&self) -> &str {
        self.query.trim_start().strip_prefix('>').unwrap_or("").trim()
    }

    fn run_in_terminal(&self) {
        unsafe {
            let args = format!("cmd /k {}", self.terminal_command());
            let args16: Vec<u16> = args.encode_utf16().chain(std::iter::once(0)).collect();
            let h = ShellExecuteW(
                None,
                w!("open"),
                w!("wt.exe"),
                PCWSTR(args16.as_ptr()),
                None,
                SW_SHOWNORMAL,
            );
            if h.0 as isize <= 32 {
                // No Windows Terminal — plain cmd window.
                let args = format!("/k {}", self.terminal_command());
                let args16: Vec<u16> = args.encode_utf16().chain(std::iter::once(0)).collect();
                ShellExecuteW(
                    None,
                    w!("open"),
                    w!("cmd.exe"),
                    PCWSTR(args16.as_ptr()),
                    None,
                    SW_SHOWNORMAL,
                );
            }
        }
    }

    fn apply_size(&self) {
        unsafe {
            let (w, h) = self.desired_size();
            let _ = SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                w,
                h,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    fn show(&mut self) {
        unsafe {
            self.query.clear();
            self.caret = 0;
            self.matches.clear();
            self.calc = None;
            self.sel = 0;

            // Staleness guard: refresh in the background if the index is old.
            // Covers MSIX installs/updates the Start Menu watcher can't see.
            if self.last_index.elapsed().as_secs() > 300 {
                self.last_index = Instant::now();
                let hwnd_val = self.hwnd_val();
                std::thread::spawn(move || crate::index::run_index(hwnd_val));
            }

            // Place on the monitor holding the cursor, centered, upper third.
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
            let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            let _ = GetMonitorInfoW(mon, &mut mi);
            let work: RECT = mi.rcWork;

            let (w, h) = self.desired_size();
            let x = work.left + ((work.right - work.left) - w) / 2;
            let y = work.top + ((work.bottom - work.top) / 4);
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE);

            let _ = ShowWindow(self.hwnd, SW_SHOWNA);
            let _ = SetForegroundWindow(self.hwnd);
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    fn hide(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    fn is_visible(&self) -> bool {
        unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(self.hwnd).as_bool() }
    }

    fn build_gfx(&self) -> Result<Gfx> {
        {
            unsafe {
                let (w, h) = self.desired_size();
                let rt = self.d2d_factory.CreateHwndRenderTarget(
                    &D2D1_RENDER_TARGET_PROPERTIES {
                        dpiX: 96.0, // render in raw pixels; we scale manually
                        dpiY: 96.0,
                        ..Default::default()
                    },
                    &D2D1_HWND_RENDER_TARGET_PROPERTIES {
                        hwnd: self.hwnd,
                        pixelSize: D2D_SIZE_U { width: w as u32, height: h as u32 },
                        presentOptions: D2D1_PRESENT_OPTIONS_NONE,
                    },
                )?;
                let fg = rt.CreateSolidColorBrush(&col(self.cfg.fg), None)?;
                let dim = rt.CreateSolidColorBrush(&col(self.cfg.dim), None)?;
                let sel = rt.CreateSolidColorBrush(&col(self.cfg.sel), None)?;
                // "iosevka" (or empty) = the bundled font from our private
                // collection; anything else = an installed family by name.
                let use_bundled = (self.cfg.font.is_empty()
                    || self.cfg.font.eq_ignore_ascii_case("iosevka"))
                    && self.iosevka.is_some();
                let family = if use_bundled {
                    crate::font::IOSEVKA_FAMILY.to_string()
                } else if self.cfg.font.is_empty()
                    || self.cfg.font.eq_ignore_ascii_case("iosevka")
                {
                    "Segoe UI Variable Text".to_string() // bundled font failed to load
                } else {
                    self.cfg.font.clone()
                };
                let collection = if use_bundled { self.iosevka.as_ref() } else { None };
                let font16: Vec<u16> =
                    family.encode_utf16().chain(std::iter::once(0)).collect();
                let font = PCWSTR(font16.as_ptr());
                let input_fmt = self.dwrite.CreateTextFormat(
                    font,
                    collection,
                    DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    self.px(INPUT_FONT),
                    w!("en-us"),
                )?;
                input_fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
                let row_fmt = self.dwrite.CreateTextFormat(
                    font,
                    collection,
                    DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    self.px(ROW_FONT),
                    w!("en-us"),
                )?;
                row_fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
                Ok(Gfx {
                    rt,
                    fg,
                    dim,
                    sel,
                    input_fmt,
                    row_fmt,
                    icons: HashMap::new(),
                })
            }
        }
    }

    /// Returns the cached D2D bitmap for an app, creating it from the
    /// extracted BGRA pixels on first use. Cloning a COM pointer is an AddRef.
    fn icon_bitmap(gfx: &mut Gfx, apps: &[AppEntry], idx: usize) -> Option<ID2D1Bitmap> {
        let rt = gfx.rt.clone();
        gfx.icons
            .entry(idx)
            .or_insert_with(|| unsafe {
                let pixels = apps[idx].icon.as_ref()?;
                rt.CreateBitmap(
                    D2D_SIZE_U {
                        width: ICON_SIZE as u32,
                        height: ICON_SIZE as u32,
                    },
                    Some(pixels.as_ptr() as _),
                    (ICON_SIZE * 4) as u32,
                    &D2D1_BITMAP_PROPERTIES {
                        pixelFormat: D2D1_PIXEL_FORMAT {
                            format: DXGI_FORMAT_B8G8R8A8_UNORM,
                            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                        },
                        dpiX: 96.0,
                        dpiY: 96.0,
                    },
                )
                .ok()
            })
            .clone()
    }

    fn render(&mut self) {
        if self.gfx.is_none() {
            self.gfx = self.build_gfx().ok();
        }
        let Some(mut gfx) = self.gfx.take() else { return };

        let (w, _h) = self.desired_size();
        let query_utf16: Vec<u16> = self.query.encode_utf16().collect();
        let caret_utf16: Vec<u16> = self.query[..self.caret].encode_utf16().collect();
        let pad = self.px(PAD);
        let input_h = self.px(INPUT_H);
        let row_h = self.px(ROW_H);
        let font_h = self.px(INPUT_FONT) * 1.3;
        let scale = self.scale;
        let icon_edge = 20.0 * scale;
        let text_left = pad + icon_edge + 12.0 * scale; // rows indent past the icon slot

        unsafe {
            gfx.rt.BeginDraw();
            gfx.rt.Clear(Some(&col(self.cfg.bg)));

            let input_rect = D2D_RECT_F {
                left: pad,
                top: 0.0,
                right: w as f32 - pad,
                bottom: input_h,
            };

            if query_utf16.is_empty() {
                let placeholder: Vec<u16> = "Search".encode_utf16().collect();
                gfx.rt.DrawText(
                    &placeholder,
                    &gfx.input_fmt,
                    &input_rect,
                    &gfx.dim,
                    Default::default(),
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            } else {
                gfx.rt.DrawText(
                    &query_utf16,
                    &gfx.input_fmt,
                    &input_rect,
                    &gfx.fg,
                    Default::default(),
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }

            // Caret: measure text up to caret, draw a thin bar.
            let caret_x = if caret_utf16.is_empty() {
                0.0
            } else if let Ok(layout) =
                self.dwrite
                    .CreateTextLayout(&caret_utf16, &gfx.input_fmt, f32::MAX, input_h)
            {
                let mut m = DWRITE_TEXT_METRICS::default();
                let _ = layout.GetMetrics(&mut m);
                m.widthIncludingTrailingWhitespace
            } else {
                0.0
            };
            let cy = input_h / 2.0;
            gfx.rt.FillRectangle(
                &D2D_RECT_F {
                    left: pad + caret_x + 1.0,
                    top: cy - font_h / 2.0,
                    right: pad + caret_x + 1.0 + 2.0 * scale,
                    bottom: cy + font_h / 2.0,
                },
                &gfx.fg,
            );

            // Result rows: calc first (an "=" in the icon slot), then apps.
            let calc_rows = self.calc.is_some() as usize;
            for row in 0..self.total_rows() {
                let top = input_h + row as f32 * row_h;
                if row == self.sel {
                    gfx.rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F {
                                left: pad * 0.5,
                                top: top + 1.0,
                                right: w as f32 - pad * 0.5,
                                bottom: top + row_h - 1.0,
                            },
                            radiusX: 6.0 * scale,
                            radiusY: 6.0 * scale,
                        },
                        &gfx.sel,
                    );
                }

                let icon_rect = D2D_RECT_F {
                    left: pad,
                    top: top + (row_h - icon_edge) / 2.0,
                    right: pad + icon_edge,
                    bottom: top + (row_h + icon_edge) / 2.0,
                };
                let text_rect = D2D_RECT_F {
                    left: text_left,
                    top,
                    right: w as f32 - pad,
                    bottom: top + row_h,
                };

                if row < calc_rows {
                    let eq: Vec<u16> = "=".encode_utf16().collect();
                    gfx.rt.DrawText(
                        &eq,
                        &gfx.row_fmt,
                        &D2D_RECT_F { left: pad, ..text_rect },
                        &gfx.dim,
                        Default::default(),
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                    let result16: Vec<u16> =
                        self.calc.as_deref().unwrap_or("").encode_utf16().collect();
                    gfx.rt.DrawText(
                        &result16,
                        &gfx.row_fmt,
                        &text_rect,
                        &gfx.fg,
                        Default::default(),
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                } else {
                    let term_name: String;
                    let name: &str = match self.matches[row - calc_rows] {
                        Hit::Term => {
                            let glyph: Vec<u16> = "\u{203A}".encode_utf16().collect(); // ›
                            gfx.rt.DrawText(
                                &glyph,
                                &gfx.row_fmt,
                                &D2D_RECT_F { left: pad + 4.0 * scale, ..text_rect },
                                &gfx.dim,
                                Default::default(),
                                DWRITE_MEASURING_MODE_NATURAL,
                            );
                            term_name = format!("run: {}", self.terminal_command());
                            &term_name
                        }
                        Hit::App(idx) => {
                            if let Some(bmp) = Self::icon_bitmap(&mut gfx, &self.apps, idx) {
                                gfx.rt.DrawBitmap(
                                    &bmp,
                                    Some(&icon_rect),
                                    1.0,
                                    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                                    None,
                                );
                            }
                            &self.apps[idx].name
                        }
                        Hit::Cmd(idx) => {
                            let glyph: Vec<u16> = "\u{203A}".encode_utf16().collect(); // ›
                            gfx.rt.DrawText(
                                &glyph,
                                &gfx.row_fmt,
                                &D2D_RECT_F { left: pad + 4.0 * scale, ..text_rect },
                                &gfx.dim,
                                Default::default(),
                                DWRITE_MEASURING_MODE_NATURAL,
                            );
                            COMMANDS[idx].0
                        }
                    };
                    let name16: Vec<u16> = name.encode_utf16().collect();
                    gfx.rt.DrawText(
                        &name16,
                        &gfx.row_fmt,
                        &text_rect,
                        &gfx.fg,
                        Default::default(),
                        DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
            }

            if gfx.rt.EndDraw(None, None).is_ok() {
                self.gfx = Some(gfx); // else: device lost — rebuild next frame
            }
        }
    }

    fn invalidate(&self) {
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    fn copy_to_clipboard(&self, text: &str) {
        unsafe {
            let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            let bytes = utf16.len() * 2;
            if OpenClipboard(Some(self.hwnd)).is_err() {
                return;
            }
            let _ = EmptyClipboard();
            if let Ok(hmem) = GlobalAlloc(GMEM_MOVEABLE, bytes) {
                let ptr = GlobalLock(hmem);
                if !ptr.is_null() {
                    std::ptr::copy_nonoverlapping(utf16.as_ptr(), ptr as *mut u16, utf16.len());
                    let _ = GlobalUnlock(hmem);
                    // 13 = CF_UNICODETEXT; ownership passes to the clipboard on success.
                    if SetClipboardData(13, Some(HANDLE(hmem.0))).is_err() {
                        let _ = windows::Win32::Foundation::GlobalFree(Some(hmem));
                    }
                } else {
                    let _ = windows::Win32::Foundation::GlobalFree(Some(hmem));
                }
            }
            let _ = CloseClipboard();
        }
    }

    fn ctrl_down() -> bool {
        unsafe { (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 }
    }

    fn prev_boundary(&self, word: bool) -> usize {
        if !word {
            return self.query[..self.caret]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
        let before = &self.query[..self.caret];
        let trimmed = before.trim_end_matches(' ');
        match trimmed.rfind(' ') {
            Some(i) => i + 1,
            None => 0,
        }
    }

    fn next_boundary(&self, word: bool) -> usize {
        if !word {
            return self.query[self.caret..]
                .chars()
                .next()
                .map(|c| self.caret + c.len_utf8())
                .unwrap_or(self.caret);
        }
        let after = &self.query[self.caret..];
        let skipped = after.len() - after.trim_start_matches(' ').len();
        match after[skipped..].find(' ') {
            Some(i) => self.caret + skipped + i,
            None => self.query.len(),
        }
    }

    fn on_char(&mut self, c: u16) {
        match c {
            0x08 => {
                // Backspace (Ctrl+Backspace arrives as 0x7F on some layouts; handle both)
                let start = self.prev_boundary(Self::ctrl_down());
                self.query.replace_range(start..self.caret, "");
                self.caret = start;
            }
            0x7F => {
                let start = self.prev_boundary(true);
                self.query.replace_range(start..self.caret, "");
                self.caret = start;
            }
            0x0D | 0x1B | 0x09 => return, // Enter/Esc/Tab handled elsewhere
            c if c >= 0x20 => {
                // TODO(surrogates): pair handling for astral-plane chars
                if let Some(ch) = char::from_u32(c as u32) {
                    self.query.insert(self.caret, ch);
                    self.caret += ch.len_utf8();
                }
            }
            _ => return,
        }
        self.update_matches();
    }

    fn on_keydown(&mut self, vk: u16) {
        match vk {
            v if v == VK_ESCAPE.0 => self.hide(),
            v if v == VK_RETURN.0 => {
                let calc_rows = self.calc.is_some() as usize;
                if self.calc.is_some() && self.sel == 0 {
                    let text = self.calc.clone().unwrap();
                    self.copy_to_clipboard(&text);
                    self.hide();
                } else {
                    match self.matches.get(self.sel - calc_rows) {
                        Some(&Hit::App(idx)) => {
                            launch(&self.apps[idx]);
                            let name = self.apps[idx].name.clone();
                            frecency::bump(&mut self.frec, &name);
                            self.hide();
                        }
                        Some(&Hit::Cmd(idx)) => {
                            self.hide();
                            self.run_action(COMMANDS[idx].2);
                        }
                        Some(&Hit::Term) => {
                            self.run_in_terminal();
                            self.hide();
                        }
                        None => {}
                    }
                }
            }
            v if v == VK_UP.0 => {
                let n = self.total_rows();
                if n > 0 {
                    self.sel = self.sel.checked_sub(1).unwrap_or(n - 1);
                    self.invalidate();
                }
            }
            v if v == VK_DOWN.0 => {
                let n = self.total_rows();
                if n > 0 {
                    self.sel = (self.sel + 1) % n;
                    self.invalidate();
                }
            }
            v if v == VK_LEFT.0 => {
                self.caret = self.prev_boundary(Self::ctrl_down());
                self.invalidate();
            }
            v if v == VK_RIGHT.0 => {
                self.caret = self.next_boundary(Self::ctrl_down());
                self.invalidate();
            }
            v if v == VK_HOME.0 => {
                self.caret = 0;
                self.invalidate();
            }
            v if v == VK_END.0 => {
                self.caret = self.query.len();
                self.invalidate();
            }
            v if v == VK_DELETE.0 => {
                let end = self.next_boundary(Self::ctrl_down());
                self.query.replace_range(self.caret..end, "");
                self.update_matches();
            }
            _ => {}
        }
    }

    fn handle(&mut self, hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe {
            match msg {
                WM_HOTKEY => {
                    if self.is_visible() {
                        self.hide();
                    } else {
                        self.show();
                    }
                    LRESULT(0)
                }
                WM_APP_SHOW => {
                    self.show();
                    LRESULT(0)
                }
                WM_APP_INDEXED => {
                    let boxed = Box::from_raw(lparam.0 as *mut Vec<AppEntry>);
                    self.apps = *boxed;
                    self.last_index = Instant::now();
                    if let Some(gfx) = &mut self.gfx {
                        gfx.icons.clear(); // indices into apps changed
                    }
                    self.update_matches();
                    LRESULT(0)
                }
                WM_CHAR => {
                    self.on_char(wparam.0 as u16);
                    LRESULT(0)
                }
                WM_KEYDOWN => {
                    self.on_keydown(wparam.0 as u16);
                    LRESULT(0)
                }
                WM_ACTIVATE => {
                    if (wparam.0 as u32 & 0xFFFF) == 0 && self.is_visible() {
                        self.hide();
                    }
                    LRESULT(0)
                }
                WM_PAINT => {
                    let mut ps = PAINTSTRUCT::default();
                    let _ = BeginPaint(hwnd, &mut ps);
                    self.render();
                    let _ = EndPaint(hwnd, &ps);
                    LRESULT(0)
                }
                WM_ERASEBKGND => LRESULT(1),
                WM_SIZE => {
                    if let Some(gfx) = &self.gfx {
                        let w = (lparam.0 as u32) & 0xFFFF;
                        let h = ((lparam.0 as u32) >> 16) & 0xFFFF;
                        let _ = gfx.rt.Resize(&D2D_SIZE_U { width: w, height: h });
                    }
                    LRESULT(0)
                }
                WM_DPICHANGED => {
                    self.scale = ((wparam.0 as u32) & 0xFFFF) as f32 / 96.0;
                    self.gfx = None; // rebuild formats at new scale
                    let rect = &*(lparam.0 as *const RECT);
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        rect.left,
                        rect.top,
                        rect.right - rect.left,
                        rect.bottom - rect.top,
                        SWP_NOACTIVATE,
                    );
                    LRESULT(0)
                }
                WM_APP_TRAY => {
                    if (lparam.0 as u32 & 0xFFFF) == WM_RBUTTONUP {
                        self.tray_menu();
                    }
                    LRESULT(0)
                }
                WM_DESTROY => {
                    let nid = NOTIFYICONDATAW {
                        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                        hWnd: hwnd,
                        uID: 1,
                        ..Default::default()
                    };
                    let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
                    PostQuitMessage(0);
                    LRESULT(0)
                }
                _ => DefWindowProcW(hwnd, msg, wparam, lparam),
            }
        }
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_NCCREATE {
            let cs = lparam.0 as *const CREATESTRUCTW;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, (*cs).lpCreateParams as isize);
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        let app = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
        if app.is_null() {
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        (*app).handle(hwnd, msg, wparam, lparam)
    }
}