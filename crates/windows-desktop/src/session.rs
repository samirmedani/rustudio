//! Session Windows de l'appli : une seule instance, démarrage avec Windows, relance du service.
use std::time::Duration;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Appli lancée avec la session (clé Run du compte, comme l'installeur).
pub fn startup_enabled() -> bool {
    #[cfg(windows)]
    {
        use winreg::enums::*;
        winreg::RegKey::predef(HKEY_CURRENT_USER).open_subkey(RUN_KEY)
            .and_then(|k| k.get_value::<String, _>("Rustudio")).is_ok()
    }
    #[cfg(not(windows))]
    false
}

pub fn set_startup(on: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        use winreg::enums::*;
        let (k, _) = winreg::RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_KEY).map_err(|e| e.to_string())?;
        if on {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            k.set_value("Rustudio", &format!("\"{}\" --tray", exe.display())).map_err(|e| e.to_string())
        } else {
            match k.delete_value("Rustudio") {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
                _ => Ok(()),
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = on;
        Err("Disponible sous Windows seulement.".into())
    }
}

/// Demande au service Rustudio de démarrer (autorisé par l'installeur pour le compte de l'utilisateur).
pub fn start_service() {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("sc").args(["start", "Rustudio"]).creation_flags(0x0800_0000).spawn();
    }
}

pub fn pid_alive(pid: u32) -> bool {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return false };
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code).is_ok() && code == 259;
        let _ = CloseHandle(h);
        ok
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        false
    }
}

/// Attend la fin d'une ancienne instance (relance après mise à jour).
pub fn wait_exit(pid: u32, timeout: Duration) {
    let t0 = std::time::Instant::now();
    while t0.elapsed() < timeout && pid_alive(pid) {
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Une seule appli par session. La seconde ouverture réveille la première puis s'arrête.
#[cfg(windows)]
pub struct Instance {
    _mutex: windows::Win32::Foundation::HANDLE,
    event: windows::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
unsafe impl Send for Instance {}

#[cfg(windows)]
impl Instance {
    /// `None` : une autre instance tourne déjà ; elle a été prévenue.
    pub fn acquire(name: &str) -> Option<Self> {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
        use windows::Win32::System::Threading::{CreateEventW, CreateMutexW, SetEvent};
        let w = |s: String| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        let (m, e) = (w(format!("Local\\{name}.Instance")), w(format!("Local\\{name}.Show")));
        unsafe {
            let event = CreateEventW(None, false, false, PCWSTR(e.as_ptr())).ok()?;
            let mutex = CreateMutexW(None, true, PCWSTR(m.as_ptr())).ok()?;
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = SetEvent(event);
                return None;
            }
            Some(Self { _mutex: mutex, event })
        }
    }

    /// Bloque jusqu'à la prochaine ouverture demandée par une autre instance.
    pub fn wait_show(&self) -> bool {
        use windows::Win32::Foundation::WAIT_OBJECT_0;
        use windows::Win32::System::Threading::{WaitForSingleObject, INFINITE};
        unsafe { WaitForSingleObject(self.event, INFINITE) == WAIT_OBJECT_0 }
    }
}

/// Coins arrondis et ombre natifs de Windows 11 pour une fenêtre sans cadre (sans effet sous Windows 10).
#[cfg(windows)]
pub fn round_corners(hwnd: isize) {
    use windows::Win32::Graphics::Dwm::{DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute};
    let preference = DWMWCP_ROUND;
    unsafe {
        let _ = DwmSetWindowAttribute(
            windows::Win32::Foundation::HWND(hwnd as _),
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &preference as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&preference) as u32,
        );
    }
}
