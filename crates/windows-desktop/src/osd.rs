//! Bulle Win32 sans navigateur. Toutes les opérations HWND passent par le fil de l'interface.
use std::{cell::RefCell, mem::size_of, sync::OnceLock};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::*;

pub struct Notice {
    pub title: &'static str,
    pub rgb: u32,
}

thread_local! {
    static STATE: RefCell<(HWND, Option<Notice>, u32)> = const { RefCell::new((HWND(std::ptr::null_mut()), None, 96)) };
}

const CLASS: PCWSTR = w!("RustudioNativeOsd");
const HIDE: usize = 1;
const WIDTH: i32 = 168;
const HEIGHT: i32 = 34;
const MARGIN: i32 = 14;
const CORNER: i32 = 14;
const DISPLAY_MS: u32 = 1050;

fn colour(rgb: u32) -> COLORREF {
    COLORREF(((rgb & 255) << 16) | (rgb & 0xff00) | (rgb >> 16))
}

fn scale(n: i32, dpi: u32) -> i32 {
    n * dpi as i32 / 96
}

fn placement(work: RECT, dpi: u32) -> RECT {
    let margin = scale(MARGIN, dpi);
    let right = work.right - margin;
    let bottom = work.bottom - margin;
    RECT {
        left: right - scale(WIDTH, dpi),
        top: bottom - scale(HEIGHT, dpi),
        right,
        bottom,
    }
}

pub fn show(notice: Notice) {
    unsafe {
        let hwnd = STATE.with_borrow(|state| state.0);
        let hwnd = if hwnd.is_invalid() {
            let Ok(module) = GetModuleHandleW(None) else {
                return;
            };
            let instance = HINSTANCE(module.0);
            let wc = WNDCLASSW {
                lpfnWndProc: Some(message),
                hInstance: instance,
                lpszClassName: CLASS,
                ..Default::default()
            };
            static REGISTERED: OnceLock<bool> = OnceLock::new();
            if !REGISTERED.get_or_init(|| RegisterClassW(&wc) != 0) {
                return;
            }
            let Ok(hwnd) = CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_LAYERED,
                CLASS,
                w!("Rustudio"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                None,
            ) else {
                return;
            };
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 236, LWA_ALPHA);
            STATE.with_borrow_mut(|state| state.0 = hwnd);
            hwnd
        } else {
            hwnd
        };

        let monitor = MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return;
        }
        let (mut dpi, mut dpi_y) = (96, 96);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut dpi_y);
        let rect = placement(info.rcWork, dpi);
        let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
        STATE.with_borrow_mut(|state| {
            state.1 = Some(notice);
            state.2 = dpi;
        });
        let corner = scale(CORNER, dpi);
        let mask = CreateRoundRectRgn(0, 0, width + 1, height + 1, corner, corner);
        if !mask.is_invalid() && SetWindowRgn(hwnd, Some(mask), false) == 0 {
            let _ = DeleteObject(HGDIOBJ(mask.0));
        }
        let _ = InvalidateRect(Some(hwnd), None, false);
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            rect.left,
            rect.top,
            width,
            height,
            SWP_SHOWWINDOW | SWP_NOACTIVATE,
        );
        if SetTimer(Some(hwnd), HIDE, DISPLAY_MS, None) == 0 {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
}

unsafe extern "system" fn message(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut paint);
            STATE.with_borrow(|state| {
                if let Some(notice) = &state.1 {
                    draw(hwnd, dc, notice, state.2);
                }
            });
            let _ = EndPaint(hwnd, &paint);
            LRESULT(0)
        }
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_TIMER if wp.0 == HIDE => {
            let _ = KillTimer(Some(hwnd), HIDE);
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe fn draw(hwnd: HWND, dc: HDC, notice: &Notice, dpi: u32) {
    let s = |n: i32| scale(n, dpi);
    let mut client = RECT::default();
    let _ = GetClientRect(hwnd, &mut client);
    let bg = CreateSolidBrush(colour(0x202126));
    FillRect(dc, &client, bg);
    let _ = DeleteObject(HGDIOBJ(bg.0));

    let corner = s(CORNER);
    let outline = CreateRoundRectRgn(0, 0, client.right + 1, client.bottom + 1, corner, corner);
    let edge = CreateSolidBrush(colour(0x3b3d47));
    let _ = FrameRgn(dc, outline, edge, 1, 1);
    let _ = DeleteObject(HGDIOBJ(edge.0));
    let _ = DeleteObject(HGDIOBJ(outline.0));

    let dot = CreateEllipticRgn(s(11), s(14), s(17), s(20));
    let accent = CreateSolidBrush(colour(notice.rgb));
    let _ = FillRgn(dc, dot, accent);
    let _ = DeleteObject(HGDIOBJ(dot.0));
    let _ = DeleteObject(HGDIOBJ(accent.0));
    SetBkMode(dc, TRANSPARENT);
    let rect = RECT { left: s(25), right: client.right - s(9), ..client };
    text(dc, notice.title, rect, s(12));
}

unsafe fn text(
    dc: HDC,
    value: &str,
    mut rect: RECT,
    size: i32,
) {
    let font = CreateFontW(
        -size,
        0,
        0,
        0,
        500,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        CLEARTYPE_QUALITY,
        0,
        w!("Segoe UI"),
    );
    let old = SelectObject(dc, HGDIOBJ(font.0));
    SetTextColor(dc, colour(0xe8e9ee));
    let mut value = value.encode_utf16().collect::<Vec<_>>();
    DrawTextW(
        dc,
        &mut value,
        &mut rect,
        DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS | DT_LEFT,
    );
    SelectObject(dc, old);
    let _ = DeleteObject(HGDIOBJ(font.0));
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indicateur_ancre_en_bas_a_droite_de_la_zone_utile() {
        let primary = placement(RECT { left: 0, top: 0, right: 1920, bottom: 1040 }, 96);
        assert_eq!((primary.left, primary.top, primary.right, primary.bottom), (1738, 992, 1906, 1026));
        let secondary = placement(RECT { left: -2560, top: 40, right: 0, bottom: 1400 }, 144);
        assert_eq!((secondary.left, secondary.top, secondary.right, secondary.bottom), (-273, 1328, -21, 1379));
    }
}
