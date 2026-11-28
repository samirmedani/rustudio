//! Webcam Logitech C920 : un fil UVC independant de la GoXLR, de la souris et du clavier.
//! L'API lit le cache ; les commandes passent par ce fil, une a la fois.
//! Aucune ecriture automatique a la connexion tant que « Garder mes réglages » n'est pas active.
use crate::controller::Ctl;
use rustudio_core::modules::{ModuleRuntime, ModuleStatus};
use rustudio_logitech_c920::{presets, Camera, DesiredSettings, Settings, Transport};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

pub const ID: &str = "logitech-c920";

/// Nombre d'echecs/derives consecutifs sur un meme champ avant mise en pause temporaire.
const FIELD_BACKOFF_STRIKES: u8 = 3;
const FIELD_BACKOFF_DURATION: Duration = Duration::from_secs(30);

struct Request {
    action: String,
    body: Value,
    deadline: Instant,
    answer: mpsc::SyncSender<Result<Value, String>>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Persisted {
    #[serde(default)]
    locked: bool,
    #[serde(default)]
    active_preset: Option<String>,
    #[serde(default)]
    desired: Option<DesiredSettings>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WebcamState {
    #[serde(flatten)]
    pub settings: Settings,
    pub locked: bool,
    pub active_preset: Option<String>,
    pub reapply_count: u64,
    pub last_reapplied_fields: Vec<String>,
    pub backoff_fields: Vec<String>,
}

#[derive(Clone)]
struct HubState {
    settings: Option<Settings>,
    connected: bool,
    present: bool,
    error: Option<String>,
    locked: bool,
    active_preset: Option<String>,
    desired: Option<DesiredSettings>,
    reapply_count: u64,
    last_reapplied_fields: Vec<String>,
    backoff_fields: Vec<String>,
}

struct Shared {
    state: Mutex<HubState>,
    version: AtomicU64,
    simulated: bool,
    store_path: Option<PathBuf>,
}

pub struct WebcamHub {
    shared: Arc<Shared>,
    tx: mpsc::SyncSender<Request>,
    rx: Mutex<Option<mpsc::Receiver<Request>>>,
}

fn load_persisted(path: &Path) -> Persisted {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|txt| serde_json::from_str::<Persisted>(&txt).ok())
        .unwrap_or_default()
}

fn save_persisted(path: &Path, data: &Persisted) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    if let Ok(json) = serde_json::to_string_pretty(data) {
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

impl WebcamHub {
    pub fn new(simulated: bool, store_path: Option<PathBuf>) -> Self {
        let (tx, rx) = mpsc::sync_channel(1);
        let persisted = store_path.as_deref().map(load_persisted).unwrap_or_default();
        let settings = if simulated {
            Camera::new(rustudio_logitech_c920::sim::Sim::default()).read().ok()
        } else {
            None
        };
        let state = HubState {
            connected: settings.is_some(),
            present: settings.is_some(),
            settings,
            error: None,
            locked: persisted.locked,
            active_preset: persisted.active_preset,
            desired: persisted.desired,
            reapply_count: 0,
            last_reapplied_fields: Vec::new(),
            backoff_fields: Vec::new(),
        };
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(state),
                version: AtomicU64::new(0),
                simulated,
                store_path,
            }),
            tx,
            rx: Mutex::new(Some(rx)),
        }
    }

    pub fn changes(&self) -> u64 {
        self.shared.version.load(Ordering::Relaxed)
    }

    pub fn runtime(&self) -> ModuleRuntime {
        let s = self.shared.state.lock().unwrap();
        let status = if s.connected {
            ModuleStatus::Ready
        } else if s.present {
            ModuleStatus::Error
        } else {
            ModuleStatus::Disconnected
        };
        ModuleRuntime {
            id: ID,
            status,
            connected: s.connected,
            simulated: self.shared.simulated,
            firmware: if s.connected {
                s.settings.as_ref().and_then(|x| x.device.firmware.clone())
            } else {
                None
            },
            firmware_kind: None,
            error: s.error.clone(),
        }
    }

    pub fn state(&self) -> Value {
        let s = self.shared.state.lock().unwrap();
        let payload = s.settings.as_ref().map(|settings| WebcamState {
            settings: settings.clone(),
            locked: s.locked,
            active_preset: s.active_preset.clone(),
            reapply_count: s.reapply_count,
            last_reapplied_fields: s.last_reapplied_fields.clone(),
            backoff_fields: s.backoff_fields.clone(),
        });
        json!({
            "state": payload,
            "v": self.changes(),
            "meta": { "presets": presets::summaries() },
            "live": {
                "locked": s.locked,
                "reapply_count": s.reapply_count,
                "active_preset": s.active_preset,
            }
        })
    }

    pub fn command(&self, action: &str, body: &Value) -> Result<Value, String> {
        if !matches!(action, "settings" | "preset" | "reset" | "lock" | "refresh") {
            return Err("Commande webcam inconnue.".into());
        }
        if self.rx.lock().unwrap().is_some() {
            return Err("Le module webcam n’est pas démarré".into());
        }
        let (answer, rx) = mpsc::sync_channel(1);
        let req = Request {
            action: action.into(),
            body: body.clone(),
            deadline: Instant::now() + Duration::from_secs(15),
            answer,
        };
        self.tx
            .try_send(req)
            .map_err(|_| "Une commande webcam est déjà en cours".to_string())?;
        rx.recv_timeout(Duration::from_secs(15))
            .map_err(|_| "La webcam ne répond pas : actualise avant de réessayer".to_string())?
    }

    pub fn start(&self) {
        self.start_with_poll(Duration::from_secs(3), Duration::from_secs(10));
    }

    /// Demarre le fil de suivi avec des intervalles configurables (pratique pour les tests).
    pub fn start_with_poll(&self, locked_poll: Duration, idle_poll: Duration) {
        let Some(rx) = self.rx.lock().unwrap().take() else { return };
        let weak = Arc::downgrade(&self.shared);
        std::thread::Builder::new()
            .name("c920-uvc".into())
            .spawn(move || {
                let mut cam: Option<Camera<Box<dyn Transport>>> = None;
                let mut strikes: HashMap<String, (u8, Option<Instant>)> = HashMap::new();
                while let Some(shared) = weak.upgrade() {
                    let locked = shared.state.lock().unwrap().locked;
                    let wait = if cam.is_some() {
                        if locked { locked_poll } else { idle_poll }
                    } else {
                        Duration::from_millis(100)
                    };
                    let request = match rx.recv_timeout(wait) {
                        Ok(req) => Some(req),
                        Err(mpsc::RecvTimeoutError::Timeout) => None,
                        Err(_) => break,
                    };
                    if request.as_ref().is_some_and(|r| r.deadline <= Instant::now()) {
                        continue;
                    }

                    let mut preset_update: Option<Option<String>> = None;
                    let mut lock_update: Option<bool> = None;
                    let mut user_wrote = false;
                    let mut reapplied_fields: Option<Vec<String>> = None;

                    let result = (|| -> Result<Settings, String> {
                        if cam.is_none() {
                            let opened = open(shared.simulated);
                            shared.state.lock().unwrap().present = opened.is_ok();
                            cam = Some(Camera::new(opened?));
                            strikes.clear();
                        }
                        let c = cam.as_mut().unwrap();
                        if let Some(r) = &request {
                            match r.action.as_str() {
                                "settings" => {
                                    user_wrote = true;
                                    preset_update = Some(None);
                                    strikes.clear();
                                    return c.apply_json(&r.body);
                                }
                                "preset" => {
                                    let id = r.body["id"]
                                        .as_str()
                                        .ok_or_else(|| "Identifiant de préréglage absent".to_owned())?;
                                    user_wrote = true;
                                    preset_update = Some(Some(id.to_owned()));
                                    strikes.clear();
                                    return c.apply_preset(id);
                                }
                                "reset" => {
                                    user_wrote = true;
                                    preset_update = Some(None);
                                    strikes.clear();
                                    return c.reset_defaults();
                                }
                                "lock" => {
                                    let enabled = r.body["enabled"]
                                        .as_bool()
                                        .ok_or_else(|| "Booléen enabled attendu".to_owned())?;
                                    lock_update = Some(enabled);
                                    strikes.clear();
                                    return c.read();
                                }
                                _ => return c.read(),
                            }
                        }

                        // Cycle periodique ou premiere lecture apres connexion / reconnexion.
                        let current = c.read()?;
                        let (is_locked, desired) = {
                            let st = shared.state.lock().unwrap();
                            (st.locked, st.desired.clone())
                        };
                        if is_locked {
                            if let Some(target) = desired {
                                let (mut patch, drifted) = target.diff_from(&current);
                                let now = Instant::now();
                                // Nettoie les pauses expirees et filtre les champs en pause.
                                strikes.retain(|_, (_, until)| until.is_none_or(|t| t > now));
                                let mut active_fields = Vec::new();
                                for name in drifted {
                                    if strikes.get(name).is_some_and(|(_, u)| u.is_some_and(|t| t > now)) {
                                        patch.clear_field(name);
                                    } else {
                                        active_fields.push(name.to_owned());
                                    }
                                }
                                if !patch.is_empty() {
                                    let after = match c.apply_patch(&patch, false) {
                                        Ok(s) => s,
                                        Err(_) => {
                                            for f in &active_fields {
                                                note_strike(&mut strikes, f, now);
                                            }
                                            return c.read();
                                        }
                                    };
                                    // Verifie quels champs ont bien tenu apres reapplication.
                                    let (_, still_drifted) = target.diff_from(&after);
                                    for f in &active_fields {
                                        if still_drifted.contains(&f.as_str()) {
                                            note_strike(&mut strikes, f, now);
                                        } else {
                                            strikes.remove(f);
                                        }
                                    }
                                    let fixed: Vec<String> = active_fields
                                        .into_iter()
                                        .filter(|f| !still_drifted.contains(&f.as_str()))
                                        .collect();
                                    if !fixed.is_empty() {
                                        reapplied_fields = Some(fixed);
                                    }
                                    return Ok(after);
                                }
                            }
                        }
                        Ok(current)
                    })();

                    // Une demande refusee ne fait pas disparaitre une webcam qui repond encore.
                    let recovered = if result.is_err() && request.is_some() {
                        cam.as_mut().and_then(|c| c.read().ok())
                    } else {
                        None
                    };

                    let mut persist_snapshot: Option<Persisted> = None;
                    let answer_payload: Result<Value, String>;
                    {
                        let mut st = shared.state.lock().unwrap();
                        let now = Instant::now();
                        st.backoff_fields = strikes
                            .iter()
                            .filter(|(_, (_, u))| u.is_some_and(|t| t > now))
                            .map(|(k, _)| k.clone())
                            .collect();
                        st.backoff_fields.sort();

                        match result.as_ref().ok().cloned().or(recovered) {
                            Some(settings) => {
                                st.settings = Some(settings.clone());
                                st.connected = true;
                                st.error = None;
                                if let Some(preset) = preset_update {
                                    st.active_preset = preset;
                                }
                                if let Some(enabled) = lock_update {
                                    st.locked = enabled;
                                    if enabled {
                                        st.desired = Some(DesiredSettings::from(&settings));
                                    }
                                    persist_snapshot = Some(Persisted {
                                        locked: st.locked,
                                        active_preset: st.active_preset.clone(),
                                        desired: st.desired.clone(),
                                    });
                                } else if user_wrote && result.is_ok() {
                                    if st.locked {
                                        st.desired = Some(DesiredSettings::from(&settings));
                                    }
                                    persist_snapshot = Some(Persisted {
                                        locked: st.locked,
                                        active_preset: st.active_preset.clone(),
                                        desired: st.desired.clone(),
                                    });
                                }
                                if let Some(fields) = reapplied_fields {
                                    st.reapply_count += 1;
                                    st.last_reapplied_fields = fields;
                                }
                            }
                            None => {
                                st.connected = false;
                                let err = result.as_ref().err().cloned();
                                if err.as_deref().is_some_and(|e| e.contains("débranchée") || e.contains("absente")) {
                                    st.present = false;
                                }
                                st.error = err;
                                cam = None;
                            }
                        }
                        shared.version.fetch_add(1, Ordering::Relaxed);
                        answer_payload = match result {
                            Ok(settings) => Ok(json!({
                                "ok": true,
                                "state": WebcamState {
                                    settings,
                                    locked: st.locked,
                                    active_preset: st.active_preset.clone(),
                                    reapply_count: st.reapply_count,
                                    last_reapplied_fields: st.last_reapplied_fields.clone(),
                                    backoff_fields: st.backoff_fields.clone(),
                                }
                            })),
                            Err(e) => Err(e),
                        };
                    }

                    if let (Some(path), Some(data)) = (shared.store_path.as_deref(), persist_snapshot.as_ref()) {
                        save_persisted(path, data);
                    }
                    if let Some(r) = request {
                        let _ = r.answer.send(answer_payload);
                    }
                    if cam.is_none() {
                        drop(shared);
                        std::thread::sleep(Duration::from_secs(2));
                    }
                }
            })
            .expect("fil webcam");
    }
}

fn note_strike(strikes: &mut HashMap<String, (u8, Option<Instant>)>, field: &str, now: Instant) {
    let entry = strikes.entry(field.to_owned()).or_insert((0, None));
    entry.0 = entry.0.saturating_add(1);
    if entry.0 >= FIELD_BACKOFF_STRIKES {
        entry.1 = Some(now + FIELD_BACKOFF_DURATION);
        entry.0 = 0;
    }
}

fn open(simulated: bool) -> Result<Box<dyn Transport>, String> {
    if simulated {
        if std::env::var_os("RUSTUDIO_SIM_C920_DISCONNECT_FILE")
            .map(PathBuf::from)
            .is_some_and(|p| p.exists())
        {
            return Err(rustudio_logitech_c920::NO_DEVICE.into());
        }
        return Ok(Box::new(rustudio_logitech_c920::sim::Sim::default()));
    }
    #[cfg(windows)]
    {
        rustudio_logitech_c920::win::KsCamera::open().map(|c| Box::new(c) as Box<dyn Transport>)
    }
    #[cfg(not(windows))]
    {
        Err("Le module Logitech C920 réel nécessite Windows".into())
    }
}

// Registre des hubs associes a chaque `Arc<Ctl>` (aucune modification requise dans controller.rs).
// Les entrees dont le `Ctl` est libere sont nettoyees automatiquement avant chaque recherche.
static REGISTRY: OnceLock<Mutex<Vec<(Weak<Ctl>, Arc<WebcamHub>)>>> = OnceLock::new();

fn registry() -> &'static Mutex<Vec<(Weak<Ctl>, Arc<WebcamHub>)>> {
    REGISTRY.get_or_init(|| Mutex::new(Vec::new()))
}

fn find_by_ptr(ptr: *const Ctl) -> Option<Arc<WebcamHub>> {
    let mut map = registry().lock().unwrap_or_else(|e| e.into_inner());
    map.retain(|(w, _)| w.strong_count() > 0);
    map.iter().find(|(w, _)| w.as_ptr() == ptr).map(|(_, h)| h.clone())
}

/// Retourne ou cree le `WebcamHub` associe a ce controleur.
pub fn hub_for(ctl: &Arc<Ctl>) -> Arc<WebcamHub> {
    let ptr = Arc::as_ptr(ctl);
    let mut map = registry().lock().unwrap_or_else(|e| e.into_inner());
    map.retain(|(w, _)| w.strong_count() > 0);
    if let Some((_, existing)) = map.iter().find(|(w, _)| w.as_ptr() == ptr) {
        return existing.clone();
    }
    let store_path = ctl.persist.then(|| ctl.cfg.data_dir.join("c920.json"));
    let hub = Arc::new(WebcamHub::new(ctl.simulated(), store_path));
    map.push((Arc::downgrade(ctl), hub.clone()));
    hub
}

/// Demarre le fil de suivi de la webcam pour ce controleur.
pub fn start(ctl: &Arc<Ctl>) {
    hub_for(ctl).start();
}

pub fn runtime(ctl: &Ctl) -> ModuleRuntime {
    if let Some(hub) = find_by_ptr(ctl as *const Ctl) {
        return hub.runtime();
    }
    let simulated = ctl.simulated();
    let settings = simulated
        .then(|| Camera::new(rustudio_logitech_c920::sim::Sim::default()).read().ok())
        .flatten();
    ModuleRuntime {
        id: ID,
        status: if settings.is_some() { ModuleStatus::Ready } else { ModuleStatus::Disconnected },
        connected: settings.is_some(),
        simulated,
        firmware: settings.and_then(|s| s.device.firmware),
        firmware_kind: None,
        error: None,
    }
}

pub fn state(ctl: &Ctl) -> Value {
    if let Some(hub) = find_by_ptr(ctl as *const Ctl) {
        return hub.state();
    }
    let simulated = ctl.simulated();
    let payload = simulated
        .then(|| Camera::new(rustudio_logitech_c920::sim::Sim::default()).read().ok())
        .flatten()
        .map(|settings| WebcamState {
            settings,
            locked: false,
            active_preset: None,
            reapply_count: 0,
            last_reapplied_fields: Vec::new(),
            backoff_fields: Vec::new(),
        });
    json!({
        "state": payload,
        "v": 0u64,
        "meta": { "presets": presets::summaries() },
        "live": { "locked": false, "reapply_count": 0u64, "active_preset": Value::Null }
    })
}

pub fn command(ctl: &Arc<Ctl>, action: &str, body: &Value) -> Result<Value, String> {
    let hub = find_by_ptr(Arc::as_ptr(ctl))
        .ok_or_else(|| "Le module webcam n’est pas démarré".to_owned())?;
    hub.command(action, body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    static SEQ: AtomicUsize = AtomicUsize::new(0);

    fn temp_file(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "rustudio-c920-{tag}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn verrouillage_reapplique_apres_reset_externe_et_reconnexion() {
        let reset_flag = temp_file("reset");
        let disconnect_flag = temp_file("disc");
        let store = temp_file("store").with_extension("json");
        let _ = std::fs::remove_file(&reset_flag);
        let _ = std::fs::remove_file(&disconnect_flag);
        let _ = std::fs::remove_file(&store);

        std::env::set_var("RUSTUDIO_SIM_C920_RESET_FILE", &reset_flag);
        std::env::set_var("RUSTUDIO_SIM_C920_DISCONNECT_FILE", &disconnect_flag);

        let hub = WebcamHub::new(true, Some(store.clone()));
        hub.start_with_poll(Duration::from_millis(80), Duration::from_millis(80));

        // 1. Applique le preset Studio puis active « Garder mes réglages ».
        let studio = hub.command("preset", &json!({"id": "studio"})).unwrap();
        assert_eq!(studio["state"]["exposure_auto"], false);
        assert_eq!(studio["state"]["low_light_comp"], false);
        assert_eq!(studio["state"]["active_preset"], "studio");

        let locked = hub.command("lock", &json!({"enabled": true})).unwrap();
        assert_eq!(locked["state"]["locked"], true);
        assert!(store.exists(), "la configuration verrouillee doit etre persistee");

        // 2. Simule un reset externe (OBS/Discord) : le garde-fou doit reappliquer automatiquement.
        std::fs::write(&reset_flag, b"1").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while hub.state()["state"]["reapply_count"].as_u64().unwrap_or(0) < 1 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(40));
        }
        let after_reset = hub.state();
        assert!(after_reset["state"]["reapply_count"].as_u64().unwrap_or(0) >= 1, "reapplication attendue : {after_reset}");
        assert_eq!(after_reset["state"]["exposure_auto"], false);
        assert_eq!(after_reset["state"]["low_light_comp"], false);
        assert_eq!(after_reset["state"]["power_line"], "hz50");

        // 3. Simule un debranchement puis rebranchement : le statut passe Disconnected puis Ready et reapplique.
        let before_replug = after_reset["state"]["reapply_count"].as_u64().unwrap_or(0);
        std::fs::write(&disconnect_flag, b"1").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while hub.runtime().status != ModuleStatus::Disconnected && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(40));
        }
        assert_eq!(hub.runtime().status, ModuleStatus::Disconnected);

        std::fs::remove_file(&disconnect_flag).unwrap();
        let deadline = Instant::now() + Duration::from_secs(6);
        while (hub.runtime().status != ModuleStatus::Ready
            || hub.state()["state"]["reapply_count"].as_u64().unwrap_or(0) <= before_replug)
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(hub.runtime().status, ModuleStatus::Ready);
        let after_replug = hub.state();
        assert!(after_replug["state"]["reapply_count"].as_u64().unwrap_or(0) > before_replug);
        assert_eq!(after_replug["state"]["exposure_auto"], false);
        assert_eq!(after_replug["state"]["low_light_comp"], false);

        // 4. Persistance : un second hub charge le meme fichier et retrouve le verrou + les reglages souhaites.
        let hub2 = WebcamHub::new(true, Some(store.clone()));
        assert_eq!(hub2.state()["state"]["locked"], true);
        assert_eq!(hub2.state()["state"]["active_preset"], "studio");

        std::env::remove_var("RUSTUDIO_SIM_C920_RESET_FILE");
        std::env::remove_var("RUSTUDIO_SIM_C920_DISCONNECT_FILE");
        let _ = std::fs::remove_file(&reset_flag);
        let _ = std::fs::remove_file(&disconnect_flag);
        let _ = std::fs::remove_file(&store);
    }
}
