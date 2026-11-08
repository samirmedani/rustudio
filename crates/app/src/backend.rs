use rustudio_core::{
    consts::{CHANNEL_COLOURS, CHANNEL_LABELS, FADERS, MUTE_TARGET_LABELS, channel_input},
    mixer_profile::MixerSettings,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Duration;

const MODULE: &str = "/api/modules/goxlr-mini";
const PATH: &str = "/api/modules/goxlr-mini/mixer";

#[derive(Clone, Deserialize, PartialEq, Debug)]
pub struct Descriptor {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub summary: String,
    #[serde(default)]
    pub origin: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Clone, Deserialize, PartialEq, Debug)]
pub struct Device {
    pub id: String,
    pub status: String,
    pub simulated: bool,
    pub firmware: Option<String>,
}

#[derive(Clone, Deserialize, PartialEq, Debug)]
pub struct Inventory {
    pub installed: Vec<Descriptor>,
    pub devices: Vec<Device>,
}

impl Snapshot {
    pub fn settings(&self) -> MixerSettings {
        MixerSettings {
            volumes: self
                .channels
                .iter()
                .map(|c| (c.id.clone(), c.volume))
                .collect(),
            faders: self
                .faders
                .iter()
                .map(|f| (f.id.clone(), f.channel.clone()))
                .collect(),
            mute_modes: self
                .faders
                .iter()
                .map(|f| (f.id.clone(), f.mode.clone()))
                .collect(),
            cuts: self
                .channels
                .iter()
                .map(|c| (c.id.clone(), c.mute.clone()))
                .collect(),
        }
    }
}

#[derive(Clone, Deserialize, PartialEq, Debug)]
pub struct Snapshot {
    pub cursor: String,
    pub boot: String,
    pub connected: bool,
    pub health: String,
    pub simulated: bool,
    pub faders: [Fader; 4],
    pub channels: Vec<Channel>,
    pub targets: Vec<Target>,
}

#[derive(Clone, Deserialize, PartialEq, Debug)]
pub struct Fader {
    pub id: String,
    pub channel: String,
    pub position: Option<u8>,
    pub mode: String,
}

#[derive(Clone, Deserialize, PartialEq, Debug)]
pub struct Channel {
    pub id: String,
    pub label: String,
    pub colour: String,
    pub volume: u8,
    pub mute: Option<String>,
    pub input: bool,
}

#[derive(Clone, Deserialize, PartialEq, Debug)]
pub struct Target {
    pub id: String,
    pub label: String,
}

#[derive(Clone)]
pub struct Client {
    agent: ureq::Agent,
    base: String,
    live: bool,
    legacy: Arc<AtomicBool>,
    mixer_visible: Arc<AtomicBool>,
    mouse_visible: Arc<AtomicBool>,
    go_visible: Arc<AtomicBool>,
    audio_visible: Arc<AtomicBool>,
    cached: Arc<Mutex<Option<(u64, Snapshot)>>>,
}

impl Client {
    pub fn new(port: u16, live: bool) -> Self {
        Self {
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_millis(1600))
                .redirects(0)
                .build(),
            base: format!("http://127.0.0.1:{port}"),
            live,
            legacy: Arc::new(AtomicBool::new(false)),
            mixer_visible: Arc::new(AtomicBool::new(false)),
            mouse_visible: Arc::new(AtomicBool::new(false)),
            go_visible: Arc::new(AtomicBool::new(false)),
            audio_visible: Arc::new(AtomicBool::new(false)),
            cached: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_mixer_visible(&self, visible: bool) {
        self.mixer_visible.store(visible, Ordering::Relaxed);
    }

    pub fn set_audio_visible(&self, visible: bool) {
        self.audio_visible.store(visible, Ordering::Relaxed);
    }
    pub fn set_go_visible(&self, visible: bool) {
        self.go_visible.store(visible, Ordering::Relaxed);
    }
    pub fn overlay_url(&self) -> String {
        format!("{}/overlay", self.base)
    }
    pub(crate) fn send_voice(&self, source: Option<(u64, u32, Vec<u8>)>) -> Result<Value, String> {
        let live = self.live_state()?;
        if live["connected"] != true || source.is_some() != self.live {
            return Err("La capture ne correspond pas au mode du moteur.".into());
        }
        let (query, bytes) = match source {
            Some((time, rate, bytes)) => (format!("rate={rate}&t0={time}&format=s16"), bytes),
            None => ("synthetic=1".into(), Vec::new()),
        };
        let request = self
            .agent
            .post(&format!(
                "{}/api/modules/goxlr-mini/voice-audio?{query}",
                self.base
            ))
            .timeout(Duration::from_secs(15))
            .set("X-Rustudio", "1")
            .set("Content-Type", "application/octet-stream");
        match request.send_bytes(&bytes) {
            Ok(response) => response
                .into_json()
                .map_err(|_| "Résultat de l’empreinte illisible.".into()),
            Err(ureq::Error::Status(_, response)) => Err(response
                .into_json::<Value>()
                .ok()
                .and_then(|v| v["error"].as_str().map(str::to_owned))
                .unwrap_or_else(|| "Le moteur a refusé la mesure.".into())),
            Err(e) => Err(format!("Empreinte non confirmée : {e}")),
        }
    }

    pub(crate) fn post_module(
        &self,
        module: &str,
        action: &str,
        params: Value,
    ) -> Result<Value, String> {
        match self
            .agent
            .post(&format!("{}{module}/{action}", self.base))
            .timeout(Duration::from_secs(23))
            .set("X-Rustudio", "1")
            .send_json(params)
        {
            Ok(r) => r
                .into_json()
                .map_err(|_| "Réponse du moteur illisible.".into()),
            Err(ureq::Error::Status(_, r)) => Err(r
                .into_json::<Value>()
                .ok()
                .and_then(|v| v["error"].as_str().map(str::to_owned))
                .unwrap_or_else(|| "Commande refusée.".into())),
            Err(e) => Err(format!("Commande non confirmée : {e}")),
        }
    }

    pub fn set_mouse_visible(&self, visible: bool) {
        self.mouse_visible.store(visible, Ordering::Relaxed);
    }

    pub fn mouse_view(&self) -> Result<crate::mouse::MouseView, String> {
        let view: crate::mouse::MouseView =
            serde_json::from_value(self.get(&format!("{}/state", crate::mouse::MODULE))?)
                .map_err(|_| "Réponse R5 Ultra illisible")?;
        if view.device.simulated == self.live {
            return Err("Mode souris incompatible.".into());
        }
        Ok(view)
    }

    pub fn mouse_command(
        &self,
        action: &str,
        params: &Value,
    ) -> Result<crate::mouse::Settings, String> {
        if !matches!(action, "settings" | "refresh") {
            return Err("Commande souris inconnue.".into());
        }
        self.live_state()?;
        let result = self
            .agent
            .post(&format!("{}{}/{}", self.base, crate::mouse::MODULE, action))
            .timeout(Duration::from_secs(23))
            .set("X-Rustudio", "1")
            .send_json(params.clone());
        let response = match result {
            Ok(response) => response,
            Err(ureq::Error::Status(_, response)) => {
                return Err(response
                    .into_json::<Value>()
                    .ok()
                    .and_then(|v| v["error"].as_str().map(str::to_owned))
                    .unwrap_or_else(|| "Commande refusée.".into()));
            }
            Err(e) => return Err(format!("Commande souris non confirmée : {e}")),
        };
        let body: Value = response
            .into_json()
            .map_err(|_| "Réponse souris illisible")?;
        if body["ok"] != true {
            return Err("Commande souris non confirmée.".into());
        }
        serde_json::from_value(body["state"].clone())
            .map_err(|_| "Réglages relus illisibles.".into())
    }

    pub fn is_live(&self) -> bool {
        self.live
    }

    pub(crate) fn get(&self, path: &str) -> Result<Value, String> {
        self.agent
            .get(&format!("{}{path}", self.base))
            .set("X-Rustudio", "1")
            .call()
            .map_err(|e| format!("Moteur inaccessible : {e}"))?
            .into_json()
            .map_err(|_| "Réponse du moteur illisible".into())
    }

    pub(crate) fn live_state(&self) -> Result<Value, String> {
        let state = self.get("/api/live")?;
        if state["sim"].as_bool() != Some(!self.live) {
            return Err("Mode du moteur incompatible.".into());
        }
        Ok(state)
    }

    pub fn inventory(&self) -> Result<Inventory, String> {
        self.live_state()?;
        let result: Inventory = serde_json::from_value(self.get("/api/modules")?)
            .map_err(|_| "Inventaire des appareils illisible")?;
        if result.devices.iter().any(|d| d.simulated == self.live) {
            return Err("Inventaire issu d’un autre mode.".into());
        }
        Ok(result)
    }

    fn legacy_snapshot(&self) -> Result<Snapshot, String> {
        let live = self.live_state()?;
        let version = live["v"].as_u64().ok_or("Version d’état manquante")?;
        let boot = live["boot"].as_str().ok_or("Session moteur manquante")?;
        let health = live["health"]
            .as_str()
            .ok_or("État de connexion manquant")?;
        let positions: [Option<u8>; 4] =
            serde_json::from_value(live["live"]["fader_positions"].clone())
                .map_err(|_| "Positions des faders illisibles")?;
        let mut cache = self.cached.lock().map_err(|_| "Mixeur inaccessible")?;
        if cache
            .as_ref()
            .is_none_or(|(v, s)| *v != version || s.boot != boot)
        {
            let response = self.get(&format!("{MODULE}/state"))?;
            let raw = response["state"]
                .as_object()
                .ok_or("Réglages du mixeur manquants")?;
            let state = rustudio_core::state::normalize_state(&Value::Object(raw.clone()));
            let channels = CHANNEL_LABELS
                .iter()
                .zip(CHANNEL_COLOURS)
                .map(|(&(id, label), (_, colour))| Channel {
                    id: id.into(),
                    label: label.into(),
                    colour: colour.into(),
                    volume: state.volume(id) as u8,
                    mute: if state.muted(id) {
                        Some("all".into())
                    } else {
                        state.partial_of(id).map(str::to_owned)
                    },
                    input: channel_input(id).is_some(),
                })
                .collect();
            let faders = FADERS.map(|id| Fader {
                id: id.into(),
                channel: state.fader_channel(id).into(),
                mode: state
                    .mute_modes
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| "all".into()),
                position: None,
            });
            *cache = Some((
                version,
                Snapshot {
                    cursor: String::new(),
                    boot: boot.into(),
                    connected: false,
                    health: String::new(),
                    simulated: !self.live,
                    faders,
                    channels,
                    targets: MUTE_TARGET_LABELS
                        .iter()
                        .map(|&(id, label)| Target {
                            id: id.into(),
                            label: label.into(),
                        })
                        .collect(),
                },
            ));
        }
        let mut snapshot = cache.as_ref().unwrap().1.clone();
        snapshot.health = health.into();
        snapshot.connected = live["connected"] == true;
        snapshot.cursor = format!("{boot}-{version}-{health}-{:?}", positions);
        for (f, p) in snapshot.faders.iter_mut().zip(positions) {
            f.position = p;
        }
        Ok(snapshot)
    }

    pub fn snapshot(&self, after: Option<&str>) -> Result<Snapshot, String> {
        if self.legacy.load(Ordering::Relaxed) {
            if after.is_some() {
                std::thread::sleep(Duration::from_millis(
                    if self.mixer_visible.load(Ordering::Relaxed) {
                        35
                    } else {
                        350
                    },
                ));
            }
            return self.legacy_snapshot();
        }
        let url = match after {
            Some(cursor) => format!("{}{PATH}?after={cursor}", self.base),
            None => format!("{}{PATH}", self.base),
        };
        let response = match self.agent.get(&url).set("X-Rustudio", "1").call() {
            Ok(response) => response,
            // La version Windows déjà distribuée n’a pas encore la route à attente longue.
            Err(ureq::Error::Status(404, _)) if self.live => {
                self.legacy.store(true, Ordering::Relaxed);
                return self.legacy_snapshot();
            }
            Err(e) => return Err(format!("Moteur inaccessible : {e}")),
        };
        let result = response
            .into_json::<Snapshot>()
            .map_err(|_| "Réponse du mixeur illisible".to_owned())?;
        if result.simulated == self.live
            || !result
                .cursor
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err("Mode du moteur incompatible.".into());
        }
        Ok(result)
    }

    fn legacy_command(&self, cmd: &Command) -> Result<(String, Value), String> {
        let before = self.live_state()?;
        if before["boot"] != cmd.boot || before["health"] != "ok" {
            return Err("Le moteur a changé de session ou l’appareil est déconnecté.".into());
        }
        let action = match cmd.action {
            "volume" | "mute" | "fader" | "settings" => {
                return Ok((format!("{MODULE}/{}", cmd.action), cmd.params.clone()));
            }
            "mixer-profile" => "profile",
            _ => return Err("Commande du mixeur inconnue.".into()),
        };
        if before["app_version"] != "5.6.14" {
            return Err(
                "Cette version du moteur ne sait pas appliquer un profil du mixeur."
                    .into(),
            );
        }
        let mut raw = self.get(&format!("{MODULE}/state"))?["state"].clone();
        if !raw.is_object() {
            return Err("Réglages du mixeur manquants.".into());
        }
        let profile: MixerSettings =
            serde_json::from_value(cmd.params.clone()).map_err(|_| "Profil invalide")?;
        let mut state = rustudio_core::state::normalize_state(&raw);
        profile.apply_to(&mut state)?;
        let updated = state.to_value();
        for key in [
            "volumes",
            "faders",
            "mutes",
            "partial",
            "mute_modes",
            "scene",
        ] {
            raw[key] = updated[key].clone();
        }
        let after = self.live_state()?;
        if after["boot"] != cmd.boot || after["health"] != "ok" || before["v"] != after["v"] {
            return Err("Le mixeur a changé pendant la préparation du profil. Réessaie.".into());
        }
        Ok((format!("{MODULE}/{action}"), json!({"profile":raw})))
    }

    pub fn send(&self, cmd: &Command) -> Result<(), String> {
        let (path, body) = if self.legacy.load(Ordering::Relaxed) {
            self.legacy_command(cmd)?
        } else {
            (
                PATH.into(),
                json!({"boot": cmd.boot, "mode":if self.live {"live"} else {"demo"}, "action": cmd.action, "params": cmd.params}),
            )
        };
        let result = self
            .agent
            .post(&format!("{}{path}", self.base))
            .set("X-Rustudio", "1")
            .send_json(body);
        match result {
            Ok(r) => {
                let response = r
                    .into_json::<Value>()
                    .map_err(|_| "Réponse de commande illisible".to_owned())?;
                if response["ok"] == true {
                    Ok(())
                } else {
                    Err("Commande non confirmée".into())
                }
            }
            Err(ureq::Error::Status(_, response)) => {
                let detail = response
                    .into_json::<Value>()
                    .ok()
                    .and_then(|v| v["error"].as_str().map(str::to_owned));
                Err(detail.unwrap_or_else(|| "Commande refusée".into()))
            }
            Err(e) => Err(format!("Commande non envoyée : {e}")),
        }
    }
}

pub struct Command {
    pub id: u64,
    pub boot: String,
    pub action: &'static str,
    pub params: Value,
}

pub enum Event {
    Snapshot(Option<Snapshot>),
    Done(u64, Result<(), String>),
    Inventory(Option<Inventory>),
    Mouse(Option<crate::mouse::MouseView>),
    Go(Option<crate::goxlr::GoView>),
    Audio(Value),
    MicOsd(String),
    Update(Value),
}

pub fn start(
    client: Client,
    first: Option<&Snapshot>,
    native_osd: bool,
) -> (mpsc::SyncSender<Command>, async_channel::Receiver<Event>) {
    let (tx, rx) = mpsc::sync_channel::<Command>(16);
    let (event_tx, event_rx) = async_channel::unbounded();
    let read_tx = event_tx.clone();
    let reader = client.clone();
    let first_cursor = first.map(|state| state.cursor.clone());
    let mouse_client = client.clone();
    let mouse_tx = event_tx.clone();
    std::thread::spawn(move || {
        let mut last = None;
        let mut last_go = None;
        while !mouse_tx.is_closed() {
            if mouse_client.mouse_visible.load(Ordering::Relaxed) {
                let current = mouse_client.mouse_view().ok();
                if current != last {
                    last = current.clone();
                    if mouse_tx.send_blocking(Event::Mouse(current)).is_err() {
                        break;
                    }
                }
            }
            if mouse_client.go_visible.load(Ordering::Relaxed) {
                let current = mouse_client.go_view().ok();
                if current != last_go {
                    last_go = current.clone();
                    if mouse_tx.send_blocking(Event::Go(current)).is_err() {
                        break;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(
                if mouse_client.mouse_visible.load(Ordering::Relaxed)
                    || mouse_client.go_visible.load(Ordering::Relaxed)
                {
                    750
                } else {
                    250
                },
            ));
        }
    });
    let session_client = client.clone();
    let session_tx = event_tx.clone();
    std::thread::spawn(move || {
        let mut audio = None;
        let mut last_read = std::time::Instant::now();
        let mut mic: Option<String> = None;
        while !session_tx.is_closed() {
            if session_client.audio_visible.load(Ordering::Relaxed)
                && (audio.is_none() || last_read.elapsed() >= Duration::from_secs(3))
            {
                let current = crate::win_session::inventory(session_client.live);
                last_read = std::time::Instant::now();
                if audio.as_ref() != Some(&current) {
                    audio = Some(current.clone());
                    if session_tx.send_blocking(Event::Audio(current)).is_err() {
                        break;
                    }
                }
            } else if !session_client.audio_visible.load(Ordering::Relaxed) {
                audio = None;
            }
            if native_osd {
                if let Ok(state) = session_client.live_state() {
                    if state["connected"] == true {
                        let value = if state["mic"]["muted"] == true {
                            "all"
                        } else {
                            state["mic"]["partial"].as_str().unwrap_or("")
                        };
                        if mic.as_deref().is_some_and(|last| last != value) && state["osd"] == true
                        {
                            if session_tx
                                .send_blocking(Event::MicOsd(value.into()))
                                .is_err()
                            {
                                break;
                            }
                        }
                        mic = Some(value.into());
                    } else {
                        mic = None;
                    }
                } else {
                    mic = None;
                }
            }
            std::thread::sleep(Duration::from_millis(if native_osd { 150 } else { 350 }));
        }
    });
    let inventory_client = client.clone();
    let inventory_tx = event_tx.clone();
    std::thread::spawn(move || {
        let mut last = None;
        let mut last_update: Option<Value> = None;
        let mut tick = 0u64;
        while !inventory_tx.is_closed() {
            if inventory_client.is_live() && tick % 3 == 0 {
                if let Ok(mut update) = inventory_client.get("/api/update") {
                    // compte à rebours de la recherche manuelle : inutile ici, et il changerait à chaque lecture
                    update.as_object_mut().map(|o| o.remove("check_wait"));
                    if last_update.as_ref() != Some(&update) {
                        last_update = Some(update.clone());
                        if inventory_tx.send_blocking(Event::Update(update)).is_err() {
                            break;
                        }
                    }
                }
            }
            tick += 1;
            let current = inventory_client.inventory().ok();
            if current != last {
                last = current.clone();
                if inventory_tx
                    .send_blocking(Event::Inventory(current))
                    .is_err()
                {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(650));
        }
    });
    std::thread::spawn(move || {
        let mut cursor = first_cursor;
        while !read_tx.is_closed() {
            match reader.snapshot(cursor.as_deref()) {
                Ok(state) if cursor.as_deref() != Some(&state.cursor) => {
                    cursor = Some(state.cursor.clone());
                    if read_tx.send_blocking(Event::Snapshot(Some(state))).is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(_) => {
                    if cursor.take().is_some() {
                        let _ = read_tx.send_blocking(Event::Snapshot(None));
                    }
                    std::thread::sleep(Duration::from_millis(350));
                }
            }
        }
    });
    std::thread::spawn(move || {
        for cmd in rx {
            let result = client.send(&cmd);
            if event_tx.send_blocking(Event::Done(cmd.id, result)).is_err() {
                break;
            }
        }
    });
    (tx, event_rx)
}

pub fn verify(client: &Client, initial: &Snapshot) -> Result<(), String> {
    if !initial.connected {
        return Err("Le simulateur doit être connecté pour le test des commandes.".into());
    }
    let boot = initial.boot.clone();
    let channel = initial.faders[0].channel.clone();
    let fader = initial.faders[0].id.clone();
    let replacement = initial.faders[1].channel.clone();
    let mut state = initial.clone();
    let actions = [
        ("volume", json!({"channel": channel, "value": 181})),
        (
            "mute",
            json!({"channel": channel, "target": "all", "muted": true}),
        ),
        (
            "mute",
            json!({"channel": channel, "target": "all", "muted": false}),
        ),
        ("fader", json!({"fader": fader, "channel": replacement})),
        (
            "settings",
            json!({"mute_modes": {(fader.clone()): "stream"}}),
        ),
    ];
    for (i, (action, params)) in actions.into_iter().enumerate() {
        client.send(&Command {
            id: i as u64,
            boot: boot.clone(),
            action,
            params,
        })?;
        state = client.snapshot(Some(&state.cursor))?;
        let row = state
            .channels
            .iter()
            .find(|c| c.id == channel)
            .ok_or("voie perdue")?;
        let valid = match i {
            0 => row.volume == 181,
            1 => row.mute.as_deref() == Some("all"),
            2 => row.mute.is_none(),
            3 => state.faders[0].channel == replacement && state.faders[1].channel == channel,
            4 => state.faders[0].mode == "stream",
            _ => false,
        };
        if !valid {
            return Err(format!("La relecture n’a pas confirmé {action}."));
        }
    }
    if client
        .send(&Command {
            id: 6,
            boot: "autre".into(),
            action: "volume",
            params: json!({"channel": channel,"value":0}),
        })
        .is_ok()
    {
        return Err("Le moteur a accepté une commande d’une autre session.".into());
    }
    println!("OK : volume, coupure, réactivation, permutation, cible et session isolée.");
    Ok(())
}
