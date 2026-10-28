//! Echange des fichiers d'une mise a jour (tache « Rustudio Update », moteur de la NOUVELLE version).
//! Sauvegarde ce qui est en place, copie la nouvelle version, verifie qu'elle repond ; sinon retour arriere.
use crate::config::now_epoch;
use crate::log::log;
use crate::logf;
use crate::updater::read_json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Plan {
    pub from: PathBuf,
    pub to: PathBuf,
    pub version: String,
    pub from_version: String,
    pub data_dir: PathBuf,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_service")]
    pub service: String,
}
fn default_port() -> u16 {
    crate::config::DEFAULT_PORT
}
fn default_service() -> String {
    crate::config::SERVICE_NAME.into()
}

pub trait Hooks {
    fn stop(&self, plan: &Plan);
    fn start(&self, plan: &Plan);
    fn wait(&self, plan: &Plan) -> bool;
    fn list_apps(&self, plan: &Plan) -> Vec<(u32, String)>;
    fn restart_apps(&self, plan: &Plan, before: Vec<(u32, String)>);
}

fn files_of(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = read_json(&dir.join("manifest.json"))
        .and_then(|m| m.get("files").and_then(|f| f.as_object()).map(|o| o.keys().cloned().collect()))
        .unwrap_or_default();
    v.sort();
    v
}

fn rel_path(base: &Path, rel: &str) -> PathBuf {
    rel.split('/').filter(|p| !p.is_empty() && *p != ".." && *p != ".").fold(base.to_path_buf(), |acc, p| acc.join(p))
}

fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(p) = to.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::rename(from, to)
}

pub fn apply_update(plan: &Plan, hooks: &dyn Hooks) -> Result<Value, String> {
    let (src, dst) = (&plan.from, &plan.to);
    let new_files = files_of(src);
    let old_files = files_of(dst);
    if new_files.is_empty() {
        return Err("Manifeste sans liste de fichiers.".into());
    }
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let backup = dst.join("backup").join(format!("{stamp}-{}", plan.from_version));
    let (mut moved, mut copied) = (vec![], vec![]);
    let mut result = json!({"version": plan.version, "from_version": plan.from_version, "t": now_epoch()});
    let apps = hooks.list_apps(plan);
    hooks.stop(plan);
    let mut all: Vec<String> = old_files.iter().chain(new_files.iter()).cloned().collect();
    all.sort();
    all.dedup();
    let r = (|| -> Result<(), String> {
        for rel in &all {
            let cur = rel_path(dst, rel);
            if cur.is_file() {
                move_file(&cur, &rel_path(&backup, rel)).map_err(|e| format!("{rel} : {e}"))?;
                moved.push(rel.clone());
            }
        }
        for rel in &new_files {
            let to = rel_path(dst, rel);
            if let Some(p) = to.parent() {
                std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            std::fs::copy(rel_path(src, rel), &to).map_err(|e| format!("{rel} : {e}"))?;
            copied.push(rel.clone());
        }
        hooks.start(plan);
        if !hooks.wait(plan) {
            return Err("la nouvelle version ne répond pas".into());
        }
        Ok(())
    })();
    match &r {
        Ok(()) => result["ok"] = json!(true),
        Err(e) => {
            result["ok"] = json!(false);
            result["error"] = json!(e);
            hooks.stop(plan);
            let failed = dst.join("backup").join(format!("echec-{}", plan.version));
            for rel in &copied {
                let p = rel_path(dst, rel);
                if p.is_file() {
                    let _ = move_file(&p, &rel_path(&failed, rel));
                }
            }
            for rel in &moved {
                let _ = move_file(&rel_path(&backup, rel), &rel_path(dst, rel));
            }
            hooks.start(plan);
        }
    }
    // on garde les deux dernieres sauvegardes
    if let Ok(rd) = std::fs::read_dir(dst.join("backup")) {
        let mut dirs: Vec<String> =
            rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.starts_with(|c: char| c.is_ascii_digit())).collect();
        dirs.sort();
        let n = dirs.len();
        for d in dirs.iter().take(n.saturating_sub(2)) {
            let _ = std::fs::remove_dir_all(dst.join("backup").join(d));
        }
    }
    if r.is_ok() {
        set_display_version(&plan.version);
        hooks.restart_apps(plan, apps);
    }
    let _ = std::fs::create_dir_all(plan.data_dir.join("update"));
    let _ = std::fs::write(plan.data_dir.join("update").join("result.json"), result.to_string());
    Ok(result)
}

fn set_display_version(_v: &str) {
    #[cfg(windows)]
    {
        use winreg::enums::*;
        if let Ok(k) = winreg::RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Rustudio", KEY_SET_VALUE)
        {
            let _ = k.set_value("DisplayVersion", &_v);
        }
    }
}

/// Point d'entree de la tache « Rustudio Update » : lit le plan prepare par le moteur.
pub fn run_apply_update() -> i32 {
    crate::log::set_prefix("[mise à jour] ");
    let base = crate::config::exe_dir(); // <donnees>/update/new
    let work = base.parent().map(Path::to_path_buf).unwrap_or(base.clone());
    let plan: Option<Plan> = read_json(&work.join("plan.json")).and_then(|v| serde_json::from_value(v).ok());
    let Some(plan) = plan else {
        crate::log::set_file(work.parent().unwrap_or(&work).join("rustudio.log"));
        logf!("Installation impossible : plan.json introuvable dans {}", work.display());
        return 1;
    };
    crate::log::set_file(plan.data_dir.join("rustudio.log"));
    logf!("Installation de {} (depuis {})", plan.version, plan.from_version);
    match apply_update(&plan, &WinHooks) {
        Ok(r) if r["ok"] == json!(true) => {
            logf!("Version {} installée", plan.version);
            0
        }
        Ok(r) => {
            logf!("Échec, retour à {} : {}", plan.from_version, r["error"].as_str().unwrap_or(""));
            1
        }
        Err(e) => {
            logf!("Échec avant l’échange des fichiers : {e}");
            1
        }
    }
}

pub struct WinHooks;

fn engine_pids(root: &Path) -> Vec<u32> {
    let prefix = root.to_string_lossy().to_lowercase();
    crate::win::processes_named("rustudio-engine.exe")
        .into_iter()
        .filter(|(_, p)| p.to_lowercase().starts_with(&prefix))
        .map(|(pid, _)| pid)
        .collect()
}

impl Hooks for WinHooks {
    fn stop(&self, plan: &Plan) {
        crate::win::run_hidden("sc", &["stop", &plan.service]);
        // arret propre d'abord : un service tue en plein arret se croit en panne et Windows le relancerait
        // pendant l'echange des fichiers
        for _ in 0..30 {
            if engine_pids(&plan.to).is_empty() {
                return;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        for _ in 0..10 {
            let pids = engine_pids(&plan.to);
            if pids.is_empty() {
                return;
            }
            for pid in pids {
                crate::win::kill_pid(pid);
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }
    fn start(&self, plan: &Plan) {
        crate::win::run_hidden("sc", &["start", &plan.service]);
    }
    fn wait(&self, plan: &Plan) -> bool {
        let url = format!("http://127.0.0.1:{}/api/health", plan.port);
        let agent = crate::net::agent(Duration::from_secs(3), 5, &crate::net::ua());
        let t0 = std::time::Instant::now();
        while t0.elapsed() < Duration::from_secs(45) {
            if let Ok(r) = agent.get(&url).call() {
                if let Ok(v) = r.into_json::<Value>() {
                    if v.get("version").and_then(|x| x.as_str()) == Some(plan.version.as_str()) {
                        return true;
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        false
    }
    /// Appli Rustudio ouverte depuis le dossier installe : [(pid, "DOMAINE\\compte")].
    fn list_apps(&self, plan: &Plan) -> Vec<(u32, String)> {
        if !cfg!(windows) {
            return vec![];
        }
        let out = crate::win::powershell(
            "Get-CimInstance Win32_Process -Filter \"Name='Rustudio.exe'\" | ForEach-Object { \
             $o = Invoke-CimMethod -InputObject $_ -MethodName GetOwner; '{0}|{1}\\{2}|{3}' -f $_.ProcessId, $o.Domain, $o.User, $_.ExecutablePath }",
        );
        let prefix = plan.to.to_string_lossy().to_lowercase();
        out.lines()
            .filter_map(|l| {
                let p: Vec<&str> = l.trim().splitn(3, '|').collect();
                (p.len() == 3 && p[2].to_lowercase().starts_with(&prefix) && p[1] != "\\").then(|| (p[0].parse().ok(), p[1].to_string()))
            })
            .filter_map(|(pid, u)| pid.map(|pid| (pid, u)))
            .collect()
    }
    /// L'appli se relance seule quand le moteur annonce la nouvelle version ; si elle reste sur l'ancien exe, on la
    /// ferme et on rouvre la nouvelle dans la session de son compte.
    fn restart_apps(&self, plan: &Plan, before: Vec<(u32, String)>) {
        if before.is_empty() {
            return;
        }
        let t0 = std::time::Instant::now();
        let mut stuck = before;
        while !stuck.is_empty() && t0.elapsed() < Duration::from_secs(12) {
            std::thread::sleep(Duration::from_secs(1));
            stuck.retain(|(pid, _)| crate::win::pid_alive(*pid));
        }
        let exe = plan.to.join("Rustudio.exe").to_string_lossy().to_string();
        let mut users: Vec<String> = vec![];
        for (pid, user) in stuck {
            crate::win::kill_pid(pid);
            if !users.contains(&user) {
                users.push(user);
            }
        }
        for user in users {
            crate::win::powershell(&format!(
                "$t = 'Rustudio (relance)'; $a = New-ScheduledTaskAction -Execute {} -Argument '--tray'; \
                 $p = New-ScheduledTaskPrincipal -UserId {} -LogonType Interactive -RunLevel Limited; \
                 Register-ScheduledTask -TaskName $t -Action $a -Principal $p -Force | Out-Null; \
                 Start-ScheduledTask -TaskName $t; Start-Sleep 3; Unregister-ScheduledTask -TaskName $t -Confirm:$false",
                crate::win::ps_quote(&exe), crate::win::ps_quote(&user)
            ));
            log("Appli relancée après la mise à jour");
        }
    }
}
