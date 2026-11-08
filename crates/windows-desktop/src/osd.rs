//! Bulle Win32 sans navigateur. Toutes les opérations HWND passent par le fil de l'interface.
use std::{cell::RefCell, mem::size_of, sync::OnceLock};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::*;

pub struct Notice {
    pub title: String,
    pub subtitle: String,
    pub rgb: u32,
    pub icon: &'static str,
}

thread_local! {
    static STATE: RefCell<(HWND, Option<Notice>, u32)> = const { RefCell::new((HWND(std::ptr::null_mut()), None, 96)) };
}

const CLASS: PCWSTR = w!("RustudioNativeOsd");
const HIDE: usize = 1;

fn colour(rgb: u32) -> COLORREF {
    COLORREF(((rgb & 255) << 16) | (rgb & 0xff00) | (rgb >> 16))
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
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 244, LWA_ALPHA);
            STATE.with_borrow_mut(|state| state.0 = hwnd);
            hwnd
        } else {
            hwnd
        };

        let monitor = MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return;
        }
        let (mut dpi, mut dpi_y) = (96, 96);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut dpi_y);
        let scale = |n: i32| n * dpi as i32 / 96;
        let (width, height) = (scale(330), scale(74));
        let x = info.rcMonitor.left + (info.rcMonitor.right - info.rcMonitor.left - width) / 2;
        let y = info.rcMonitor.bottom - height - scale(116);
        STATE.with_borrow_mut(|state| {
            state.1 = Some(notice);
            state.2 = dpi;
        });
        let mask = CreateRoundRectRgn(0, 0, width + 1, height + 1, scale(44), scale(44));
        if !mask.is_invalid() && SetWindowRgn(hwnd, Some(mask), false) == 0 {
            let _ = DeleteObject(HGDIOBJ(mask.0));
        }
        let _ = InvalidateRect(Some(hwnd), None, false);
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            x,
            y,
            width,
            height,
            SWP_SHOWWINDOW | SWP_NOACTIVATE,
        );
        if SetTimer(Some(hwnd), HIDE, 1650, None) == 0 {
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

const MIC: &[u8] = include_bytes!("../assets/osd-mic.rgba");
const MIC_OFF: &[u8] = include_bytes!("../assets/osd-micoff.rgba");
const GLYPH: usize = 48;

/// Pastille ronde lissée (couleur de l'état) avec le pictogramme blanc, en BGRA prémultiplié.
fn badge_pixels(n: usize, rgb: u32, glyph: &[u8]) -> Vec<u8> {
    let channel = |shift: u32| ((rgb >> shift) & 255) as f32;
    let (r, g, b) = (channel(16), channel(8), channel(0));
    let centre = n as f32 / 2.;
    let size = n as f32 * 0.52;
    let start = centre - size / 2.;
    let glyph_alpha = |x: f32, y: f32| -> f32 {
        let (gx, gy) = ((x - start) / size * GLYPH as f32, (y - start) / size * GLYPH as f32);
        if gx < 0. || gy < 0. || gx >= GLYPH as f32 || gy >= GLYPH as f32 {
            return 0.;
        }
        f32::from(glyph[(gy as usize * GLYPH + gx as usize) * 4 + 3]) / 255.
    };
    let mut out = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let (mut cover, mut ink) = (0f32, 0f32);
            for sy in 0..4 {
                for sx in 0..4 {
                    let (px, py) = (x as f32 + (sx as f32 + 0.5) / 4., y as f32 + (sy as f32 + 0.5) / 4.);
                    if (px - centre).powi(2) + (py - centre).powi(2) <= centre * centre {
                        cover += 1.;
                        ink += glyph_alpha(px, py);
                    }
                }
            }
            let (cover, ink) = (cover / 16., if cover > 0. { ink / cover } else { 0. });
            let mix = |c: f32| (c * (1. - ink) + 255. * ink) * cover;
            let i = (y * n + x) * 4;
            out[i] = mix(b) as u8;
            out[i + 1] = mix(g) as u8;
            out[i + 2] = mix(r) as u8;
            out[i + 3] = (255. * cover) as u8;
        }
    }
    out
}

unsafe fn badge(dc: HDC, x: i32, y: i32, n: i32, rgb: u32, glyph: &[u8]) {
    let pixels = badge_pixels(n as usize, rgb, glyph);
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: n,
            biHeight: -n,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let memory = CreateCompatibleDC(Some(dc));
    let mut bits = std::ptr::null_mut();
    let Ok(bitmap) = CreateDIBSection(Some(memory), &info, DIB_RGB_COLORS, &mut bits, None, 0) else {
        let _ = DeleteDC(memory);
        return;
    };
    std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u8, pixels.len());
    let old = SelectObject(memory, HGDIOBJ(bitmap.0));
    let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
    let _ = AlphaBlend(dc, x, y, n, n, memory, 0, 0, n, n, blend);
    SelectObject(memory, old);
    let _ = DeleteObject(HGDIOBJ(bitmap.0));
    let _ = DeleteDC(memory);
}

unsafe fn draw(hwnd: HWND, dc: HDC, notice: &Notice, dpi: u32) {
    let s = |n: i32| n * dpi as i32 / 96;
    let mut client = RECT::default();
    let _ = GetClientRect(hwnd, &mut client);
    let bg = CreateSolidBrush(colour(0xfbfbfd));
    FillRect(dc, &client, bg);
    let _ = DeleteObject(HGDIOBJ(bg.0));
    // filet clair sur le contour arrondi
    let outline = CreateRoundRectRgn(0, 0, client.right + 1, client.bottom + 1, s(44), s(44));
    let edge = CreateSolidBrush(colour(0xd9d9de));
    let _ = FrameRgn(dc, outline, edge, 1, 1);
    let _ = DeleteObject(HGDIOBJ(edge.0));
    let _ = DeleteObject(HGDIOBJ(outline.0));
    badge(dc, s(15), s(15), s(44), notice.rgb, if notice.icon == "mic-off" { MIC_OFF } else { MIC });
    SetBkMode(dc, TRANSPARENT);
    let rect = |x, y, end, bottom| RECT { left: s(x), top: s(y), right: s(end), bottom: s(bottom) };
    text(dc, &notice.title, rect(72, 13, 318, 40), s(16), 600, 0x1d1d1f, DT_LEFT);
    text(dc, &notice.subtitle, rect(72, 39, 318, 60), s(13), 400, 0x6e6e73, DT_LEFT);
}

unsafe fn text(
    dc: HDC,
    value: &str,
    mut rect: RECT,
    size: i32,
    weight: i32,
    rgb: u32,
    align: DRAW_TEXT_FORMAT,
) {
    let font = CreateFontW(
        -size,
        0,
        0,
        0,
        weight,
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
    SetTextColor(dc, colour(rgb));
    let mut value = value.encode_utf16().collect::<Vec<_>>();
    DrawTextW(
        dc,
        &mut value,
        &mut rect,
        DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS | align,
    );
    SelectObject(dc, old);
    let _ = DeleteObject(HGDIOBJ(font.0));
}
