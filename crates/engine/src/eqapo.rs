//! Equalizer APO sur les voies GoXLR : etat, installation, branchement (memes cles que son Device Selector).
use crate::config::Config;
use crate::log::log;
use indexmap::IndexMap;
use rustudio_core::hpeq::{CONFIG_FILE, HP_CHANNELS};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

pub const EQAPO_URL: &str = "https://sourceforge.net/projects/equalizerapo/files/1.4.2/EqualizerAPO-x64-1.4.2.exe/download";
pub const EQAPO_SHA256: &str = "7403be7427bbe1936a40dded082829b6e217fc4f5990fee5cba501f0ae055afa"; // 1.4.2, 11 980 366 octets
pub const EQAPO_PRE: &str = "{EACD2258-FCAC-4FF4-B36D-419E924A6D79}";
pub const EQAPO_POST: &str = "{EC1CC9CE-FAED-4822-828A-82A81A6F018F}";
#[allow(dead_code)]
const RENDER_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render";
#[allow(dead_code)]
const FX: &str = "{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},";
#[allow(dead_code)]
const FX_SLOTS: [u32; 5] = [1, 2, 5, 6, 7]; // LFX, GFX, SFX, MFX, EFX
#[allow(dead_code)]
const FX_MULTI: [u32; 3] = [13, 14, 15];
#[allow(dead_code)]
const PROC_MODES: &str = "{d3993a3f-99c2-4402-b5ec-a92a0367664b},";
#[allow(dead_code)]
const DEFAULT_MODE: &str = "{C18E2F7E-933D-4965-B7D1-1EEF228D2AF3}";
#[allow(dead_code)]
const DISABLE_ENH: &str = "{1da5d803-d492-4edd-8c23-e0c0ffee7f0e},5";
#[allow(dead_code)]
const CHILD_APOS: &str = r"SOFTWARE\EqualizerAPO\Child APOs";
pub const NAME_PROP: &str = "{a45c254e-df1c-4efd-8020-67d146a850e0},2";
pub const DEVICE_PROP: &str = "{b3f8fa53-0004-438e-9003-51a46e139bfc},6";

#[derive(Clone, Debug)]
pub struct Endpoint {
    pub guid: String,
    pub installed: bool,
}

struct Inner {
    sim_endpoints: IndexMap<String, bool>,
    sim_apo: bool,
    setup: Option<serde_json::Map<String, Value>>,
    written: Option<String>,
    last_auto: Option<Instant>,
}

pub struct EqApo {
    pub sim: bool,
    dir: PathBuf,
    inner: Mutex<Inner>,
    changes: AtomicU64,
}

pub fn eqapo_dir() -> PathBuf {
    if let Ok(d) = std::env::var("RUSTUDIO_EQAPO_DIR") {
        return PathBuf::from(d);
    }
    let pf = std::env::var("ProgramW6432").or_else(|_| std::env::var("ProgramFiles")).unwrap_or_else(|_| r"C:\Program Files".into());
    PathBuf::from(pf).join("EqualizerAPO")
}

impl EqApo {
    pub fn new(cfg: &Config, simulated: bool) -> EqApo {
        let sim = simulated || !cfg!(windows);
        EqApo {
            sim,
            dir: if sim { cfg.data_dir.join("eqapo-sim") } else { eqapo_dir() },
            inner: Mutex::new(Inner {
                sim_endpoints: HP_CHANNELS.iter().map(|c| (c.to_string(), false)).collect(),
                sim_apo: false,
                setup: None,
                written: None,
                last_auto: None,
            }),
            changes: AtomicU64::new(0),
        }
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn changes(&self) -> u64 {
        self.changes.load(Ordering::Relaxed)
    }
    pub fn bump(&self) {
        self.changes.fetch_add(1, Ordering::Relaxed);
    }
    pub fn installed(&self) -> bool {
        if self.sim {
            return self.lock().sim_apo;
        }
        self.dir.join("EqualizerAPO.dll").exists()
    }
    pub fn setup_running(&self) -> bool {
        self.lock().setup.as_ref().and_then(|s| s.get("running")).and_then(|v| v.as_bool()).unwrap_or(false)
    }
    pub fn set_setup(&self, kv: &[(&str, Value)], reset: bool) {
        let mut g = self.lock();
        if reset || g.setup.is_none() {
            g.setup = Some(serde_json::Map::new());
        }
        let s = g.setup.as_mut().unwrap();
        for (k, v) in kv {
            s.insert(k.to_string(), v.clone());
        }
        drop(g);
        self.bump();
    }
    pub fn reset_written(&self) {
        self.lock().written = None;
    }
    pub fn auto_due(&self) -> bool {
        self.lock().last_auto.map(|t| t.elapsed().as_secs() > 600).unwrap_or(true)
    }
    pub fn mark_auto(&self) {
        self.lock().last_auto = Some(Instant::now());
    }

    /// Voies GoXLR presentes : nom -> guid, et si Equalizer APO y est branche.
    pub fn endpoints(&self) -> IndexMap<String, Endpoint> {
        if self.sim {
            return self.lock().sim_endpoints.iter().map(|(c, v)| (c.clone(), Endpoint { guid: format!("{{sim-{c}}}"), installed: *v })).collect();
        }
        #[cfg(windows)]
        {
            win::endpoints()
        }
        #[cfg(not(windows))]
        IndexMap::new()
    }

    pub fn install_endpoint(&self, guid: &str) -> Result<(), String> {
        if self.sim {
            let mut g = self.lock();
            for (c, v) in g.sim_endpoints.iter_mut() {
                if format!("{{sim-{c}}}") == guid {
                    *v = true;
                }
            }
            return Ok(());
        }
        #[cfg(windows)]
        {
            win::install_endpoint(guid).map_err(|e| e.to_string())
        }
        #[cfg(not(windows))]
        Ok(())
    }

    pub fn uninstall_endpoint(&self, guid: &str) -> Result<(), String> {
        if self.sim {
            let mut g = self.lock();
            for (c, v) in g.sim_endpoints.iter_mut() {
                if format!("{{sim-{c}}}") == guid {
                    *v = false;
                }
            }
            return Ok(());
        }
        #[cfg(windows)]
        {
            win::uninstall_endpoint(guid).map_err(|e| e.to_string())
        }
        #[cfg(not(windows))]
        Ok(())
    }

    /// Le service audio recharge les effets des voies (2 secondes de silence).
    pub fn restart_audio(&self) {
        if self.sim {
            return;
        }
        crate::win::powershell("Restart-Service -Name Audiosrv -Force");
    }

    /// config.txt d'Equalizer APO n'inclut plus que notre fichier (l'ancien est garde a cote).
    pub fn take_config(&self) {
        let cfg_dir = self.dir.join("config");
        let _ = std::fs::create_dir_all(&cfg_dir);
        let cfg = cfg_dir.join("config.txt");
        // fichier temporaire oublie par GoXLR Mini (inerte, mais trompeur)
        let _ = std::fs::remove_file(cfg_dir.join("goxlr-mini.txt.tmp"));
        let cur = std::fs::read(&cfg).map(|b| String::from_utf8_lossy(&b).trim_start_matches('\u{feff}').to_string()).unwrap_or_default();
        if cur.contains(&format!("Include: {CONFIG_FILE}")) {
            return;
        }
        let mut lines = vec!["# Gere par Rustudio (page Casque de l'appli).".to_string()];
        if !cur.trim().is_empty() {
            let backup = format!("config.rustudio-backup-{}.txt", chrono::Local::now().format("%Y%m%d-%H%M%S"));
            let _ = std::fs::write(cfg_dir.join(&backup), &cur);
            lines.push(format!("# Configuration precedente : {backup}"));
        }
        lines.push(format!("Include: {CONFIG_FILE}"));
        let _ = std::fs::write(&cfg, lines.join("\n") + "\n");
        log("Equalizer APO : configuration reprise par Rustudio");
    }

    pub fn write(&self, text: &str) -> bool {
        if self.lock().written.as_deref() == Some(text) {
            return false;
        }
        let cfg_dir = self.dir.join("config");
        if !cfg_dir.is_dir() {
            return false;
        }
        let path = cfg_dir.join(CONFIG_FILE);
        let tmp = cfg_dir.join(format!("{CONFIG_FILE}.tmp"));
        if std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, &path)).is_err() {
            return false; // Equalizer APO relit le fichier des qu'il change
        }
        self.lock().written = Some(text.into());
        self.bump();
        true
    }

    pub fn download_and_install(&self, step: &dyn Fn(&str)) -> Result<(), String> {
        if self.sim {
            step("Installation d'Equalizer APO…");
            std::thread::sleep(std::time::Duration::from_millis(200));
            self.lock().sim_apo = true;
            let _ = std::fs::create_dir_all(self.dir.join("config"));
            return Ok(());
        }
        step("Téléchargement d'Equalizer APO…");
        let work = std::env::temp_dir().join(format!("rustudio-eqapo-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&work);
        let exe = work.join("EqualizerAPO-x64-1.4.2.exe");
        let r = (|| -> Result<(), String> {
            let agent = crate::net::agent(std::time::Duration::from_secs(60), 10, &crate::net::ua());
            let resp = agent.get(EQAPO_URL).set("User-Agent", &format!("Rustudio/{}", crate::config::VERSION)).call().map_err(|e| e.to_string())?;
            let mut reader = resp.into_reader();
            let data = crate::updater::read_limited(&mut reader, 40 * 1024 * 1024).map_err(|_| "Téléchargement trop gros : installation refusée.".to_string())?;
            if crate::updater::sha256_hex(&data) != EQAPO_SHA256 {
                return Err("Empreinte d'Equalizer APO inattendue : installation refusée.".into());
            }
            std::fs::write(&exe, &data).map_err(|e| e.to_string())?;
            step("Installation d'Equalizer APO…");
            let mut child = crate::win::hidden(&exe.to_string_lossy()).arg("/S").spawn().map_err(|e| e.to_string())?;
            let t0 = Instant::now();
            loop {
                if let Ok(Some(_)) = child.try_wait() {
                    break;
                }
                if t0.elapsed().as_secs() > 300 {
                    let _ = child.kill();
                    return Err("L'installation d'Equalizer APO ne se termine pas.".into());
                }
                // l'installeur ouvre son Device Selector et l'attend : on fait son travail nous-memes
                if crate::win::processes().contains("deviceselector.exe") {
                    crate::win::run_hidden("taskkill", &["/f", "/im", "DeviceSelector.exe"]);
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            if !self.installed() {
                return Err("Equalizer APO ne s'est pas installé.".into());
            }
            log("Equalizer APO 1.4.2 installé");
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(&work);
        r
    }

    pub fn status(&self, live: bool) -> Value {
        let eps = self.endpoints();
        let g = self.lock();
        let active = g.written.as_deref().map(|w| w.contains("Filter") || w.contains("Preamp")).unwrap_or(false);
        let endpoints: serde_json::Map<String, Value> =
            HP_CHANNELS.iter().map(|c| (c.to_string(), eps.get(*c).map(|e| json!(e.installed)).unwrap_or(Value::Null))).collect();
        json!({"apo": if self.sim { g.sim_apo } else { self.dir.join("EqualizerAPO.dll").exists() },
               "endpoints": endpoints, "live": live, "active": active,
               "setup": g.setup.clone().map(Value::Object), "sim": self.sim})
    }
}

#[cfg(windows)]
mod win {
    use super::*;
    use winreg::enums::*;
    use winreg::{RegKey, RegValue};

    fn hklm() -> RegKey {
        RegKey::predef(HKEY_LOCAL_MACHINE)
    }

    pub fn endpoints() -> IndexMap<String, Endpoint> {
        let mut out = IndexMap::new();
        let Ok(root) = hklm().open_subkey_with_flags(RENDER_KEY, KEY_READ | KEY_WOW64_64KEY) else { return out };
        for guid in root.enum_keys().flatten() {
            let Ok(k) = root.open_subkey_with_flags(&guid, KEY_READ | KEY_WOW64_64KEY) else { continue };
            if k.get_value::<u32, _>("DeviceState").unwrap_or(0) != 1 {
                continue;
            }
            let Ok(p) = root.open_subkey_with_flags(format!(r"{guid}\Properties"), KEY_READ | KEY_WOW64_64KEY) else { continue };
            let (Ok(name), Ok(dev)) = (p.get_value::<String, _>(NAME_PROP), p.get_value::<String, _>(DEVICE_PROP)) else { continue };
            if !dev.to_lowercase().contains("goxlr") || !HP_CHANNELS.contains(&name.as_str()) {
                continue;
            }
            let mut inst = false;
            if let Ok(fx) = root.open_subkey_with_flags(format!(r"{guid}\FxProperties"), KEY_READ | KEY_WOW64_64KEY) {
                for s in FX_SLOTS {
                    if let Ok(v) = fx.get_value::<String, _>(format!("{FX}{s}")) {
                        let u = v.to_uppercase();
                        if u == EQAPO_PRE || u == EQAPO_POST {
                            inst = true;
                        }
                    }
                }
            }
            out.insert(name, Endpoint { guid, installed: inst });
        }
        out
    }

    fn multi_sz(items: &[&str]) -> RegValue {
        let mut w: Vec<u16> = vec![];
        for i in items {
            w.extend(i.encode_utf16());
            w.push(0);
        }
        w.push(0);
        RegValue { bytes: w.iter().flat_map(|u| u.to_le_bytes()).collect(), vtype: REG_MULTI_SZ }
    }

    /// Branche Equalizer APO sur une voie (mode SFX/EFX, ou LFX/GFX si le pilote n'a que ceux-la).
    /// Les valeurs d'origine sont gardees dans Child APOs pour pouvoir les remettre.
    pub fn install_endpoint(guid: &str) -> std::io::Result<()> {
        let fx = hklm().open_subkey_with_flags(format!(r"{RENDER_KEY}\{guid}\FxProperties"), KEY_READ | KEY_SET_VALUE | KEY_WOW64_64KEY)?;
        let get = |name: &str| fx.get_value::<String, _>(name).ok();
        let orig: IndexMap<u32, Option<String>> = FX_SLOTS.iter().map(|s| (*s, get(&format!("{FX}{s}")))).collect();
        if orig.values().flatten().any(|v| v.to_uppercase() == EQAPO_PRE || v.to_uppercase() == EQAPO_POST) {
            return Ok(());
        }
        let o = |s: u32| orig.get(&s).cloned().flatten();
        let only_lg = (o(1).is_some() || o(2).is_some())
            && ![5, 6, 7].iter().any(|s| o(*s).is_some())
            && !FX_MULTI.iter().any(|s| get(&format!("{FX}{s}")).is_some());
        let nv = |s: u32| o(s).unwrap_or_else(|| "!VALUE".into());
        let (mut pre, mut post) = if only_lg || (o(5).is_none() && o(7).is_none()) { (nv(1), nv(2)) } else { (nv(5), nv(7)) };
        if pre.starts_with('!') {
            pre.clear();
        }
        if post.starts_with('!') {
            post.clear();
        }
        let (ch, _) = hklm().create_subkey_with_flags(format!(r"{CHILD_APOS}\{guid}"), KEY_ALL_ACCESS | KEY_WOW64_64KEY)?;
        for s in FX_SLOTS {
            ch.set_value(format!("{FX}{s}"), &nv(s))?;
        }
        ch.set_value("PreMixChild", &pre)?;
        ch.set_value("PostMixChild", &post)?;
        ch.set_value("AllowSilentBufferModification", &"false")?;
        ch.set_value("Version", &"2")?;
        let drop = |name: &str| {
            let _ = fx.delete_value(name);
        };
        if only_lg {
            fx.set_value(format!("{FX}1"), &EQAPO_PRE)?;
            fx.set_value(format!("{FX}2"), &EQAPO_POST)?;
            for s in [5, 6, 7] {
                drop(&format!("{FX}{s}"));
            }
        } else {
            drop(&format!("{FX}1"));
            drop(&format!("{FX}2"));
            fx.set_value(format!("{FX}5"), &EQAPO_PRE)?;
            if fx.get_raw_value(format!("{PROC_MODES}5")).is_err() {
                fx.set_raw_value(format!("{PROC_MODES}5"), &multi_sz(&[DEFAULT_MODE]))?;
            }
            fx.set_value(format!("{FX}7"), &EQAPO_POST)?;
            if fx.get_raw_value(format!("{PROC_MODES}7")).is_err() {
                fx.set_raw_value(format!("{PROC_MODES}7"), &multi_sz(&[DEFAULT_MODE]))?;
            }
        }
        drop(DISABLE_ENH);
        Ok(())
    }

    /// Remet les effets d'origine de la voie (ceux gardes a l'installation).
    pub fn uninstall_endpoint(guid: &str) -> std::io::Result<()> {
        let Ok(ch) = hklm().open_subkey_with_flags(format!(r"{CHILD_APOS}\{guid}"), KEY_READ | KEY_WOW64_64KEY) else { return Ok(()) };
        let orig: Vec<(u32, String)> =
            FX_SLOTS.iter().map(|s| (*s, ch.get_value::<String, _>(format!("{FX}{s}")).unwrap_or_else(|_| "!VALUE".into()))).collect();
        drop(ch);
        let fx = hklm().open_subkey_with_flags(format!(r"{RENDER_KEY}\{guid}\FxProperties"), KEY_READ | KEY_SET_VALUE | KEY_WOW64_64KEY)?;
        for (s, v) in orig {
            if v.starts_with('!') {
                let _ = fx.delete_value(format!("{FX}{s}"));
            } else {
                fx.set_value(format!("{FX}{s}"), &v)?;
            }
        }
        hklm().delete_subkey_with_flags(format!(r"{CHILD_APOS}\{guid}"), KEY_WOW64_64KEY)
    }
}
