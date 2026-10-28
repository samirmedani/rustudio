//! Desinstallation (« Parametres › Applications » lance « rustudio-engine.exe uninstall »).
//! Le moteur se copie dans le dossier temporaire pour pouvoir effacer son propre dossier, puis retire service,
//! fichiers, raccourcis, demarrage avec la session et entree « Applications ». Les reglages restent (sauf --remove-data).
use std::path::PathBuf;
use std::time::Duration;

fn is_admin() -> bool {
    unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() != 0 }
}

fn shell_execute(exe: &str, args: &str, elevate: bool) -> bool {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    let w = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (verb, file, params) = (w(if elevate { "runas" } else { "open" }), w(exe), w(args));
    let r = unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), params.as_ptr(), std::ptr::null(), 0) };
    r as isize > 32
}

fn message(text: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};
    let w = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    unsafe { MessageBoxW(std::ptr::null_mut(), w(text).as_ptr(), w("Rustudio").as_ptr(), MB_OK | MB_ICONINFORMATION) };
}

pub fn run(args: &[String]) -> i32 {
    let has = |a: &str| args.iter().any(|x| x == a);
    let exe = std::env::current_exe().unwrap_or_default();
    if !has("--from-temp") {
        let dir = exe.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let tmp = std::env::temp_dir().join("rustudio-uninstall.exe");
        if std::fs::copy(&exe, &tmp).is_err() {
            return 1;
        }
        let a = format!(
            "uninstall --from-temp --dir \"{}\"{}{}",
            dir.display(),
            if has("--remove-data") { " --remove-data" } else { "" },
            if has("--quiet") { " --quiet" } else { "" }
        );
        return if shell_execute(&tmp.to_string_lossy(), &a, !is_admin()) { 0 } else { 1 };
    }
    let dir = args.iter().position(|a| a == "--dir").and_then(|i| args.get(i + 1)).map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("ProgramFiles").unwrap_or_default()).join("Rustudio"));
    if !dir.ends_with("Rustudio") {
        return 1; // garde-fou : on n'efface que le dossier de Rustudio
    }
    let _ = crate::service::uninstall();
    let prefix = dir.to_string_lossy().to_lowercase();
    for name in ["Rustudio.exe", "rustudio-engine.exe"] {
        for (pid, path) in crate::win::processes_named(name) {
            if path.to_lowercase().starts_with(&prefix) {
                crate::win::kill_pid(pid);
            }
        }
    }
    std::thread::sleep(Duration::from_millis(500));
    crate::win::powershell(
        "Remove-Item (Join-Path ([Environment]::GetFolderPath('CommonPrograms')) 'Rustudio.lnk') -Force -ErrorAction SilentlyContinue; \
         Remove-Item (Join-Path ([Environment]::GetFolderPath('Desktop')) 'Rustudio.lnk') -Force -ErrorAction SilentlyContinue; \
         Remove-ItemProperty 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run' -Name 'Rustudio' -ErrorAction SilentlyContinue; \
         Remove-Item 'HKLM:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Rustudio' -Recurse -Force -ErrorAction SilentlyContinue; \
         foreach ($t in 'Rustudio Update', 'Rustudio (relance)', 'Rustudio (premier lancement)') { schtasks /delete /tn $t /f 2>$null | Out-Null }",
    );
    for _ in 0..10 {
        if std::fs::remove_dir_all(&dir).is_ok() || !dir.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    let data = crate::config::default_data_dir();
    if has("--remove-data") {
        let _ = std::fs::remove_dir_all(&data);
    }
    if !has("--quiet") {
        message(&if has("--remove-data") {
            "Rustudio est désinstallé.".to_string()
        } else {
            format!("Rustudio est désinstallé. Tes réglages restent dans {}.", data.display())
        });
    }
    0
}
