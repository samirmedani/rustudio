//! Module « Micro » : la voix de n'importe quel micro de Windows, traitée par Equalizer APO (gain, coupe-bas,
//! égaliseur), avec la suppression du bruit RNNoise, partagée avec la page Micro de la GoXLR : un seul micro à la
//! fois en profite. Le volume et le micro par défaut de Windows appartiennent à la session de l'utilisateur :
//! l'appli les règle elle-même, le moteur (compte système) ne les voit pas.
//!
//! Rien ne s'active seul : brancher Equalizer APO, activer la voix ou la suppression du bruit demande un clic.
use crate::api::{ApiError, ApiResult};
use crate::controller::Ctl;
use crate::eqapo::Capture;
use crate::log::log;
use rustudio_core::modules::{ModuleRuntime, ModuleStatus};
use rustudio_windows_mic::{self as wm, Settings};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const ID: &str = wm::ID;

pub struct WinMicHub {
    store: Option<PathBuf>,
    settings: Mutex<Settings>,
    /// entrées Windows relues au plus toutes les quelques secondes (registre), pour l'accueil et l'état
    captures: Mutex<Option<(Instant, Vec<Capture>)>>,
    version: AtomicU64,
}

impl WinMicHub {
    pub fn new(store: Option<PathBuf>) -> WinMicHub {
        let settings = store
            .as_deref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str::<Settings>(&t).ok())
            .map(Settings::sanitized)
            .unwrap_or_default();
        WinMicHub { store, settings: Mutex::new(settings), captures: Mutex::new(None), version: AtomicU64::new(0) }
    }

    pub fn changes(&self) -> u64 {
        self.version.load(Ordering::Relaxed)
    }

    pub fn settings(&self) -> Settings {
        self.settings.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set(&self, s: Settings) {
        let mut cur = self.settings.lock().unwrap_or_else(|e| e.into_inner());
        if *cur == s {
            return;
        }
        *cur = s.clone();
        drop(cur);
        if let Some(path) = &self.store {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let tmp = path.with_extension("json.tmp");
            if let Ok(text) = serde_json::to_string_pretty(&s) {
                if std::fs::write(&tmp, text).is_ok() {
                    let _ = std::fs::rename(&tmp, path);
                }
            }
        }
        self.version.fetch_add(1, Ordering::Relaxed);
    }

    fn cached(&self, max_age: Duration) -> Option<Vec<Capture>> {
        let g = self.captures.lock().unwrap_or_else(|e| e.into_inner());
        g.as_ref().filter(|(t, _)| t.elapsed() < max_age).map(|(_, c)| c.clone())
    }

    fn store_captures(&self, caps: &[Capture]) {
        let mut g = self.captures.lock().unwrap_or_else(|e| e.into_inner());
        let changed = g.as_ref().is_none_or(|(_, old)| old.len() != caps.len() || old.iter().zip(caps).any(|(a, b)| a.guid != b.guid || a.installed != b.installed || a.name != b.name));
        *g = Some((Instant::now(), caps.to_vec()));
        drop(g);
        if changed {
            self.version.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl Ctl {
    /// Entrées micro de Windows, relues si le cache a plus de `max_age`.
    fn winmic_captures(&self, max_age: Duration) -> Vec<Capture> {
        if let Some(c) = self.winmic.cached(max_age) {
            return c;
        }
        let caps = self.eqapo.captures();
        self.winmic.store_captures(&caps);
        caps
    }

    /// Accueil : prêt dès que Windows voit un micro. Ne lit que le cache (appelé sous le verrou du moteur).
    pub(crate) fn winmic_runtime(&self) -> ModuleRuntime {
        let any = self.winmic.cached(Duration::from_secs(3600)).is_some_and(|c| !c.is_empty());
        ModuleRuntime {
            id: ID,
            status: if any { ModuleStatus::Ready } else { ModuleStatus::Disconnected },
            connected: any,
            simulated: self.simulated(),
            firmware: None,
            firmware_kind: None,
            error: None,
        }
    }

    /// Micro affiché : celui choisi s'il est branché, sinon le micro par défaut de Windows, sinon le premier qui
    /// n'est pas celui de la GoXLR (elle a sa propre page).
    fn winmic_selected(&self, s: &Settings, caps: &[Capture]) -> Option<Capture> {
        let find = |g: &str| caps.iter().find(|c| c.guid.eq_ignore_ascii_case(g)).cloned();
        let default = self.lock().mic_default.clone();
        s.selected
            .as_deref()
            .and_then(find)
            .or_else(|| default.as_deref().and_then(find))
            .or_else(|| caps.iter().find(|c| !c.goxlr).cloned())
            .or_else(|| caps.first().cloned())
    }

    /// Écrit rustudio-micro-voix.txt d'après les réglages : seuls les micros actifs et branchés sont traités.
    pub fn winmic_apply(&self) {
        let caps = self.winmic_captures(Duration::from_secs(2));
        if !self.eqapo.installed() {
            return;
        }
        let s = self.winmic.settings();
        let targets: Vec<wm::Target> = caps.iter().map(|c| wm::Target { guid: &c.guid, installed: c.installed }).collect();
        let text = wm::config_text(&s, &targets);
        if text.contains("Device:") {
            // la voix vient d'être activée : config.txt doit inclure son fichier (sans effet s'il l'inclut déjà)
            self.eqapo.take_config();
        }
        self.eqapo.write_voice(&text);
    }

    pub(crate) fn winmic_state(&self) -> Value {
        let s = self.winmic.settings();
        let caps = self.winmic_captures(Duration::from_secs(1));
        let selected = self.winmic_selected(&s, &caps);
        let ns = self.lock().state.mic_ns.clone();
        let ns_target = self.ns_target();
        let here = |c: &Option<Capture>| c.as_ref().zip(selected.as_ref()).is_some_and(|(a, b)| a.guid.eq_ignore_ascii_case(&b.guid));
        let setup = self.eqapo.setup_value().filter(|v| v["kind"] == "winmic" || v["kind"] == "mic-ns");
        let voice = selected.as_ref().map(|c| s.voice(&c.guid)).unwrap_or_default();
        json!({
            "state": {
                "selected": selected.as_ref().map(|c| c.guid.to_lowercase()),
                "voice": voice,
            },
            "v": self.winmic.changes() + self.eqapo.changes(),
            "meta": wm::meta(),
            "live": {
                "captures": caps.iter().map(|c| json!({"guid": c.guid.to_lowercase(), "name": c.name, "channels": c.channels,
                    "installed": c.installed, "goxlr": c.goxlr, "enabled": s.voice(&c.guid).enabled})).collect::<Vec<_>>(),
                "apo": self.eqapo.installed(),
                "plugin": self.eqapo.plugin_installed(),
                "installed": selected.as_ref().is_some_and(|c| c.installed),
                "setup": setup,
                "ns": {
                    "enabled": ns.enabled && here(&ns_target),
                    "strength": ns.strength,
                    "grace_ms": ns.grace_ms,
                    // la suppression du bruit tourne sur un autre micro : la page le dit avant de la déplacer
                    "elsewhere": (ns.enabled && !here(&ns_target)).then(|| ns_target.as_ref().map(|t| t.name.clone())).flatten(),
                },
            },
        })
    }

    pub(crate) fn winmic_command(self: &Arc<Self>, action: &str, p: &Value) -> ApiResult {
        match action {
            "settings" => {
                let cur = self.winmic.settings();
                let mut p = p.clone();
                if p.is_object() && p.get("device").is_none() && p.get("selected").is_none() {
                    // sans micro précisé : celui affiché dans la page
                    let caps = self.winmic_captures(Duration::from_secs(1));
                    if let Some(shown) = self.winmic_selected(&cur, &caps) {
                        p["device"] = json!(shown.guid.to_lowercase());
                    }
                }
                let next = wm::update(&p, &cur).map_err(ApiError::bad)?;
                self.winmic.set(next);
                self.winmic_apply();
            }
            "noise" => {
                let caps = self.winmic_captures(Duration::from_secs(1));
                let target = self.winmic_selected(&self.winmic.settings(), &caps).ok_or_else(|| ApiError::bad("Aucun micro trouvé dans Windows."))?;
                let wants = p.get("enabled").map(rustudio_core::value::truthy);
                let ready = self.eqapo.installed() && self.eqapo.plugin_installed() && target.installed;
                if wants == Some(true) && !ready {
                    return self.winmic_setup(true, true);
                }
                {
                    let mut g = self.lock();
                    let mut src = p.clone();
                    // la suppression du bruit suit le micro affiché dans cette page
                    src["device"] = json!(target.guid.to_lowercase());
                    g.state.mic_ns = rustudio_core::micns::normalize(&src, Some(&g.state.mic_ns));
                    g.save_dirty = true;
                    g.version += 1;
                }
                self.ns_apply();
            }
            "setup" => return self.winmic_setup(true, p.get("noise").is_some_and(rustudio_core::value::truthy)),
            "remove" => return self.winmic_setup(false, false),
            _ => return Err(ApiError { status: 404, msg: format!("action inconnue pour ce module : {action}") }),
        }
        Ok(self.winmic_state())
    }

    /// Branche Equalizer APO sur le micro affiché (et l'installe s'il manque) puis active sa voix, avec RNNoise
    /// si `noise` ; ou retire Equalizer APO de ce micro. Le son de Windows redémarre (2 secondes).
    fn winmic_setup(self: &Arc<Self>, install: bool, noise: bool) -> ApiResult {
        if self.eqapo.setup_running() {
            return Err(ApiError::bad("Une opération sur Equalizer APO est déjà en cours."));
        }
        let caps = self.winmic_captures(Duration::from_secs(0));
        let target = self.winmic_selected(&self.winmic.settings(), &caps).ok_or_else(|| ApiError::bad("Aucun micro trouvé dans Windows : branche-le puis recommence."))?;
        self.eqapo.set_setup(
            &[("running", json!(true)), ("step", json!("Préparation…")), ("error", Value::Null), ("done", json!(false)), ("kind", json!("winmic")), ("install", json!(install))],
            true,
        );
        let c = self.clone();
        std::thread::spawn(move || {
            let step = |t: &str| c.eqapo.set_setup(&[("step", json!(t))], false);
            let guid = target.guid.to_lowercase();
            let r = (|| -> Result<(), String> {
                if install {
                    if !c.eqapo.installed() {
                        c.eqapo.download_and_install(&step)?;
                    }
                    if noise {
                        c.eqapo.install_plugin(&step)?;
                    }
                    step("Branchement sur ton micro…");
                    if !target.installed {
                        c.eqapo.install_capture(&target.guid)?;
                    }
                    let mut s = c.winmic.settings();
                    s.selected = Some(guid.clone());
                    // brancher sans « noise » : l'utilisateur a demandé le traitement de sa voix
                    let s = if noise { s } else { wm::update(&json!({"device": guid, "enabled": true}), &s)? };
                    c.winmic.set(s);
                    if noise {
                        let mut g = c.lock();
                        g.state.mic_ns.device = Some(guid.clone());
                        g.state.mic_ns.enabled = true;
                        g.save_dirty = true;
                        g.version += 1;
                    }
                    c.eqapo.take_config();
                    c.ns_apply();
                    c.winmic.store_captures(&c.eqapo.captures());
                    c.winmic_apply();
                    step("Redémarrage du son de Windows (2 secondes)…");
                    c.eqapo.restart_audio();
                    crate::logf!("Module Micro : Equalizer APO branché sur « {} »", target.name);
                } else {
                    if target.installed {
                        step("Retrait du micro…");
                        c.eqapo.uninstall_capture(&target.guid)?;
                    }
                    let s = wm::update(&json!({"device": guid, "enabled": false}), &c.winmic.settings())?;
                    c.winmic.set(s);
                    {
                        let mut g = c.lock();
                        if g.state.mic_ns.device.as_deref().is_some_and(|d| d.eq_ignore_ascii_case(&guid)) {
                            g.state.mic_ns.enabled = false;
                            g.save_dirty = true;
                            g.version += 1;
                        }
                    }
                    c.ns_apply();
                    c.winmic.store_captures(&c.eqapo.captures());
                    c.winmic_apply();
                    step("Redémarrage du son de Windows (2 secondes)…");
                    c.eqapo.restart_audio();
                    log("Module Micro : Equalizer APO retiré du micro");
                }
                Ok(())
            })();
            match r {
                Ok(()) => c.eqapo.set_setup(&[("running", json!(false)), ("done", json!(true)), ("step", json!("Terminé"))], false),
                Err(e) => {
                    crate::logf!("Module Micro : {e}");
                    c.eqapo.set_setup(&[("running", json!(false)), ("error", json!(e))], false);
                }
            }
            c.lock().version += 1;
        });
        Ok(json!({"ok": true, "running": true}))
    }
}
