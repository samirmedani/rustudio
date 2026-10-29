//! Ce qui touche a Windows : processus, liste USB, taches planifiees, PowerShell sans fenetre.
//! Hors de Windows (developpement, tests), des equivalents simples ou rien.
use std::collections::HashSet;
use std::process::{Command, Output};

pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn hidden(cmd: &str) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(cmd);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

pub fn run_hidden(cmd: &str, args: &[&str]) -> Option<Output> {
    hidden(cmd).args(args).output().ok()
}

/// Script PowerShell sans fenetre (encode en base64 : aucun souci de guillemets) ; sortie en UTF-8.
pub fn powershell(script: &str) -> String {
    let full = format!(
        "$ProgressPreference = 'SilentlyContinue'; [Console]::OutputEncoding = [Text.Encoding]::UTF8; {script}"
    );
    let utf16: Vec<u8> = full.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    let b64 = base64(&utf16);
    run_hidden("powershell.exe", &["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &b64])
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}

pub fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Noms (minuscules) des processus en cours.
pub fn processes() -> HashSet<String> {
    #[cfg(windows)]
    {
        win_processes()
    }
    #[cfg(not(windows))]
    {
        let out = Command::new("ps").args(["-axo", "comm="]).output();
        out.map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.trim().rsplit('/').next().unwrap_or("").to_lowercase())
                .collect()
        })
        .unwrap_or_default()
    }
}

/// PID -> nom d'exe (en minuscules) de tous les processus.
pub fn process_ids() -> std::collections::HashMap<u32, String> {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut out = std::collections::HashMap::new();
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE || snap.is_null() {
            return out;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut e);
        while ok != 0 {
            let end = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(e.szExeFile.len());
            out.insert(e.th32ProcessID, String::from_utf16_lossy(&e.szExeFile[..end]).to_lowercase());
            ok = Process32NextW(snap, &mut e);
        }
        CloseHandle(snap);
    }
    out
}

#[cfg(windows)]
fn win_processes() -> HashSet<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
    let mut names = HashSet::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE || snap.is_null() {
            return names;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut e);
        while ok != 0 {
            let end = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(e.szExeFile.len());
            names.insert(String::from_utf16_lossy(&e.szExeFile[..end]).to_lowercase());
            ok = Process32NextW(snap, &mut e);
        }
        CloseHandle(snap);
    }
    names
}

/// PID et chemin complet des processus portant ce nom d'exe.
#[cfg(windows)]
pub fn processes_named(exe: &str) -> Vec<(u32, String)> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
    use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
    let mut out = vec![];
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE || snap.is_null() {
            return out;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut e);
        while ok != 0 {
            let end = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(e.szExeFile.len());
            let name = String::from_utf16_lossy(&e.szExeFile[..end]);
            if name.eq_ignore_ascii_case(exe) {
                let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, e.th32ProcessID);
                if !h.is_null() {
                    let mut buf = [0u16; 1024];
                    let mut n = 1024u32;
                    if QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut n) != 0 {
                        out.push((e.th32ProcessID, String::from_utf16_lossy(&buf[..n as usize])));
                    }
                    CloseHandle(h);
                }
            }
            ok = Process32NextW(snap, &mut e);
        }
        CloseHandle(snap);
    }
    out
}
#[cfg(not(windows))]
pub fn processes_named(_exe: &str) -> Vec<(u32, String)> {
    vec![]
}

pub fn kill_pid(pid: u32) {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
        let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if !h.is_null() {
            TerminateProcess(h, 1);
            CloseHandle(h);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("kill").args(["-9", &pid.to_string()]).output();
    }
}

pub fn pid_alive(pid: u32) -> bool {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
        use windows_sys::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return false;
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code) != 0 && code == STILL_ACTIVE as u32;
        CloseHandle(h);
        ok
    }
    #[cfg(not(windows))]
    {
        Command::new("kill").args(["-0", &pid.to_string()]).status().map(|s| s.success()).unwrap_or(false)
    }
}

/// Ce que Windows voit sur l'USB, sans passer par le pilote GoXLR (aucun risque pour ses handles) :
/// "mini", "full" (GoXLR complete, non geree), "failed" (un appareil n'arrive pas a se presenter), "none".
pub fn usb_scan() -> Option<String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
        let usb: Vec<u16> = "USB\0".encode_utf16().collect();
        let flags = CM_GETIDLIST_FILTER_ENUMERATOR | CM_GETIDLIST_FILTER_PRESENT;
        let mut size = 0u32;
        unsafe {
            if CM_Get_Device_ID_List_SizeW(&mut size, usb.as_ptr(), flags) != CR_SUCCESS || size == 0 {
                return None;
            }
            let mut buf = vec![0u16; size as usize];
            if CM_Get_Device_ID_ListW(usb.as_ptr(), buf.as_mut_ptr(), size, flags) != CR_SUCCESS {
                return None;
            }
            let ids: Vec<String> =
                String::from_utf16_lossy(&buf).split('\0').filter(|s| !s.is_empty()).map(|s| s.to_uppercase()).collect();
            return Some(classify_usb(&ids));
        }
    }
    #[cfg(not(windows))]
    None
}

pub fn classify_usb(ids: &[String]) -> String {
    use rustudio_core::consts::{USB_FULL, USB_MINI};
    if ids.iter().any(|i| i.starts_with(USB_MINI)) {
        "mini".into()
    } else if ids.iter().any(|i| i.starts_with(USB_FULL)) {
        "full".into()
    } else if ids.iter().any(|i| i.starts_with("USB\\VID_0000&") || i.contains("DESCRIPTOR_FAILURE")) {
        "failed".into()
    } else {
        "none".into()
    }
}

/// Tache SYSTEM lancee une fois (installation d'une mise a jour hors de l'arbre du service).
pub fn schtasks_run_system(name: &str, exe: &str, args: &str) -> Result<(), String> {
    let tr = format!("\"{exe}\" {args}");
    let mk = run_hidden("schtasks", &["/create", "/tn", name, "/tr", &tr, "/sc", "once", "/st", "23:59", "/ru", "SYSTEM", "/rl", "HIGHEST", "/f"]);
    if !mk.map(|o| o.status.success()).unwrap_or(false) {
        return Err("Impossible de préparer l’installation (tâche planifiée).".into());
    }
    let run = run_hidden("schtasks", &["/run", "/tn", name]);
    if !run.map(|o| o.status.success()).unwrap_or(false) {
        return Err("Impossible de lancer l’installation (tâche planifiée).".into());
    }
    Ok(())
}

pub fn schtasks_delete(name: &str) {
    let _ = run_hidden("schtasks", &["/delete", "/tn", name, "/f"]);
}
