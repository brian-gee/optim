use std::ffi::c_void;

use windows::core::{w, Result, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D_SIZE_U, D2D1_COLOR_F};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Factory, ID2D1HwndRenderTarget, ID2D1SolidColorBrush,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_HWND_RENDER_TARGET_PROPERTIES,
    D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES,
};
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
    GetKeyState, RegisterHotKey, MOD_ALT, MOD_NOREPEAT, VK_CONTROL, VK_DELETE, VK_END,
    VK_ESCAPE, VK_HOME, VK_LEFT, VK_RIGHT, VK_SPACE,
};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, LoadCursorW, PostQuitMessage, RegisterClassW,
    SetWindowLongPtrW, GetWindowLongPtrW, SetWindowPos, ShowWindow, SetForegroundWindow,
    CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, HWND_TOPMOST, IDC_ARROW,
    SWP_NOACTIVATE, SW_HIDE, SW_SHOWNA, WM_ACTIVATE, WM_APP, WM_CHAR,
    WM_DESTROY, WM_DPICHANGED, WM_ERASEBKGND, WM_HOTKEY, WM_KEYDOWN, WM_NCCREATE, WM_PAINT,
    WM_SIZE, WNDCLASSW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

pub const WINDOW_CLASS: PCWSTR = w!("flick_window");
pub const WM_APP_SHOW: u32 = WM_APP + 1;
const HOTKEY_ID: i32 = 1;

// Logical layout (scaled by DPI at render time).
const WIN_W: f32 = 640.0;
const PAD: f32 = 18.0;
const INPUT_H: f32 = 60.0;
const INPUT_FONT: f32 = 20.0;

// Palette — near-black, minimal.
const BG: D2D1_COLOR_F = rgb(0x1B, 0x1B, 0x1D);
const FG: D2D1_COLOR_F = rgb(0xEC, 0xEC, 0xEE);
const DIM: D2D1_COLOR_F = rgb(0x77, 0x77, 0x7C);

const fn rgb(r: u8, g: u8, b: u8) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: 1.0 }
}

struct Gfx {
    rt: ID2D1HwndRenderTarget,
    fg: ID2D1SolidColorBrush,
    dim: ID2D1SolidColorBrush,
    input_fmt: IDWriteTextFormat,
}

pub struct App {
    hwnd: HWND,
    d2d_factory: ID2D1Factory,
    dwrite: IDWriteFactory,
    gfx: Option<Gfx>,
    query: String,
    caret: usize, // byte offset into query, always on a char boundary
    scale: f32,
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

            let mut app = Box::new(App {
                hwnd: HWND::default(),
                d2d_factory,
                dwrite,
                gfx: None,
                query: String::new(),
                caret: 0,
                scale: 1.0,
            });

            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                WINDOW_CLASS,
                w!("flick"),
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

            RegisterHotKey(Some(hwnd), HOTKEY_ID, MOD_ALT | MOD_NOREPEAT, VK_SPACE.0 as u32)?;

            Ok(app)
        }
    }

    fn px(&self, v: f32) -> f32 {
        v * self.scale
    }

    fn desired_size(&self) -> (i32, i32) {
        // M1: input row only. Results rows come in M2.
        (self.px(WIN_W) as i32, self.px(INPUT_H) as i32)
    }

    fn show(&mut self) {
        unsafe {
            self.query.clear();
            self.caret = 0;

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

    fn ensure_gfx(&mut self) -> Result<&Gfx> {
        if self.gfx.is_none() {
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
                let fg = rt.CreateSolidColorBrush(&FG, None)?;
                let dim = rt.CreateSolidColorBrush(&DIM, None)?;
                let input_fmt = self.dwrite.CreateTextFormat(
                    w!("Segoe UI Variable Text"),
                    None,
                    DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    self.px(INPUT_FONT),
                    w!("en-us"),
                )?;
                input_fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
                self.gfx = Some(Gfx { rt, fg, dim, input_fmt });
            }
        }
        Ok(self.gfx.as_ref().unwrap())
    }

    fn render(&mut self) {
        let (w, h) = self.desired_size();
        let query_utf16: Vec<u16> = self.query.encode_utf16().collect();
        let caret_utf16: Vec<u16> = self.query[..self.caret].encode_utf16().collect();
        let pad = self.px(PAD);
        let input_h = self.px(INPUT_H);
        let font_h = self.px(INPUT_FONT) * 1.3;
        let scale = self.scale;
        let dwrite = self.dwrite.clone();

        let Ok(gfx) = self.ensure_gfx() else { return };
        unsafe {
            gfx.rt.BeginDraw();
            gfx.rt.Clear(Some(&BG));

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
            } else if let Ok(layout) = dwrite.CreateTextLayout(
                &caret_utf16,
                &gfx.input_fmt,
                f32::MAX,
                input_h,
            ) {
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

            let _ = h; // full height used in M2 for result rows
            if gfx.rt.EndDraw(None, None).is_err() {
                self.gfx = None; // device lost — recreate next frame
            }
        }
    }

    fn invalidate(&self) {
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
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
        self.invalidate();
    }

    fn on_keydown(&mut self, vk: u16) {
        match vk {
            v if v == VK_ESCAPE.0 => self.hide(),
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
                self.invalidate();
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
                WM_DESTROY => {
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