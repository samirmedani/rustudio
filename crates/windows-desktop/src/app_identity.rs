//! Fenêtre active et icônes des applications choisies par l'utilisateur.
#[cfg(windows)]
pub use win::{foreground_executable, icon_bgra};

#[cfg(not(windows))]
pub fn foreground_executable() -> Option<std::path::PathBuf> {
    None
}

#[cfg(not(windows))]
pub fn icon_bgra(_path: &std::path::Path) -> Option<Vec<u8>> {
    None
}

pub const ICON_SIZE: u32 = 32;

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn extrait_l_icone_locale_d_un_executable() {
        let path = PathBuf::from(std::env::var_os("WINDIR").unwrap()).join("explorer.exe");
        let pixels = icon_bgra(&path).unwrap();
        assert_eq!(pixels.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
        assert!(pixels.chunks_exact(4).any(|pixel| pixel[3] > 0));
        assert!(pixels.chunks_exact(4).any(|pixel| pixel[3] == 0));
    }
}

#[cfg(windows)]
mod win {
    use super::ICON_SIZE;
    use std::{
        os::windows::ffi::{OsStrExt, OsStringExt},
        path::{Path, PathBuf},
    };
    use windows::{
        Win32::{
            Foundation::CloseHandle,
            Graphics::Gdi::{
                BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection,
                DIB_RGB_COLORS, DeleteDC, DeleteObject, SelectObject,
            },
            System::Threading::{
                OpenProcess, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
                QueryFullProcessImageNameW,
            },
            UI::{
                Shell::ExtractIconExW,
                WindowsAndMessaging::{
                    DI_NORMAL, DestroyIcon, DrawIconEx, GetForegroundWindow,
                    GetWindowThreadProcessId, HICON,
                },
            },
        },
        core::{PCWSTR, PWSTR},
    };

    pub fn foreground_executable() -> Option<PathBuf> {
        let hwnd = unsafe { GetForegroundWindow() };
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
        }
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
        let mut buf = vec![0u16; 32768];
        let mut len = buf.len() as u32;
        let result = unsafe {
            let result = QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_FORMAT(0),
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            );
            let _ = CloseHandle(process);
            result
        };
        result.ok()?;
        Some(PathBuf::from(std::ffi::OsString::from_wide(
            &buf[..len as usize],
        )))
    }

    pub fn icon_bgra(path: &Path) -> Option<Vec<u8>> {
        let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut icon = HICON::default();
        unsafe {
            ExtractIconExW(PCWSTR(path.as_ptr()), 0, Some(&mut icon), None, 1);
            if icon.is_invalid() {
                return None;
            }
            let pixels = draw_icon(icon);
            let _ = DestroyIcon(icon);
            pixels
        }
    }

    unsafe fn draw_icon(icon: HICON) -> Option<Vec<u8>> {
        let size = ICON_SIZE as i32;
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        unsafe {
            let dc = CreateCompatibleDC(None);
            if dc.is_invalid() {
                return None;
            }
            let mut pixels = std::ptr::null_mut();
            let result = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut pixels, None, 0)
                .ok()
                .and_then(|bitmap| {
                    let count = (size * size * 4) as usize;
                    let result = if pixels.is_null() {
                        None
                    } else {
                        std::ptr::write_bytes(pixels.cast::<u8>(), 0, count);
                        let previous = SelectObject(dc, bitmap.into());
                        let drawn = DrawIconEx(dc, 0, 0, icon, size, size, 0, None, DI_NORMAL);
                        SelectObject(dc, previous);
                        drawn.ok().map(|_| {
                            std::slice::from_raw_parts(pixels.cast::<u8>(), count).to_vec()
                        })
                    };
                    let _ = DeleteObject(bitmap.into());
                    result
                });
            let _ = DeleteDC(dc);
            result
        }
    }
}
