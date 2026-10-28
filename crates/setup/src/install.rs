//! Installation de Rustudio : fichiers, service, raccourcis, entree « Applications », passage depuis GoXLR Mini.
//! En passage depuis GoXLR Mini, l'ancienne version n'est retiree qu'une fois Rustudio en marche (et la GoXLR
//! reprise si Windows la voit) ; sinon tout est remis comme avant.
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

pub const PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/payload.zip"));
pub const NAME: &str = "Rustudio";
const LEGACY: &str = "GoXLR Mini";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Rustudio";
const LEGACY_UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\GoXLRMini";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// le service se demarre aussi depuis le compte de l'utilisateur (l'appli le relance s'il s'arrete)
const SERVICE_SDDL: &str = "D:(A;;CCLCSWRPWPDTLOCRRC;;;SY)(A;;CCDCLCSWRPWPDTLOCRSDRCWDWO;;;BA)(A;;CCLCSWLOCRRC;;;IU)(A;;CCLCSWLOCRRC;;;SU)(A;;RPLCLOCRRC;;;AU)";

#[derive(Clone, Debug)]
pub struct Opts {
    pub desktop: bool,
    pub startup: bool,
    pub launch: bool,
    pub quiet: bool,
    /// comptes dont on reprend l'appli (passage depuis GoXLR Mini lance par le compte systeme)
    pub for_users: Vec<String>,
}

pub fn program_files() -> PathBuf {
    PathBuf::from(std::env::var("ProgramW6432").or_else(|_| std::env::var("ProgramFiles")).unwrap_or_else(|_| r"C:\Program Files".into()))
}
pub fn install_dir() -> PathBuf {
    program_files().join(NAME)
}
pub fn legacy_dir() -> PathBuf {
    program_files().join(LEGACY)
}
pub fn payload_version() -> Option<String> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(PAYLOAD)).ok()?;
    let mut f = z.by_name("manifest.json").ok()?;
    let mut s = String::new();
    std::io::Read::read_to_string(&mut f, &mut s).ok()?;
    serde_json::from_str::<Value>(&s).ok()?["version"].as_str().map(String::from)
}
pub fn legacy_present() -> bool {
    legacy_dir().join("goxlrctl.py").exists()
}
pub fn installed_version() -> Option<String> {
    let t = std::fs::read_to_string(install_dir().join("manifest.json")).ok()?;
    serde_json::from_str::<Value>(&t).ok()?["version"].as_str().map(String::from)
}

fn hidden(cmd: &str) -> Command {
    let mut c = Command::new(cmd);
    use std::os::windows::process::CommandExt;
    c.creation_flags(0x0800_0000);
    c
}
fn run(cmd: &str, args: &[&str]) -> bool {
    hidden(cmd).args(args).status().map(|s| s.success()).unwrap_or(false)
}
pub fn powershell(script: &str) -> String {
    let full = format!("$ProgressPreference = 'SilentlyContinue'; [Console]::OutputEncoding = [Text.Encoding]::UTF8; {script}");
    let utf16: Vec<u8> = full.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    let b64 = b64(&utf16);
    hidden("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &b64])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}
fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}
fn q(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn health() -> Option<Value> {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(3)).build().get("http://127.0.0.1:14565/api/health").call().ok()?.into_json().ok()
}

/// Arrete ce qui tourne : Rustudio (service, appli) et GoXLR Mini (tache, moteur Python, appli).
fn stop_all() {
    run("sc", &["stop", NAME]);
    run("schtasks", &["/end", "/tn", LEGACY]);
    let dirs = [install_dir(), legacy_dir()];
    let pat = dirs.iter().map(|d| q(&format!("{}\\", d.display()))).collect::<Vec<_>>().join(",");
    powershell(&format!(
        "$d = @({pat}); Get-CimInstance Win32_Process | Where-Object {{ $p = $_.ExecutablePath; $p -and ($d | Where-Object {{ $p.StartsWith($_, [StringComparison]::OrdinalIgnoreCase) }}) }} | ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }}"
    ));
    std::thread::sleep(Duration::from_millis(800));
}

fn extract(dest: &Path, step: &dyn Fn(&str, f64)) -> Result<Vec<String>, String> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(PAYLOAD)).map_err(|_| "Installeur incomplet : archive absente ou abîmée.".to_string())?;
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let n = z.len();
    let mut files = vec![];
    for i in 0..n {
        let mut f = z.by_index(i).map_err(|e| e.to_string())?;
        let name = f.name().replace('\\', "/");
        let parts: Vec<&str> = name.split('/').filter(|p| !p.is_empty()).collect();
        if name.starts_with('/') || parts.iter().any(|p| *p == ".." || p.contains(':')) {
            return Err(format!("Chemin refusé dans l'archive : {name}"));
        }
        let target = parts.iter().fold(dest.to_path_buf(), |a, p| a.join(p));
        if name.ends_with('/') {
            let _ = std::fs::create_dir_all(&target);
            continue;
        }
        if let Some(p) = target.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        // un exe encore ouvert ne se remplace pas, mais se renomme : on l'ecarte d'abord
        if target.exists() && std::fs::remove_file(&target).is_err() {
            let _ = std::fs::rename(&target, target.with_extension("old"));
        }
        let mut out = std::fs::File::create(&target).map_err(|e| format!("{} : {e}", target.display()))?;
        std::io::copy(&mut f, &mut out).map_err(|e| e.to_string())?;
        files.push(name);
        step("Copie des fichiers…", 0.15 + 0.35 * (i + 1) as f64 / n as f64);
    }
    Ok(files)
}

/// Compte cible : SID et dossier du profil (raccourcis, demarrage avec la session).
struct User {
    sid: String,
    profile: Option<PathBuf>,
}
fn user_of(name: &str) -> Option<User> {
    let out = powershell(&format!(
        "$s = (New-Object Security.Principal.NTAccount({})).Translate([Security.Principal.SecurityIdentifier]).Value; $p = (Get-ItemProperty \"HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\ProfileList\\$s\" -ErrorAction SilentlyContinue).ProfileImagePath; \"$s|$p\"",
        q(name)
    ));
    let (sid, prof) = out.trim().split_once('|')?;
    (!sid.is_empty()).then(|| User { sid: sid.into(), profile: (!prof.is_empty()).then(|| PathBuf::from(prof)) })
}
fn is_system() -> bool {
    powershell("[Security.Principal.WindowsIdentity]::GetCurrent().IsSystem").trim().eq_ignore_ascii_case("true")
}
fn current_user() -> Option<User> {
    let out = powershell("[Security.Principal.WindowsIdentity]::GetCurrent().User.Value + '|' + $env:USERPROFILE");
    let (sid, prof) = out.trim().split_once('|')?;
    Some(User { sid: sid.into(), profile: Some(PathBuf::from(prof)) })
}
/// Bureau reel du compte (redirige vers OneDrive, par exemple), lu dans sa ruche.
fn desktop_of(u: &User) -> Option<PathBuf> {
    use winreg::enums::*;
    let k = winreg::RegKey::predef(HKEY_USERS).open_subkey(format!(r"{}\Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders", u.sid)).ok();
    let raw: Option<String> = k.and_then(|k| k.get_raw_value("Desktop").ok()).map(|v| {
        let w: Vec<u16> = v.bytes.chunks(2).map(|c| u16::from_le_bytes([c[0], *c.get(1).unwrap_or(&0)])).take_while(|c| *c != 0).collect();
        String::from_utf16_lossy(&w)
    });
    match (raw, &u.profile) {
        (Some(r), Some(p)) => Some(PathBuf::from(r.replace("%USERPROFILE%", &p.to_string_lossy()))),
        (None, Some(p)) => Some(p.join("Desktop")),
        _ => None,
    }
}

fn shortcut(lnk: &Path, exe: &Path) {
    powershell(&format!(
        "$s = (New-Object -ComObject WScript.Shell).CreateShortcut({}); $s.TargetPath = {}; $s.WorkingDirectory = {}; $s.IconLocation = {}; $s.Description = 'Rustudio'; $s.Save()",
        q(&lnk.to_string_lossy()), q(&exe.to_string_lossy()), q(&exe.parent().unwrap().to_string_lossy()), q(&format!("{},0", exe.display()))
    ));
}

fn common_programs() -> PathBuf {
    PathBuf::from(std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into())).join(r"Microsoft\Windows\Start Menu\Programs")
}

fn run_key(u: &User, on: bool, exe: &Path) {
    use winreg::enums::*;
    let hku = winreg::RegKey::predef(HKEY_USERS);
    if let Ok((k, _)) = hku.create_subkey(format!(r"{}\{RUN_KEY}", u.sid)) {
        let _ = k.delete_value(LEGACY);
        if on {
            let _ = k.set_value(NAME, &format!("\"{}\" --tray", exe.display()));
        } else {
            let _ = k.delete_value(NAME);
        }
    }
}

fn register_uninstall(version: &str, users: &[String]) {
    use winreg::enums::*;
    let dir = install_dir();
    if let Ok((k, _)) = winreg::RegKey::predef(HKEY_LOCAL_MACHINE).create_subkey(UNINSTALL_KEY) {
        let vals = [
            ("DisplayName", NAME.to_string()), ("DisplayVersion", version.to_string()), ("Publisher", "Rustudio (logiciel libre)".into()),
            ("DisplayIcon", dir.join("Rustudio.exe").to_string_lossy().to_string()), ("InstallLocation", dir.to_string_lossy().to_string()),
            ("URLInfoAbout", "https://github.com/samirmedani/rustudio".into()), ("InstalledFor", users.join(";")),
            ("UninstallString", format!("\"{}\" uninstall", dir.join("rustudio-engine.exe").display())),
        ];
        for (n, v) in vals {
            let _ = k.set_value(n, &v);
        }
        let _ = k.set_value("NoModify", &1u32);
        let _ = k.set_value("NoRepair", &1u32);
    }
}

/// Retire GoXLR Mini (ses reglages restent dans ProgramData, deja repris par Rustudio).
/// La tache « GoXLR Mini Update » reste : en passage depuis GoXLR Mini 4.x, c'est elle qui fait tourner le pont
/// et cet installeur. Le moteur Rustudio la supprime une fois le pont termine (`Updater::cleanup_legacy_update`).
fn remove_legacy(users: &[User]) -> bool {
    use winreg::enums::*;
    for t in [LEGACY, "GoXLR Mini (premier lancement)", "GoXLR Mini (relance)"] {
        run("schtasks", &["/end", "/tn", t]);
        run("schtasks", &["/delete", "/tn", t, "/f"]);
    }
    let _ = winreg::RegKey::predef(HKEY_LOCAL_MACHINE).delete_subkey_all(LEGACY_UNINSTALL_KEY);
    let _ = std::fs::remove_file(common_programs().join(format!("{LEGACY}.lnk")));
    let mut had_desktop = false;
    for u in users {
        if let Some(d) = desktop_of(u) {
            had_desktop |= std::fs::remove_file(d.join(format!("{LEGACY}.lnk"))).is_ok();
        }
        if let Some(p) = &u.profile {
            had_desktop |= std::fs::remove_file(p.join("Desktop").join(format!("{LEGACY}.lnk"))).is_ok();
        }
    }
    let _ = std::fs::remove_dir_all(legacy_dir());
    had_desktop
}

fn launch_app(users: &[String], exe: &Path, tray: bool) {
    if users.is_empty() {
        // installeur lance par l'utilisateur (eleve) : explorer ouvre l'appli sans les droits administrateur
        let _ = hidden("explorer.exe").arg(exe).spawn();
        return;
    }
    for u in users {
        powershell(&format!(
            "$t = 'Rustudio (premier lancement)'; $a = New-ScheduledTaskAction -Execute {}{}; $p = New-ScheduledTaskPrincipal -UserId {} -LogonType Interactive -RunLevel Limited; Register-ScheduledTask -TaskName $t -Action $a -Principal $p -Force | Out-Null; Start-ScheduledTask -TaskName $t; Start-Sleep 3; Unregister-ScheduledTask -TaskName $t -Confirm:$false",
            q(&exe.to_string_lossy()), if tray { " -Argument '--tray'" } else { "" }, q(u)
        ));
    }
}

/// Attend le moteur ; si Windows voit la GoXLR, attend aussi qu'il la reprenne.
fn wait_engine(version: &str, step: &dyn Fn(&str, f64)) -> Result<Value, String> {
    let t0 = Instant::now();
    let mut seen: Option<Instant> = None;
    while t0.elapsed() < Duration::from_secs(60) {
        std::thread::sleep(Duration::from_millis(700));
        let Some(h) = health() else { continue };
        if h["version"].as_str() != Some(version) {
            continue;
        }
        let s = *seen.get_or_insert_with(Instant::now);
        step("Connexion à la GoXLR…", 0.8 + 0.1 * (s.elapsed().as_secs_f64() / 25.0).min(1.0));
        if h["connected"] == Value::Bool(true) {
            return Ok(h);
        }
        let usb_sees_it = h["usb"].as_str() == Some("mini");
        if !usb_sees_it && s.elapsed() > Duration::from_secs(8) {
            return Ok(h); // GoXLR eteinte ou debranchee : Rustudio l'attendra
        }
        if usb_sees_it && s.elapsed() > Duration::from_secs(25) {
            return Err(format!("Rustudio voit la GoXLR mais n'arrive pas à la piloter : {}", h["last_error"].as_str().unwrap_or("erreur inconnue")));
        }
    }
    Err("Le moteur Rustudio ne répond pas.".into())
}

pub fn install(o: &Opts, step: &dyn Fn(&str, f64)) -> Result<Value, String> {
    let version = payload_version().ok_or("Installeur incomplet : archive absente ou abîmée.")?;
    let dir = install_dir();
    let exe = dir.join("Rustudio.exe");
    let legacy = legacy_present();
    step("Arrêt de la version en place…", 0.08);
    stop_all();
    let r = (|| -> Result<Value, String> {
        extract(&dir, step)?;
        step("Service Rustudio…", 0.6);
        let engine = dir.join("rustudio-engine.exe");
        let ok = hidden(&engine.to_string_lossy()).arg("service-install").status().map(|s| s.success()).unwrap_or(false);
        if !ok {
            return Err("Le service Rustudio ne s'installe pas.".into());
        }
        run("sc", &["sdset", NAME, SERVICE_SDDL]);
        step("Démarrage du moteur…", 0.75);
        wait_engine(&version, step)
    })();
    let h = match r {
        Ok(h) => h,
        Err(e) => {
            if legacy {
                // retour a GoXLR Mini, comme avant
                let _ = hidden(&dir.join("rustudio-engine.exe").to_string_lossy()).arg("service-uninstall").status();
                stop_all();
                let _ = std::fs::remove_dir_all(&dir);
                run("schtasks", &["/run", "/tn", LEGACY]);
                let old_app = legacy_dir().join("app").join("GoXLR Mini.exe");
                if old_app.exists() && !o.for_users.is_empty() {
                    launch_app(&o.for_users, &old_app, true);
                }
            }
            return Err(e);
        }
    };
    step("Raccourcis…", 0.92);
    // installeur lance par toi : ton compte ; passage silencieux depuis le compte systeme : les comptes indiques
    let users: Vec<User> = if !o.for_users.is_empty() {
        o.for_users.iter().filter_map(|u| user_of(u)).collect()
    } else if o.quiet && is_system() {
        vec![]
    } else {
        current_user().into_iter().collect()
    };
    let had_desktop = if legacy { remove_legacy(&users) } else { false };
    let _ = std::fs::create_dir_all(common_programs());
    shortcut(&common_programs().join(format!("{NAME}.lnk")), &exe);
    for u in &users {
        if o.desktop || had_desktop {
            if let Some(d) = desktop_of(u).filter(|d| d.is_dir()) {
                shortcut(&d.join(format!("{NAME}.lnk")), &exe);
            }
        }
        run_key(u, o.startup, &exe);
    }
    register_uninstall(&version, &o.for_users);
    if o.launch && !(o.quiet && o.for_users.is_empty() && is_system()) {
        launch_app(&o.for_users, &exe, o.quiet);
    }
    step("Terminé", 1.0);
    Ok(serde_json::json!({"version": version, "connected": h["connected"], "migrated": legacy}))
}
