//! Traitement des entrées Windows et migration explicite de la configuration Equalizer APO.
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
    pub(crate) native: crate::native_mic::Native,
    settings: Mutex<Settings>,
    command: Mutex<()>,
    /// entrées Windows relues au plus toutes les quelques secondes (registre), pour l'accueil et l'état
    captures: Mutex<Option<(Instant, Vec<Capture>)>>,
    version: AtomicU64,
}

impl WinMicHub {
    pub fn new(store: Option<PathBuf>, simulated: bool) -> WinMicHub {
        let settings = store
            .as_deref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str::<Settings>(&t).ok())
            .map(Settings::sanitized)
            .unwrap_or_default();
        WinMicHub {
            store,
            native: crate::native_mic::Native::new(simulated || !cfg!(windows)),
            settings: Mutex::new(settings),
            command: Mutex::new(()),
            captures: Mutex::new(None),
            version: AtomicU64::new(0),
        }
    }

    pub fn changes(&self) -> u64 {
        self.version.load(Ordering::Relaxed)
    }

    pub fn settings(&self) -> Settings {
        self.settings.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn edit(&self, update: impl FnOnce(&Settings) -> Result<Settings, String>) -> Result<(), String> {
        let mut cur = self.settings.lock().unwrap_or_else(|e| e.into_inner());
        let next = update(&cur)?;
        if *cur == next {
            return Ok(());
        }
        if let Some(path) = &self.store {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            let tmp = path.with_extension("json.tmp");
            let bytes = serde_json::to_vec_pretty(&next).map_err(|e| e.to_string())?;
            std::fs::write(&tmp, bytes)
                .and_then(|_| std::fs::rename(&tmp, path))
                .map_err(|e| format!("Enregistrement des réglages micro : {e}"))?;
        }
        *cur = next;
        self.version.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn edit_voice(&self, guid: &str, change: impl FnOnce(&mut wm::Voice)) -> Result<(), String> {
        self.edit(|s| {
            let mut next = s.clone();
            let mut voice = s.voice(guid);
            change(&mut voice);
            next.store_voice(guid.to_owned(), voice)?;
            Ok(next)
        })
    }

    fn cached(&self, max_age: Duration) -> Option<Vec<Capture>> {
        let g = self.captures.lock().unwrap_or_else(|e| e.into_inner());
        g.as_ref().filter(|(t, _)| t.elapsed() < max_age).map(|(_, c)| c.clone())
    }

    fn store_captures(&self, caps: &[Capture]) {
        let mut g = self.captures.lock().unwrap_or_else(|e| e.into_inner());
        let changed = g.as_ref().is_none_or(|(_, old)| {
            old.len() != caps.len()
                || old
                    .iter()
                    .zip(caps)
                    .any(|(a, b)| a.guid != b.guid || a.installed != b.installed || a.name != b.name)
        });
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

    fn winmic_target(&self, p: &Value, caps: &[Capture]) -> Result<Capture, ApiError> {
        let selected = if let Some(id) = p.get("device") {
            id.as_str().and_then(|g| caps.iter().find(|c| c.guid.eq_ignore_ascii_case(g))).cloned()
        } else {
            self.winmic_selected(&self.winmic.settings(), caps)
        };
        selected.ok_or_else(|| ApiError::bad("Ce micro est absent de Windows."))
    }

    /// Écrit rustudio-micro-voix.txt d'après les réglages : seuls les micros actifs et branchés sont traités.
    pub fn winmic_apply(&self) {
        let caps = self.winmic_captures(Duration::from_secs(2));
        let settings = self.winmic.settings.lock().unwrap_or_else(|e| e.into_inner());
        let s = settings.clone();
        for (g, v) in &s.mics {
            if v.native_requested {
                if let Err(e) = self.winmic.native.sync(g, v) {
                    crate::logf!("Micro : {e}");
                }
            }
        }
        drop(settings);
        if !self.eqapo.installed() {
            return;
        }

        let targets: Vec<wm::Target> = caps
            .iter()
            .map(|c| wm::Target {
                guid: &c.guid,
                installed: c.installed && !self.winmic.native.attached(&c.guid.to_lowercase()),
            })
            .collect();
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
        let here = |c: &Option<Capture>| {
            c.as_ref()
                .zip(selected.as_ref())
                .is_some_and(|(a, b)| a.guid.eq_ignore_ascii_case(&b.guid))
        };
        let setup = self.eqapo.setup_value().filter(|v| v["kind"] == "winmic" || v["kind"] == "mic-ns");
        let voice = selected.as_ref().map(|c| s.voice(&c.guid)).unwrap_or_default();
        let native = selected
            .as_ref()
            .map(|c| self.winmic.native.status(&c.guid.to_ascii_lowercase(), &voice))
            .unwrap_or(Value::Null);
        let is_native = native["attached"] == true;
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
                "native": native, "legacy": selected.as_ref().is_some_and(|c|c.installed),
                "apo": self.eqapo.installed(),
                "plugin": self.eqapo.plugin_installed(),
                "installed": is_native || selected.as_ref().is_some_and(|c| c.installed),
                "setup": setup,
                "ns": {
                    "enabled": if is_native {voice.noise} else {ns.enabled && here(&ns_target)},
                    "strength": if is_native {voice.strength} else {ns.strength},
                    "grace_ms": if is_native {voice.grace_ms} else {ns.grace_ms},
                    // la suppression du bruit tourne sur un autre micro : la page le dit avant de la déplacer
                    "elsewhere": (ns.enabled && !here(&ns_target)).then(|| ns_target.as_ref().map(|t| t.name.clone())).flatten(),
                },
            },
        })
    }

    pub(crate) fn winmic_command(self: &Arc<Self>, action: &str, p: &Value) -> ApiResult {
        let _command = self.winmic.command.lock().unwrap_or_else(|e| e.into_inner());
        if matches!(action, "attach" | "detach" | "native-noise") {
            return self.winmic_native(action, p);
        }
        if self.eqapo.setup_running() {
            return Err(ApiError::bad("Une opération audio est en cours."));
        }
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
                self.winmic.edit(|s| wm::update(&p, s)).map_err(ApiError::bad)?;
                self.winmic_apply();
            }
            "noise" => {
                let caps = self.winmic_captures(Duration::from_secs(1));
                let target = self.winmic_target(p, &caps)?;
                if self.winmic.native.managed(&target.guid.to_ascii_lowercase()) {
                    let mut body = p.clone();
                    body["device"] = json!(target.guid);
                    return self.winmic_native("native-noise", &body);
                }
                let wants = p.get("enabled").map(rustudio_core::value::truthy);
                let ready = self.eqapo.installed() && self.eqapo.plugin_installed() && target.installed;
                if wants == Some(true) && !ready {
                    return self.winmic_setup(true, true, p);
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
            "setup" => return self.winmic_setup(true, p.get("noise").is_some_and(rustudio_core::value::truthy), p),
            "remove" => return self.winmic_setup(false, false, p),
            _ => {
                return Err(ApiError {
                    status: 404,
                    msg: format!("action inconnue pour ce module : {action}"),
                })
            }
        }
        Ok(self.winmic_state())
    }

    /// Branche Equalizer APO sur le micro affiché (et l'installe s'il manque) puis active sa voix, avec RNNoise
    /// si `noise` ; ou retire Equalizer APO de ce micro. Le son de Windows redémarre (2 secondes).
    fn winmic_setup(self: &Arc<Self>, install: bool, noise: bool, p: &Value) -> ApiResult {
        if self.eqapo.setup_running() {
            return Err(ApiError::bad("Une opération sur Equalizer APO est déjà en cours."));
        }
        let caps = self.winmic_captures(Duration::from_secs(0));
        let target = self.winmic_target(p, &caps)?;
        if self.winmic.native.managed(&target.guid.to_ascii_lowercase()) {
            return Err(ApiError::bad("Ce micro utilise déjà l’effet Rustudio."));
        }
        self.eqapo.begin_setup("winmic", install)?;
        let c = self.clone();
        std::thread::spawn(move || {
            let step = |t: &str| c.eqapo.set_setup(&[("step", json!(t))]);
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
                    c.winmic.edit(|cur| {
                        let mut s = cur.clone();
                        s.selected = Some(guid.clone());
                        if noise {
                            Ok(s)
                        } else {
                            wm::update(&json!({"device":guid,"enabled":true}), &s)
                        }
                    })?;
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
                    c.winmic.edit(|s| wm::update(&json!({"device": guid, "enabled": false}), s))?;
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
                Ok(()) => c
                    .eqapo
                    .set_setup(&[("running", json!(false)), ("done", json!(true)), ("step", json!("Terminé"))]),
                Err(e) => {
                    crate::logf!("Module Micro : {e}");
                    c.eqapo.set_setup(&[("running", json!(false)), ("error", json!(e))]);
                }
            }
            c.lock().version += 1;
        });
        Ok(json!({"ok": true, "running": true}))
    }
}

impl Ctl {
    pub(crate) fn winmic_watchdog(&self) {
        let failed = self.winmic.native.faults();
        if failed.is_empty() || self.eqapo.begin_setup("winmic", false).is_err() {
            return;
        }
        let mut errors = Vec::new();
        let mut removed = false;
        for guid in &failed {
            match self.winmic.native.remove(guid) {
                Ok(()) => {
                    removed = true;
                    if let Err(e) = self.winmic.edit_voice(guid, |v| {
                        v.native_requested = false;
                        v.safe_mode = true;
                    }) {
                        errors.push(e);
                    }
                }
                Err(e) => errors.push(e),
            }
        }
        if removed {
            if let Err(e) = crate::native_mic::restart_audio(self.simulated()) {
                errors.push(e);
            }
        }
        let msg = if errors.is_empty() {
            "L’effet micro a été retiré après une erreur audio.".to_owned()
        } else {
            format!("Restauration du micro : {}", errors.join(". "))
        };
        self.eqapo.set_setup(&[("running", json!(false)), ("error", json!(msg))]);
    }

    fn winmic_native(self: &Arc<Self>, action: &str, p: &Value) -> ApiResult {
        let guid = p["device"]
            .as_str()
            .filter(|g| rustudio_core::micns::is_guid(g))
            .ok_or_else(|| ApiError::bad("Choisis le micro à régler."))?
            .to_ascii_lowercase();
        let caps = self.winmic_captures(Duration::ZERO);
        let target = caps.iter().find(|c| c.guid.eq_ignore_ascii_case(&guid));
        let voice = self.winmic.settings().voice(&guid);
        let installed = self.winmic.native.attached(&guid);
        if self.eqapo.setup_running() {
            return Err(ApiError::bad("Une opération audio est en cours."));
        }
        if action == "attach" && installed && voice.native_requested && voice.enabled && !voice.safe_mode {
            return Ok(self.winmic_state());
        }
        if action == "native-noise" {
            if !installed && p["enabled"] != true {
                return Err(ApiError::bad("Aucun filtre Rustudio actif sur ce micro."));
            }
            for (key, value) in p.as_object().ok_or_else(|| ApiError::bad("Réglages de bruit invalides."))? {
                let valid = match key.as_str() {
                    "device" => true,
                    "enabled" => value.is_boolean(),
                    "strength" | "grace_ms" => value.is_i64(),
                    _ => false,
                };
                if !valid {
                    return Err(ApiError::bad("Réglage de bruit invalide."));
                }
            }
        }
        if action == "native-noise" && installed && p.get("enabled").and_then(Value::as_bool).is_none_or(|v| v == voice.noise) {
            self.winmic
                .edit_voice(&guid, |v| {
                    if let Some(s) = p["strength"].as_i64() {
                        v.strength = s.clamp(0, 99) as i32;
                    }
                    if let Some(s) = p["grace_ms"].as_i64() {
                        v.grace_ms = s.clamp(0, 500) as i32;
                    }
                })
                .map_err(ApiError::bad)?;
            self.winmic_apply();
            return Ok(self.winmic_state());
        }
        if action != "detach" && target.is_none() {
            return Err(ApiError::bad("Ce micro est débranché."));
        }
        let detach = action == "detach";
        let manage = self.winmic.native.managed(&guid);
        let legacy = target.is_some_and(|t| t.installed);
        let ns = self.lock().state.mic_ns.clone();
        let migrate_noise = legacy && ns.enabled && self.ns_target().is_some_and(|t| t.guid.eq_ignore_ascii_case(&guid));
        if detach && !manage && !voice.native_requested && !voice.safe_mode {
            return Err(ApiError::bad("Aucun branchement Rustudio à retirer."));
        }
        let mut next = voice.clone();
        next.native_requested = !detach;
        next.safe_mode = false;
        if detach {
            next.enabled = false;
            next.noise = false;
        } else if action == "attach" {
            next.enabled = true;
        }
        if migrate_noise && !detach {
            next.noise = true;
            next.strength = ns.strength;
            next.grace_ms = ns.grace_ms;
        }
        if action == "native-noise" {
            next.noise = p["enabled"].as_bool().ok_or_else(|| ApiError::bad("Activation du filtre attendue."))?;
        }
        let mut projected = self.winmic.settings();
        projected.store_voice(guid.clone(), next.clone()).map_err(ApiError::bad)?;
        self.eqapo.begin_setup("winmic", !detach)?;
        let c = self.clone();
        std::thread::spawn(move || {
            let edit = |v: &wm::Voice| c.winmic.edit_voice(&guid, |out| *out = v.clone());
            let step = |message: &str| c.eqapo.set_setup(&[("step", json!(message))]);
            let result = (|| -> Result<(), String> {
                if detach {
                    step("Restauration des effets du micro…");
                    c.winmic.native.remove(&guid)?;
                    let saved = edit(&next);
                    let restarted = if manage {
                        step("Redémarrage du service audio de Windows…");
                        crate::native_mic::restart_audio(c.simulated())
                    } else {
                        Ok(())
                    };
                    saved.and(restarted)?;
                } else {
                    step("Sauvegarde et branchement du micro…");
                    if manage && !installed {
                        c.winmic.native.remove(&guid)?;
                    }
                    let allow_migration = legacy && (voice.enabled || migrate_noise);
                    edit(&next)?;
                    if let Err(cause) = c.winmic.native.attach(&guid, &next, allow_migration) {
                        edit(&voice).map_err(|e| format!("{cause} Restauration des réglages : {e}"))?;
                        return Err(cause);
                    }
                    let verified = (|| -> Result<(), String> {
                        step("Redémarrage du service audio de Windows…");
                        crate::native_mic::restart_audio(c.simulated())?;
                        #[cfg(windows)]
                        if !c.simulated() {
                            step("Vérification du traitement sur ce micro…");
                            wm::capture::verify(&guid)?;
                        }
                        Ok(())
                    })();
                    if let Err(cause) = verified {
                        step("Restauration des effets du micro…");
                        c.winmic.native.remove(&guid).map_err(|e| format!("{cause} Restauration : {e}"))?;
                        let mut old = voice.clone();
                        old.native_requested = false;
                        old.safe_mode = true;
                        let saved = edit(&old);
                        let restarted = crate::native_mic::restart_audio(c.simulated());
                        saved.and(restarted).map_err(|e| format!("{cause} Restauration : {e}"))?;
                        return Err(format!("{cause} Les effets Windows précédents ont été rétablis."));
                    }
                    if migrate_noise {
                        let mut g = c.lock();
                        if g.state.mic_ns == ns {
                            g.state.mic_ns.enabled = false;
                            g.save_dirty = true;
                            g.version += 1;
                        }
                    }
                }
                Ok(())
            })();
            c.winmic.store_captures(&c.eqapo.captures());
            c.ns_apply();
            c.winmic_apply();
            match result {
                Ok(()) => c
                    .eqapo
                    .set_setup(&[("running", json!(false)), ("done", json!(true)), ("step", json!("Terminé"))]),
                Err(e) => {
                    crate::logf!("Module Micro : {e}");
                    c.eqapo.set_setup(&[("running", json!(false)), ("error", json!(e))]);
                }
            }
            c.lock().version += 1;
        });
        Ok(json!({"ok":true,"running":true}))
    }
}
