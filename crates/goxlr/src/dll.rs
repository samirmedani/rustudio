//! Pilote officiel : API Thesycon de goxlr_audioapi_x64.dll, chargee a l'execution.
use crate::Tusb;
use libloading::{Library, Symbol};
use std::ffi::CStr;
use std::os::raw::c_char;

type FnVoid = unsafe extern "system" fn() -> u32;
type FnOpen = unsafe extern "system" fn(u32, *mut u32) -> u32;
type FnInst = unsafe extern "system" fn(u32, *mut u16, u32) -> u32;
type FnVendor = unsafe extern "system" fn(u32, u32, u32, u32, u32, u16, u16, *mut u8, *mut u8, u32) -> u32;
type FnClose = unsafe extern "system" fn(u32) -> u32;
type FnStatus = unsafe extern "system" fn(u32) -> *const c_char;
type FnDrvStats = unsafe extern "system" fn(u32, *mut u8, u32) -> u32;

pub struct Dll {
    lib: Library,
    /// statistiques du pilote (2,7 Mo), allouees a la premiere lecture
    stats: Vec<u8>,
    has_stats: bool,
}

pub fn default_path() -> String {
    if let Ok(p) = std::env::var("RUSTUDIO_GOXLR_DLL") {
        return p;
    }
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
    format!(r"{pf}\TC-Helicon\GoXLR_Audio_Driver\x64\goxlr_audioapi_x64.dll")
}

impl Dll {
    pub fn load(path: &str) -> Result<Dll, String> {
        // SAFETY : DLL du pilote officiel, chargee depuis son dossier d'installation.
        let lib = unsafe { Library::new(path) }.map_err(|e| format!("{path} : {e}"))?;
        // SAFETY : on verifie seulement la presence du symbole (absent des pilotes plus anciens : diagnostic eteint).
        let has_stats = unsafe { lib.get::<FnDrvStats>(b"TUSBAUDIO_QueryDriverStatistics") }.is_ok();
        let d = Dll { lib, stats: Vec::new(), has_stats };
        for name in [
            "TUSBAUDIO_EnumerateDevices", "TUSBAUDIO_GetDeviceCount", "TUSBAUDIO_OpenDeviceByIndex",
            "TUSBAUDIO_GetDeviceInstanceIdString", "TUSBAUDIO_ClassVendorRequestOut", "TUSBAUDIO_ClassVendorRequestIn",
            "TUSBAUDIO_CloseDevice", "TUSBAUDIO_StatusCodeStringA",
        ] {
            // SAFETY : on verifie seulement la presence du symbole.
            unsafe { d.lib.get::<FnVoid>(name.as_bytes()) }.map_err(|e| format!("{name} absent : {e}"))?;
        }
        Ok(d)
    }
    fn sym<T>(&self, name: &str) -> Symbol<'_, T> {
        // SAFETY : symboles verifies a l'ouverture, signatures de l'API Thesycon (stdcall = convention x64).
        unsafe { self.lib.get::<T>(name.as_bytes()).expect("symbole verifie") }
    }
}

impl Tusb for Dll {
    fn enumerate(&mut self) -> u32 {
        unsafe { self.sym::<FnVoid>("TUSBAUDIO_EnumerateDevices")() }
    }
    fn device_count(&mut self) -> u32 {
        unsafe { self.sym::<FnVoid>("TUSBAUDIO_GetDeviceCount")() }
    }
    fn open_by_index(&mut self, idx: u32) -> Result<u32, u32> {
        let mut h = 0u32;
        let code = unsafe { self.sym::<FnOpen>("TUSBAUDIO_OpenDeviceByIndex")(idx, &mut h) };
        if code == 0 { Ok(h) } else { Err(code) }
    }
    fn instance_id(&mut self, h: u32) -> Option<String> {
        let mut buf = [0u16; 256];
        let code = unsafe { self.sym::<FnInst>("TUSBAUDIO_GetDeviceInstanceIdString")(h, buf.as_mut_ptr(), 256) };
        if code != 0 {
            return None;
        }
        let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end]))
    }
    fn vendor_out(&mut self, h: u32, request: u32, data: &[u8]) -> u32 {
        let mut copy = if data.is_empty() { vec![0u8; 1] } else { data.to_vec() };
        let mut meta = [0u8; 64];
        unsafe {
            self.sym::<FnVendor>("TUSBAUDIO_ClassVendorRequestOut")(
                h, 1, 0, request, 0, 0, data.len() as u16, copy.as_mut_ptr(), meta.as_mut_ptr(), 64,
            )
        }
    }
    fn vendor_in(&mut self, h: u32, request: u32, len: usize) -> Result<Vec<u8>, u32> {
        let mut buf = vec![0u8; len.max(1)];
        let mut meta = [0u8; 64];
        let code = unsafe {
            self.sym::<FnVendor>("TUSBAUDIO_ClassVendorRequestIn")(
                h, 1, 0, request, 0, 0, len as u16, buf.as_mut_ptr(), meta.as_mut_ptr(), 64,
            )
        };
        if code != 0 {
            return Err(code);
        }
        let n = u32::from_le_bytes([meta[0], meta[1], meta[2], meta[3]]) as usize;
        buf.truncate(n.min(len));
        Ok(buf)
    }
    fn close(&mut self, h: u32) {
        unsafe { self.sym::<FnClose>("TUSBAUDIO_CloseDevice")(h) };
    }
    fn status_text(&self, code: u32) -> String {
        let p = unsafe { self.sym::<FnStatus>("TUSBAUDIO_StatusCodeStringA")(code) };
        if p.is_null() {
            return code.to_string();
        }
        unsafe { CStr::from_ptr(p) }.to_string_lossy().to_string()
    }
    fn stream_stats(&mut self, h: u32) -> Option<crate::stream::StreamStats> {
        if !self.has_stats {
            return None;
        }
        if self.stats.len() != crate::stream::DRIVER_STATS_SIZE {
            self.stats = vec![0u8; crate::stream::DRIVER_STATS_SIZE];
        }
        let buf = self.stats.as_mut_ptr();
        // SAFETY : tampon de la taille exacte attendue par le pilote 5.57 (verifiee par la DLL elle-meme).
        let code = unsafe { self.sym::<FnDrvStats>("TUSBAUDIO_QueryDriverStatistics")(h, buf, crate::stream::DRIVER_STATS_SIZE as u32) };
        if code != 0 {
            return None;
        }
        crate::stream::parse(&self.stats)
    }
}
