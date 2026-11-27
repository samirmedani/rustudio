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
const CAPTURE_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Capture";
/// format du moteur audio d'une entrée (WAVEFORMATEX après 8 octets d'en-tête) : nombre de voies à l'octet 10
#[allow(dead_code)]
const FORMAT_PROP: &str = "{f19f064d-082c-4e27-bc73-6882a1bb8e4c},0";
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
/// Entrées du mode simulé : le micro de la GoXLR et un micro USB quelconque (module Micro).
pub const SIM_GOXLR_MIC: &str = "{00000000-0000-0000-0000-00000000c0de}";
pub const SIM_USB_MIC: &str = "{00000000-0000-0000-0000-0000000005b0}";

#[derive(Clone, Debug)]
pub struct Endpoint {
    pub guid: String,
    pub installed: bool,
}

/// Entrée micro de Windows : identifiant, nom affiché, nombre de voies, Equalizer APO branché ou non.
#[derive(Clone, Debug)]
pub struct Capture {
    pub guid: String,
    pub name: String,
    pub channels: u16,
    pub installed: bool,
    /// micro de la GoXLR (« Chat Mic » du pilote officiel, micro du firmware Rustudio)
    pub goxlr: bool,
}

struct Inner {
    sim_endpoints: IndexMap<String, bool>,
    sim_apo: bool,
    setup: Option<serde_json::Map<String, Value>>,
    written: Option<String>,
    last_auto: Option<Instant>,
    /// dernière sortie par défaut sur laquelle on a tenté de brancher Equalizer APO
    tried_default: Option<String>,
    written_mic: Option<String>,
    /// Dernière configuration micro demandée (conservée pendant une pause de mesure vocale).
    pending_mic: Option<String>,
    /// Pause temporaire de RNNoise pendant la capture de l'empreinte vocale.
    mic_bypass: bool,
    /// dernier rustudio-micro-voix.txt écrit (module Micro)
    written_voice: Option<String>,
    /// entrées simulées sur lesquelles Equalizer APO est branché
    sim_captures: Vec<String>,
    sim_plugin: bool,
}

pub struct EqApo {
    pub sim: bool,
    dir: PathBuf,
    /// dossier du module RNNoise (données de Rustudio)
    plugin_dir: PathBuf,
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
            plugin_dir: cfg.data_dir.join("rnnoise"),
            inner: Mutex::new(Inner {
                sim_endpoints: HP_CHANNELS.iter().map(|c| (c.to_string(), false)).collect(),
                sim_apo: false,
                setup: None,
                written: None,
                last_auto: None,
                tried_default: None,
                written_mic: None,
                pending_mic: None,
                mic_bypass: false,
                written_voice: None,
                sim_captures: vec![],
                sim_plugin: false,
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
    /// Opération en cours ou dernière opération (installation, branchement…).
    pub fn setup_value(&self) -> Option<Value> {
        self.lock().setup.clone().map(Value::Object)
    }
    pub fn reset_written(&self) {
        let mut g = self.lock();
        g.written = None;
        g.written_mic = None;
        g.written_voice = None;
    }
    pub fn auto_due(&self) -> bool {
        self.lock().last_auto.map(|t| t.elapsed().as_secs() > 600).unwrap_or(true)
    }
    /// Vrai la première fois pour cette sortie (pas de redémarrage du son en boucle si le branchement échoue).
    pub fn try_default(&self, channel: &str) -> bool {
        let mut g = self.lock();
        if g.tried_default.as_deref() == Some(channel) {
            return false;
        }
        g.tried_default = Some(channel.to_owned());
        true
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
        // casque, suppression du bruit puis voix du micro : le bruit part avant l'égaliseur, comme dans OBS
        let includes = [CONFIG_FILE, rustudio_core::micns::CONFIG_FILE, rustudio_windows_mic::CONFIG_FILE].map(|f| format!("Include: {f}"));
        let has = |inc: &str| cur.lines().any(|l| l.trim() == inc);
        if includes.iter().all(|inc| has(inc)) {
            return;
        }
        let mut lines = vec!["# Gere par Rustudio (pages Casque et Micro de l'appli).".to_string()];
        if includes.iter().any(|inc| has(inc)) {
            // déjà à Rustudio : on garde ses lignes et on ajoute les fichiers manquants
            lines = cur.lines().filter(|l| !l.starts_with("# Gere par Rustudio")).map(String::from).collect();
            lines.insert(0, "# Gere par Rustudio (pages Casque et Micro de l'appli).".into());
        } else if !cur.trim().is_empty() {
            let backup = format!("config.rustudio-backup-{}.txt", chrono::Local::now().format("%Y%m%d-%H%M%S"));
            let _ = std::fs::write(cfg_dir.join(&backup), &cur);
            lines.push(format!("# Configuration precedente : {backup}"));
        }
        for inc in includes {
            if !lines.iter().any(|l| l.trim() == inc) {
                lines.push(inc);
            }
        }
        let content = lines.join("\n") + "\n";
        let _ = write_atomic(&cfg, &cfg_dir.join("config.txt.tmp"), &content);
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
        if !write_atomic(&path, &tmp, text) {
            return false; // Equalizer APO relit le fichier des qu'il change
        }
        self.lock().written = Some(text.into());
        self.bump();
        true
    }

    /// Écrit rustudio-micro.txt (suppression du bruit du micro) s'il a changé.
    pub fn write_mic(&self, text: &str) -> bool {
        {
            let mut g = self.lock();
            g.pending_mic = Some(text.into());
            if g.mic_bypass || g.written_mic.as_deref() == Some(text) {
                return false;
            }
        }
        let cfg_dir = self.dir.join("config");
        if !cfg_dir.is_dir() {
            return false;
        }
        let name = rustudio_core::micns::CONFIG_FILE;
        let (path, tmp) = (cfg_dir.join(name), cfg_dir.join(format!("{name}.tmp")));
        if !write_atomic(&path, &tmp, text) {
            return false;
        }
        self.lock().written_mic = Some(text.into());
        self.bump();
        true
    }

    /// Écrit rustudio-micro-voix.txt (voix des micros, module Micro) s'il a changé.
    pub fn write_voice(&self, text: &str) -> bool {
        if self.lock().written_voice.as_deref() == Some(text) {
            return false;
        }
        let cfg_dir = self.dir.join("config");
        if !cfg_dir.is_dir() {
            return false;
        }
        let name = rustudio_windows_mic::CONFIG_FILE;
        if !write_atomic(&cfg_dir.join(name), &cfg_dir.join(format!("{name}.tmp")), text) {
            return false;
        }
        self.lock().written_voice = Some(text.into());
        self.bump();
        true
    }

    /// Met en pause RNNoise pendant une mesure vocale afin que Windows capture la voix brute et le vrai bruit
    /// de la pièce, puis restaure immédiatement la configuration dès la fin de la mesure.
    pub fn set_mic_bypass(&self, bypass: bool) {
        let restore = {
            let mut g = self.lock();
            if g.mic_bypass == bypass {
                return;
            }
            g.mic_bypass = bypass;
            if bypass {
                None
            } else {
                g.pending_mic.clone()
            }
        };
        if bypass {
            let cfg_dir = self.dir.join("config");
            if cfg_dir.is_dir() {
                let name = rustudio_core::micns::CONFIG_FILE;
                let (path, tmp) = (cfg_dir.join(name), cfg_dir.join(format!("{name}.tmp")));
                let paused = "# Rustudio : suppression du bruit du micro en pause (mesure vocale en cours).\n";
                if write_atomic(&path, &tmp, paused) {
                    self.lock().written_mic = Some(paused.into());
                    self.bump();
                }
            }
        } else if let Some(text) = restore {
            self.write_mic(&text);
        }
    }

    pub fn plugin_dir(&self) -> &std::path::Path {
        &self.plugin_dir
    }

    /// Le module RNNoise est-il en place ?
    pub fn plugin_installed(&self) -> bool {
        if self.sim {
            return self.lock().sim_plugin;
        }
        self.plugin_dir.join(rustudio_core::micns::PLUGIN_MONO).is_file() && self.plugin_dir.join(rustudio_core::micns::PLUGIN_STEREO).is_file()
    }

    /// Télécharge le module RNNoise (release GitHub, taille et SHA-256 vérifiées) et en garde les deux DLL VST.
    pub fn install_plugin(&self, step: &dyn Fn(&str)) -> Result<(), String> {
        use rustudio_core::micns::{PLUGIN_MONO, PLUGIN_SHA256, PLUGIN_SIZE, PLUGIN_STEREO, PLUGIN_URL};
        if self.sim {
            self.lock().sim_plugin = true;
            return Ok(());
        }
        if self.plugin_installed() {
            return Ok(());
        }
        step("Téléchargement de la suppression du bruit (RNNoise)…");
        let agent = crate::net::agent(std::time::Duration::from_secs(60), 10, &crate::net::ua());
        let resp = agent.get(PLUGIN_URL).call().map_err(|e| format!("Téléchargement impossible : {e}"))?;
        let mut reader = resp.into_reader();
        let data = crate::updater::read_limited(&mut reader, PLUGIN_SIZE + 1).ok_or("Téléchargement trop gros : installation refusée.")?;
        if data.len() as u64 != PLUGIN_SIZE || crate::updater::sha256_hex(&data) != PLUGIN_SHA256 {
            return Err("Le module téléchargé n’est pas celui attendu : installation refusée.".into());
        }
        step("Installation du module…");
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|_| "Archive du module illisible.".to_string())?;
        std::fs::create_dir_all(&self.plugin_dir).map_err(|e| e.to_string())?;
        for name in [PLUGIN_MONO, PLUGIN_STEREO] {
            let mut f = zip.by_name(&format!("win-rnnoise/vst/{name}")).map_err(|_| format!("{name} absent de l’archive."))?;
            let mut bytes = vec![];
            std::io::Read::read_to_end(&mut f, &mut bytes).map_err(|e| e.to_string())?;
            let tmp = self.plugin_dir.join(format!("{name}.tmp"));
            std::fs::write(&tmp, &bytes).and_then(|_| std::fs::rename(&tmp, self.plugin_dir.join(name))).map_err(|e| e.to_string())?;
        }
        log("Suppression du bruit : module RNNoise 1.22 installé");
        Ok(())
    }

    /// Entrées micro de Windows actives.
    pub fn captures(&self) -> Vec<Capture> {
        if self.sim {
            let on = self.lock().sim_captures.clone();
            return [(SIM_GOXLR_MIC, "Chat Mic (simulé)", true), (SIM_USB_MIC, "Micro USB (simulé)", false)]
                .into_iter()
                .map(|(guid, name, goxlr)| Capture { guid: guid.into(), name: name.into(), channels: 1, installed: on.iter().any(|g| g.eq_ignore_ascii_case(guid)), goxlr })
                .collect();
        }
        #[cfg(windows)]
        {
            win::captures()
        }
        #[cfg(not(windows))]
        vec![]
    }

    /// Branche Equalizer APO sur une entrée micro (sa clé d'effets appartient au service audio : on en prend
    /// d'abord la propriété, comme le Device Selector d'Equalizer APO).
    pub fn install_capture(&self, guid: &str) -> Result<(), String> {
        if self.sim {
            let mut g = self.lock();
            if !g.sim_captures.iter().any(|c| c.eq_ignore_ascii_case(guid)) {
                g.sim_captures.push(guid.to_lowercase());
            }
            return Ok(());
        }
        #[cfg(windows)]
        {
            win::own_capture_key(guid)?;
            win::install_capture(guid).map_err(|e| format!("Branchement sur le micro impossible : {e}"))
        }
        #[cfg(not(windows))]
        {
            let _ = guid;
            Ok(())
        }
    }

    pub fn uninstall_capture(&self, guid: &str) -> Result<(), String> {
        if self.sim {
            self.lock().sim_captures.retain(|c| !c.eq_ignore_ascii_case(guid));
            return Ok(());
        }
        #[cfg(windows)]
        {
            match win::uninstall_capture(guid) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                    win::own_capture_key(guid)?;
                    win::uninstall_capture(guid).map_err(|e| e.to_string())
                }
                Err(e) => Err(e.to_string()),
            }
        }
        #[cfg(not(windows))]
        {
            let _ = guid;
            Ok(())
        }
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
            let data = crate::updater::read_limited(&mut reader, 40 * 1024 * 1024).ok_or_else(|| "Téléchargement trop gros : installation refusée.".to_string())?;
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
            self.reset_written();
            log("Equalizer APO 1.4.2 installé");
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(&work);
        r
    }

    /// Désinstalle Equalizer APO de Windows (son désinstalleur, sans fenêtre), puis vérifie qu'il est parti.
    pub fn uninstall_software(&self) -> Result<(), String> {
        if self.sim {
            let mut g = self.lock();
            g.sim_apo = false;
            g.written = None;
            g.written_mic = None;
            g.written_voice = None;
            return Ok(());
        }
        let un = self.dir.join("Uninstall.exe");
        if !un.exists() {
            return Err("Désinstalleur d'Equalizer APO introuvable : retire-le depuis les Paramètres de Windows.".into());
        }
        crate::win::hidden(&un.to_string_lossy()).arg("/S").spawn().map_err(|e| e.to_string())?;
        // le désinstalleur se copie ailleurs et rend la main tout de suite : on attend que le composant disparaisse
        let t0 = Instant::now();
        while self.installed() {
            if t0.elapsed().as_secs() > 120 {
                return Err("La désinstallation d'Equalizer APO ne se termine pas.".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        self.reset_written();
        Ok(())
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

/// Écriture atomique via fichier temporaire, avec repli direct si le service audio Windows verrouille brièvement le rename.
fn write_atomic(path: &std::path::Path, tmp: &std::path::Path, text: &str) -> bool {
    if std::fs::write(tmp, text).and_then(|_| std::fs::rename(tmp, path)).is_ok() {
        return true;
    }
    let _ = std::fs::remove_file(tmp);
    std::fs::write(path, text).is_ok()
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
            // Firmware Rustudio : une sortie Windows par voie, à son nom (System : « Rustudio Mini Audio »).
            let name = if let Some(channel) = rustudio_core::hpeq::rustudio_channel(&dev) {
                channel.to_owned()
            } else if dev.to_lowercase().contains("goxlr") && HP_CHANNELS.contains(&name.as_str()) {
                name
            } else {
                continue;
            };
            if out.contains_key(&name) {
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
        install_on(RENDER_KEY, guid)
    }

    pub fn install_capture(guid: &str) -> std::io::Result<()> {
        install_on(CAPTURE_KEY, guid)
    }

    pub fn uninstall_capture(guid: &str) -> std::io::Result<()> {
        uninstall_on(CAPTURE_KEY, guid)
    }

    /// Entrées micro actives, avec leur nombre de voies (format du moteur audio) et l'état d'Equalizer APO.
    pub fn captures() -> Vec<Capture> {
        let mut out = vec![];
        let Ok(root) = hklm().open_subkey_with_flags(CAPTURE_KEY, KEY_READ | KEY_WOW64_64KEY) else { return out };
        for guid in root.enum_keys().flatten() {
            let Ok(k) = root.open_subkey_with_flags(&guid, KEY_READ | KEY_WOW64_64KEY) else { continue };
            if k.get_value::<u32, _>("DeviceState").unwrap_or(0) != 1 {
                continue;
            }
            let Ok(p) = root.open_subkey_with_flags(format!(r"{guid}\Properties"), KEY_READ | KEY_WOW64_64KEY) else { continue };
            let name = p.get_value::<String, _>(NAME_PROP).unwrap_or_default();
            let dev = p.get_value::<String, _>(DEVICE_PROP).unwrap_or_default();
            let channels = p.get_raw_value(FORMAT_PROP).ok().filter(|v| v.bytes.len() >= 12).map(|v| u16::from_le_bytes([v.bytes[10], v.bytes[11]])).unwrap_or(1);
            let mut installed = false;
            if let Ok(fx) = root.open_subkey_with_flags(format!(r"{guid}\FxProperties"), KEY_READ | KEY_WOW64_64KEY) {
                installed = FX_SLOTS.iter().any(|s| fx.get_value::<String, _>(format!("{FX}{s}")).is_ok_and(|v| [EQAPO_PRE, EQAPO_POST].contains(&v.to_uppercase().as_str())));
            }
            let goxlr = dev.eq_ignore_ascii_case(rustudio_core::hpeq::RUSTUDIO_DEVICE) || (dev.to_lowercase().contains("goxlr") && name.to_lowercase().contains("chat mic"));
            let label = if dev.is_empty() { name } else if name.is_empty() { dev } else { format!("{name} ({dev})") };
            out.push(Capture { guid, name: label, channels, installed, goxlr });
        }
        out.sort_by_key(|c| (!c.goxlr, c.name.to_lowercase()));
        out
    }

    /// La clé des effets d'une entrée appartient au service audio : propriété aux Administrateurs et droit
    /// d'écriture, pour cette seule clé (ce que fait le Device Selector d'Equalizer APO).
    pub fn own_capture_key(guid: &str) -> Result<(), String> {
        if !rustudio_core::micns::is_guid(guid) {
            return Err("Entrée micro inconnue.".into());
        }
        let sub = format!(r"{CAPTURE_KEY}\{guid}\FxProperties");
        let script = format!(
            r#"$ErrorActionPreference='Stop'
Add-Type -TypeDefinition 'using System;using System.Runtime.InteropServices;public static class RsPriv{{[DllImport("ntdll.dll")]public static extern int RtlAdjustPrivilege(int p,bool e,bool t,out bool w);}}'
$w=$false; [RsPriv]::RtlAdjustPrivilege(9,$true,$false,[ref]$w) | Out-Null
$sub={sub}
$admins=New-Object System.Security.Principal.SecurityIdentifier('S-1-5-32-544')
$k=[Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($sub,[Microsoft.Win32.RegistryKeyPermissionCheck]::ReadWriteSubTree,[System.Security.AccessControl.RegistryRights]::TakeOwnership)
$acl=$k.GetAccessControl([System.Security.AccessControl.AccessControlSections]::None); $acl.SetOwner($admins); $k.SetAccessControl($acl); $k.Close()
$k=[Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($sub,[Microsoft.Win32.RegistryKeyPermissionCheck]::ReadWriteSubTree,[System.Security.AccessControl.RegistryRights]::ChangePermissions)
$acl=$k.GetAccessControl(); $acl.AddAccessRule((New-Object System.Security.AccessControl.RegistryAccessRule($admins,'FullControl','ContainerInherit','None','Allow'))); $k.SetAccessControl($acl); $k.Close()
'ok'"#,
            sub = crate::win::ps_quote(&sub)
        );
        let out = crate::win::powershell(&script);
        if out.trim().ends_with("ok") {
            Ok(())
        } else {
            Err("Windows refuse l’accès aux réglages de ce micro.".into())
        }
    }

    fn install_on(root_key: &str, guid: &str) -> std::io::Result<()> {
        let fx = hklm().open_subkey_with_flags(format!(r"{root_key}\{guid}\FxProperties"), KEY_READ | KEY_SET_VALUE | KEY_WOW64_64KEY)?;
        let get = |name: &str| fx.get_value::<String, _>(name).ok();
        let orig: IndexMap<u32, Option<String>> = FX_SLOTS.iter().map(|s| (*s, get(&format!("{FX}{s}")))).collect();
        if orig.values().flatten().any(|v| v.to_uppercase() == EQAPO_PRE || v.to_uppercase() == EQAPO_POST) {
            let _ = fx.delete_value(DISABLE_ENH);
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
        uninstall_on(RENDER_KEY, guid)
    }

    fn uninstall_on(root_key: &str, guid: &str) -> std::io::Result<()> {
        let Ok(ch) = hklm().open_subkey_with_flags(format!(r"{CHILD_APOS}\{guid}"), KEY_READ | KEY_WOW64_64KEY) else { return Ok(()) };
        let orig: Vec<(u32, String)> =
            FX_SLOTS.iter().map(|s| (*s, ch.get_value::<String, _>(format!("{FX}{s}")).unwrap_or_else(|_| "!VALUE".into()))).collect();
        drop(ch);
        let fx = hklm().open_subkey_with_flags(format!(r"{root_key}\{guid}\FxProperties"), KEY_READ | KEY_SET_VALUE | KEY_WOW64_64KEY)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_eqapo(tag: &str) -> (PathBuf, EqApo) {
        let dir = std::env::temp_dir().join(format!("rustudio-eqapo-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("eqapo-sim/config")).unwrap();
        let cfg = Config {
            simulate: true,
            port: 0,
            data_dir: dir.clone(),
            base_dir: dir.clone(),
            supervised: false,
            engine_restarts: 0,
        };
        (dir, EqApo::new(&cfg, true))
    }

    #[test]
    fn bypass_micro_met_en_pause_et_restaure_la_derniere_config() {
        let (dir, apo) = temp_eqapo("bypass");
        let mic_cfg = dir.join("eqapo-sim/config").join(rustudio_core::micns::CONFIG_FILE);
        assert!(apo.write_mic("Device: {A}\nVSTPlugin: test1\nDevice: all\n"));
        assert!(std::fs::read_to_string(&mic_cfg).unwrap().contains("VSTPlugin: test1"));

        apo.set_mic_bypass(true);
        let paused = std::fs::read_to_string(&mic_cfg).unwrap();
        assert!(!paused.contains("VSTPlugin"), "RNNoise doit etre coupe pendant la mesure : {paused}");

        // Une ecriture en arriere-plan (watch::ns_apply) ne doit pas reactiver RNNoise pendant la pause,
        // mais la nouvelle valeur doit etre restauree a la fin de la pause.
        assert!(!apo.write_mic("Device: {A}\nVSTPlugin: test2\nDevice: all\n"));
        assert!(!std::fs::read_to_string(&mic_cfg).unwrap().contains("VSTPlugin"));

        apo.set_mic_bypass(false);
        assert!(std::fs::read_to_string(&mic_cfg).unwrap().contains("VSTPlugin: test2"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn take_config_ignore_lignes_commentees_et_reset_written_reecrit_micro() {
        let (dir, apo) = temp_eqapo("take");
        let cfg_file = dir.join("eqapo-sim/config/config.txt");
        std::fs::write(&cfg_file, "# Include: rustudio.txt\n# Include: rustudio-micro.txt\n").unwrap();
        apo.take_config();
        let content = std::fs::read_to_string(&cfg_file).unwrap();
        assert!(content.lines().any(|l| l.trim() == "Include: rustudio.txt"), "{content}");
        assert!(content.lines().any(|l| l.trim() == "Include: rustudio-micro.txt"), "{content}");
        assert!(content.lines().any(|l| l.trim() == "Include: rustudio-micro-voix.txt"), "{content}");

        let text = "Device: {A}\nVSTPlugin: test\nDevice: all\n";
        assert!(apo.write_mic(text));
        assert!(!apo.write_mic(text));
        apo.reset_written();
        assert!(apo.write_mic(text), "reset_written doit aussi invalider le cache micro");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
