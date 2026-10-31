//! Etat + boucle USB. Seule device_loop() parle a l'appareil ; le reste modifie l'etat sous le verrou et marque
//! ce qui doit partir. L'API repond donc toujours, meme si le pilote se bloque.
use crate::api::{need_str, panic_text, ApiError, ApiResult, Event};
pub use crate::inputs::{fader_filtered, FADER_DEADZONE};
use crate::config::{now_epoch, Config};
use crate::log::log;
use crate::logf;
use rustudio_core::consts::*;
use rustudio_core::dsp;
use rustudio_core::state::{self, State};
use rustudio_core::value::{get, has, parse_bool};
use rustudio_core::{lighting, routing};
use rustudio_goxlr::sim::Sim;
use rustudio_goxlr::{f32_bytes, level_db, DevError, DevInfo, GoXlr, Loader};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Key {
    All,
    Faders,
    Mic,
    Dsp,
    Lighting,
    Colours,
    Buttons,
    Submix,
    Volume(usize),
    Mute(usize),
    Route(usize),
}

pub fn ch(name: &str) -> usize {
    channel_index(name).unwrap_or(0)
}
pub fn inp(name: &str) -> usize {
    INPUTS.iter().position(|(n, _, _)| *n == name).unwrap_or(0)
}

#[derive(Clone, Debug, Default)]
pub struct Live {
    pub fader_positions: [Option<u8>; 4],
    pub mic_db: Option<f64>,
    pub level_t: Option<Instant>,
    pub pressed: Vec<&'static str>,
    pub encoders: [i8; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub enum AutoActive {
    Off,
    Unknown,
    On(String, String),
}

pub struct Inner {
    pub state: State,
    pub connected: bool,
    pub present: Option<bool>,
    pub last_error: Option<String>,
    pub connected_since: Option<f64>,
    pub conn_mono: Option<Instant>,
    pub live: Live,
    pub version: u64,
    pub job: Option<serde_json::Map<String, Value>>,
    pub fast_level: bool,
    pub level_until: Option<Instant>,
    pub dirty: HashSet<Key>,
    pub want: u64,
    pub done: u64,
    pub save_dirty: bool,
    pub prev_mask: u32,
    pub prev_pos: Option<[u8; 4]>,
    pub pos_history: [[u8; 2]; 4],
    pub faders_seen: bool,
    pub btn: HashMap<&'static str, (Instant, bool)>,
    pub bleep: bool,
    pub blink: HashMap<String, bool>,
    pub gain_override: Option<(String, i32)>,
    pub listen_until: Option<Instant>,
    /// test « son qui crépite » : la voie Sample part seulement vers le Stream Mix jusqu'à cet instant
    pub sound_test_until: Option<Instant>,
    pub listen_level: i32,
    pub force_reconnect: bool,
    pub usb: Option<String>,
    pub hp_live: bool,
    pub errors: VecDeque<(f64, String)>,
    pub connects: u64,
    pub reconnects: u64,
    pub fail_streak: u32,
    pub auto_active: AutoActive,
    pub auto_note: Option<Value>,
    pub auto_seen: bool,
    pub mix2: bool,
    pub submixes: bool,
    wake: Arc<Event>,
}

pub enum Op {
    Fader(usize, usize),
    Volume(usize, i32),
    MonitoredMix(u8),
    ChannelMixes,
    ChannelState(usize, bool),
    Effects(Vec<(u32, i32)>),
    Routing(&'static str, indexmap::IndexMap<String, bool>),
    MicGain(String, i32),
    MicParams(Vec<(u32, [u8; 4])>),
    FaderDisplay(usize, bool, bool),
    Animation(u8, i32, i32, u8),
    ColourMap(Vec<u8>),
    ButtonStates(Vec<u8>),
}

fn exec(dev: &mut GoXlr, op: Op) -> Result<(), DevError> {
    match op {
        Op::Fader(f, c) => dev.set_fader(f, c),
        Op::Volume(c, v) => dev.set_volume(c, v),
        Op::MonitoredMix(m) => dev.set_monitored_mix(m),
        Op::ChannelMixes => dev.set_channel_mixes(),
        Op::ChannelState(c, m) => dev.set_channel_state(c, m),
        Op::Effects(e) => dev.set_effects(&e),
        Op::Routing(i, r) => dev.set_routing(i, &r),
        Op::MicGain(t, g) => dev.set_mic_gain(&t, g),
        Op::MicParams(p) => dev.set_mic_params(&p),
        Op::FaderDisplay(f, g, m) => dev.set_fader_display(f, g, m),
        Op::Animation(a, m1, m2, w) => dev.set_animation(a, m1, m2, w),
        Op::ColourMap(d) => dev.set_colour_map(&d),
        Op::ButtonStates(b) => dev.set_button_states(&b),
    }
}

impl Inner {
    /// Marque des elements a envoyer ; la boucle USB les enverra avec la valeur la plus recente.
    pub fn mark(&mut self, keys: &[Key], bump: bool) -> u64 {
        self.dirty.extend(keys.iter().copied());
        self.want += 1;
        if bump {
            self.version += 1;
            self.save_dirty = true;
        }
        self.wake.set();
        self.want
    }
    pub fn listening(&self) -> bool {
        self.listen_until.is_some()
    }
    pub fn cal_for(&self) -> Option<&state::MicCal> {
        self.state.cal_for(None)
    }
    pub fn preset_dsp(&self, name: &str) -> dsp::Dsp {
        dsp::fit_dsp(name, self.cal_for(), Some(self.state.mic.gain_db))
    }
    /// Reglages du preregle courant recales sur la voix mesuree ; « Naturel » garde des reglages doux prets.
    pub fn refit(&self) -> dsp::Dsp {
        let name = self.state.mic_preset.clone();
        self.preset_dsp(if name == "brut" { "naturel" } else { &name })
    }
    pub fn effective_routing(&self, input: &str) -> indexmap::IndexMap<String, bool> {
        let mut r = routing::effective_routing(&self.state, input, self.mix2, self.listening());
        if input == "Samples" && self.sound_test_until.is_some() {
            // son de test : ni casque, ni Discord, ni enceintes ; seulement le Stream Mix, que le test réécoute
            for (out, on) in r.iter_mut() {
                *on = out == "BroadcastMix";
            }
            r.insert("BroadcastMix".into(), true);
        }
        r
    }
    pub fn vod_output(&self) -> &'static str {
        routing::vod_output(self.mix2)
    }

    /// Interrupteurs des traitements. Un traitement allume sur des reglages neutres reprend des reglages doux.
    pub fn set_fx(&mut self, p: &Value) {
        let soft = self.preset_dsp("naturel");
        for k in dsp::FX_KEYS {
            if !has(p, k) {
                continue;
            }
            let on = parse_bool(get(p, k));
            if on && !self.state.fx.get(k).unwrap_or(false) {
                match k {
                    "comp" if self.state.dsp.comp.ratio == 0 => self.state.dsp.comp = soft.comp.clone(),
                    "deesser" if self.state.dsp.deesser == 0 => self.state.dsp.deesser = 20,
                    "gate" => self.state.dsp.gate.enabled = true,
                    _ => {}
                }
            }
            self.state.fx.set(k, on);
        }
        self.state.mic_preset = if self.state.fx.any() { "custom".into() } else { "brut".into() };
        self.mark(&[Key::Dsp], true);
    }

    pub fn dsp_lists(&self) -> (Vec<(u32, [u8; 4])>, Vec<(u32, i32)>) {
        let s = &self.state;
        let d = dsp::effective_dsp(&s.dsp, &s.fx);
        let (g, c) = (&d.gate, &d.comp);
        let thr = if g.enabled { g.threshold } else { -59 };
        let att = dsp::gate_attenuation(if g.enabled { g.attenuation } else { 0 });
        let mut params = vec![
            (0x30200, f32_bytes(thr as f64)), (0x30400, f32_bytes(g.attack as f64)), (0x30600, f32_bytes(g.release as f64)),
            (0x30900, f32_bytes(att as f64)), (0x60200, f32_bytes(c.threshold as f64)),
            (0x60300, f32_bytes(dsp::ratio_x(c.ratio))), (0x60400, f32_bytes(c.attack as f64)),
            (0x60600, f32_bytes(c.release as f64)), (0x60700, f32_bytes(c.makeup as f64)),
            (0x70100, f32_bytes(s.bleep_db as f64 * 65536.0)),
        ];
        for (band, kf, kg) in EQ_BANDS {
            params.push((kf, f32_bytes(band.parse::<f64>().unwrap_or(0.0))));
            params.push((kg, f32_bytes(*d.eq.get(band).unwrap_or(&0) as f64)));
        }
        let effects = vec![
            (0x0011, thr), (0x0016, g.attack), (0x0017, g.release), (0x0015, att), (0x0014, 1), (0x0010, 2), (0x014B, 1),
            (0x013D, c.threshold), (0x013C, c.ratio), (0x013E, c.attack), (0x013F, c.release), (0x0140, c.makeup),
            (0x000B, d.deesser), (0x0073, s.bleep_db), (0x0158, s.muted("Mic") as i32),
        ];
        (params, effects)
    }

    pub fn plan(&self, keys: &HashSet<Key>) -> Vec<Op> {
        let s = &self.state;
        let all = keys.contains(&Key::All);
        let has = |k: Key| all || keys.contains(&k);
        let mut ops = vec![];
        if has(Key::Faders) {
            for (i, f) in FADERS.iter().enumerate() {
                ops.push(Op::Fader(i, ch(s.fader_channel(f))));
            }
        }
        let mon = if self.listening() { self.listen_level } else { s.volume("MicMonitor") };
        for (i, c) in CHANNELS.iter().enumerate() {
            if has(Key::Volume(i)) {
                ops.push(Op::Volume(i, if *c == "MicMonitor" { mon } else { s.volume(c) }));
            }
        }
        if has(Key::Submix) && self.submixes {
            // comme goxlr-utility : le casque ecoute le mix A, chaque sortie est sur le mix A, puis le retour micro
            ops.push(Op::MonitoredMix(0));
            ops.push(Op::ChannelMixes);
            ops.push(Op::Volume(ch("MicMonitor"), mon));
        }
        for (i, c) in CHANNELS.iter().enumerate() {
            if has(Key::Mute(i)) {
                ops.push(Op::ChannelState(i, s.muted(c)));
            }
        }
        if has(Key::Mute(ch("Mic"))) && !has(Key::Dsp) {
            ops.push(Op::Effects(vec![(0x0158, s.muted("Mic") as i32)]));
        }
        for (i, (name, _, _)) in INPUTS.iter().enumerate() {
            if has(Key::Route(i)) {
                ops.push(Op::Routing(name, self.effective_routing(name)));
            }
        }
        if has(Key::Mic) {
            let (t, g) = self.gain_override.clone().unwrap_or((s.mic.kind.clone(), s.mic.gain_db));
            ops.push(Op::MicGain(t, g));
        }
        if has(Key::Dsp) {
            let (params, effects) = self.dsp_lists();
            ops.push(Op::MicParams(params));
            ops.push(Op::Effects(effects));
        }
        if has(Key::Lighting) {
            let li = &s.lighting;
            for (i, f) in FADERS.iter().enumerate() {
                let (g, m) = fader_style(li.fader_style.get(*f).map(|x| x.as_str()).unwrap_or("gradient")).unwrap_or((true, false));
                ops.push(Op::FaderDisplay(i, g, m));
            }
            ops.push(Op::Animation(
                animation(&li.animation).unwrap_or(5), li.mod1, li.mod2, waterfall(&li.waterfall).unwrap_or(2),
            ));
        }
        if has(Key::Colours) {
            ops.push(Op::ColourMap(lighting::colour_map(s)));
        }
        if has(Key::Buttons) {
            ops.push(Op::ButtonStates(lighting::button_states(s, &self.blink, self.bleep)));
        }
        ops
    }

    pub fn note_usb(&mut self, seen: Option<String>) {
        if seen == self.usb {
            return;
        }
        let before = std::mem::replace(&mut self.usb, seen.clone());
        self.version += 1;
        let note = |k: &str| match k {
            "none" => "Windows ne voit aucune GoXLR sur l’USB : si elle est allumée, son câble ne fait peut-être que charger, ou le port n’est pas relié au PC",
            "failed" => "Un appareil USB n’arrive pas à se présenter à Windows (câble ou port abîmé ?) : c’est peut-être la GoXLR",
            "full" => "Une GoXLR complète est branchée : Rustudio ne pilote pour l’instant que la Mini",
            _ => "Windows voit la GoXLR sur l’USB",
        };
        match seen.as_deref() {
            Some(k @ ("none" | "failed" | "full")) => log(note(k)),
            Some("mini") if matches!(before.as_deref(), Some("none") | Some("failed")) => log(note("mini")),
            _ => {}
        }
    }
}

type CallFn = Box<dyn FnOnce(&mut GoXlr) -> Result<Value, DevError> + Send>;
struct Call {
    f: CallFn,
    tx: mpsc::Sender<Result<Value, String>>,
}

pub struct Ctl {
    pub cfg: Config,
    inner: Mutex<Inner>,
    pub wake: Arc<Event>,
    pub dev_info: Arc<Mutex<DevInfo>>,
    dev: Mutex<Option<GoXlr>>,
    calls: Mutex<VecDeque<Call>>,
    t0: Instant,
    heartbeat_ms: AtomicU64,
    pub updater: crate::updater::Updater,
    pub mouse: crate::mouse::MouseHub,
    pub eqapo: crate::eqapo::EqApo,
    pub sim: Option<Sim>,
    pub boot_id: String,
    pub started: f64,
    pub persist: bool,
    pub absent_retry: Mutex<Duration>,
    pub poll_interval: Mutex<Duration>,
    last_backup: Mutex<f64>,
    pub loop_running: AtomicBool,
    /// coupures du son comptees par le pilote, relues toutes les \`stats_interval\` par la boucle USB
    pub drops: Mutex<crate::drops::UsbDrops>,
    pub stats_interval: Mutex<Duration>,
    stats_at: Mutex<Option<Instant>>,
}

pub const DEFAULT_PROFILE: &str = include_str!("../../../ui/profile.json");

fn initial_state(cfg: &Config) -> Value {
    let sp = cfg.state_path();
    let bak = std::path::PathBuf::from(format!("{}.bak", sp.display()));
    for p in [sp.clone(), bak] {
        if p.exists() {
            match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string())) {
                Ok(v) => {
                    if p != sp {
                        logf!("État repris depuis {}", p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default());
                    }
                    return v;
                }
                Err(e) => logf!("Lecture {} impossible: {e}", p.display()),
            }
        }
    }
    if let Some(old) = crate::config::legacy_data_dir().map(|d| d.join("state.json")).filter(|p| p.exists()) {
        if let Ok(v) = std::fs::read_to_string(&old).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string())) {
            log("Réglages repris de GoXLR Mini");
            return v;
        }
    }
    serde_json::from_str(DEFAULT_PROFILE).unwrap_or(Value::Null)
}

impl Ctl {
    pub fn new(cfg: Config, loader: Loader, sim: Option<Sim>, persist: bool) -> Arc<Ctl> {
        let wake = Arc::new(Event::new());
        // premier lancement (reglages repris de GoXLR Mini ou profil par defaut) : on les ecrit tout de suite
        let first_run = !cfg.state_path().exists();
        let mut st = state::normalize_state(&initial_state(&cfg));
        if theme(&st.lighting.theme).is_some() {
            lighting::apply_theme(&mut st);
        }
        let mut dev = GoXlr::new(loader);
        dev.log = Box::new(|m| log(m));
        let dev_info = dev.info.clone();
        let drops_path = cfg.data_dir.join("usb-drops.json");
        let inner = Inner {
            state: st, connected: false, present: None, last_error: None, connected_since: None, conn_mono: None,
            live: Live::default(), version: 1, job: None, fast_level: false, level_until: None, dirty: HashSet::new(),
            want: 0, done: 0, save_dirty: first_run, prev_mask: 0, prev_pos: None, pos_history: [[0; 2]; 4],
            faders_seen: false, btn: HashMap::new(), bleep: false, blink: HashMap::new(), gain_override: None,
            listen_until: None, sound_test_until: None, listen_level: 0, force_reconnect: false, usb: None, hp_live: false,
            errors: VecDeque::new(), connects: 0, reconnects: 0, fail_streak: 0, auto_active: AutoActive::Off,
            auto_note: None, auto_seen: false, mix2: false, submixes: false, wake: wake.clone(),
        };
        let simulated = cfg.simulate || sim.is_some();
        let ctl = Arc::new(Ctl {
            updater: crate::updater::Updater::new(&cfg),
            mouse: crate::mouse::MouseHub::new(simulated),
            eqapo: crate::eqapo::EqApo::new(&cfg, simulated),
            cfg,
            inner: Mutex::new(inner),
            wake,
            dev_info,
            dev: Mutex::new(Some(dev)),
            calls: Mutex::new(VecDeque::new()),
            t0: Instant::now(),
            heartbeat_ms: AtomicU64::new(0),
            sim,
            boot_id: format!("{}", (now_epoch() * 1000.0) as u64),
            started: now_epoch(),
            persist,
            absent_retry: Mutex::new(Duration::from_secs(2)),
            poll_interval: Mutex::new(Duration::from_millis(15)),
            last_backup: Mutex::new(0.0),
            loop_running: AtomicBool::new(false),
            drops: Mutex::new(crate::drops::UsbDrops::load(drops_path, now_epoch())),
            stats_interval: Mutex::new(Duration::from_secs(1)),
            stats_at: Mutex::new(None),
        });
        // captures de la documentation : en mode simule seulement, la mise a jour proposee s'affiche installable
        if ctl.simulated() && std::env::var("RUSTUDIO_DEMO_UPDATE").as_deref() == Ok("1") {
            ctl.updater.force_installable.store(true, Ordering::Relaxed);
        }
        ctl
    }

    pub fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn simulated(&self) -> bool {
        self.cfg.simulate || self.sim.is_some()
    }
    pub fn beat(&self) {
        self.heartbeat_ms.store(self.t0.elapsed().as_millis() as u64, Ordering::Relaxed);
    }
    pub fn heartbeat_age(&self) -> Duration {
        let now = self.t0.elapsed().as_millis() as u64;
        Duration::from_millis(now.saturating_sub(self.heartbeat_ms.load(Ordering::Relaxed)))
    }
    /// Acces direct a l'appareil quand la boucle USB est arretee (tests, commande unique).
    pub fn with_dev<R>(&self, f: impl FnOnce(&mut GoXlr) -> R) -> Option<R> {
        self.dev.lock().unwrap_or_else(|e| e.into_inner()).as_mut().map(f)
    }
    pub fn version(&self) -> u64 {
        self.lock().version + self.updater.changes() + self.eqapo.changes() + self.mouse.changes()
    }

    fn usb_scan(&self) -> Option<String> {
        if let Some(sim) = &self.sim {
            return Some(sim.usb_scan());
        }
        if self.cfg.simulate {
            return None;
        }
        crate::win::usb_scan()
    }

    /// Ouvre l'appareil (boucle USB uniquement). EnumerateDevices n'est appele qu'avec le handle ferme.
    pub fn connect(&self, dev: &mut GoXlr, apply: bool) -> Result<(), DevError> {
        dev.close();
        dev.open()?;
        let (serial, fw, mix2) = {
            let i = dev.info.lock().unwrap();
            (i.serial.clone().unwrap_or_default(), i.firmware.clone().unwrap_or_default(), i.mix2())
        };
        {
            let mut g = self.lock();
            g.connected = true;
            g.present = Some(true);
            g.connected_since = Some(now_epoch());
            g.conn_mono = Some(Instant::now());
            g.last_error = None;
            g.prev_mask = 0;
            g.prev_pos = None;
            g.btn.clear();
            g.bleep = false;
            g.faders_seen = false;
            if g.connects > 0 {
                g.reconnects += 1;
            }
            g.connects += 1;
            g.version += 1;
            g.mix2 = mix2;
            g.submixes = dev.submixes();
            if apply {
                g.dirty = [Key::All].into_iter().collect();
                g.want += 1;
            }
        }
        logf!("GoXLR connectée : série {serial}, firmware {fw}{}", if mix2 { " (Stream Mix 2)" } else { "" });
        if apply {
            let n = self.flush(dev)?;
            logf!("État complet appliqué ({n} envois)");
        }
        Ok(())
    }

    pub fn lost(&self, dev: &mut GoXlr, msg: &str) {
        let was = {
            let mut g = self.lock();
            let was = g.connected;
            g.connected = false;
            g.connected_since = None;
            g.conn_mono = None;
            g.last_error = Some(msg.into());
            g.live.fader_positions = [None; 4];
            g.live.pressed.clear();
            if was {
                g.version += 1;
                if g.errors.len() >= 15 {
                    g.errors.pop_front();
                }
                g.errors.push_back((now_epoch(), msg.into()));
            }
            was
        };
        dev.close();
        self.drops.lock().unwrap_or_else(|e| e.into_inner()).reset();
        if was {
            logf!("GoXLR perdue : {msg}");
        }
    }

    /// Envoie ce qui est marque. A appeler uniquement depuis la boucle USB.
    pub fn flush(&self, dev: &mut GoXlr) -> Result<usize, DevError> {
        let (gen, ops) = {
            let mut g = self.lock();
            if g.dirty.is_empty() {
                return Ok(0);
            }
            let keys = std::mem::take(&mut g.dirty);
            (g.want, g.plan(&keys))
        };
        let n = ops.len();
        for op in ops {
            let volume_channel = match &op { Op::Volume(c, _) => Some(*c), _ => None };
            exec(dev, op)?;
            if let Some(channel) = volume_channel {
                let mut g = self.lock();
                for (i, fader) in FADERS.iter().enumerate() {
                    if ch(g.state.fader_channel(fader)) == channel {
                        // La commande ne deplace pas la poignee : repartir de la derniere
                        // lecture physique, jamais du volume logique demande.
                        let physical = g.pos_history[i][1];
                        g.pos_history[i] = [physical; 2];
                        if let Some(ref mut prev) = g.prev_pos { prev[i] = physical; }
                    }
                }
            }
            self.beat();
        }
        let mut g = self.lock();
        if gen > g.done {
            g.done = gen;
        }
        Ok(n)
    }

    fn fail_calls(&self, msg: &str) {
        let mut q = self.calls.lock().unwrap_or_else(|e| e.into_inner());
        while let Some(c) = q.pop_front() {
            let _ = c.tx.send(Err(msg.into()));
        }
    }

    fn run_calls(&self, dev: &mut GoXlr) -> Result<(), DevError> {
        loop {
            let call = self.calls.lock().unwrap_or_else(|e| e.into_inner()).pop_front();
            let Some(c) = call else { return Ok(()) };
            match (c.f)(dev) {
                Ok(v) => {
                    let _ = c.tx.send(Ok(v));
                }
                Err(e) => {
                    let _ = c.tx.send(Err(e.to_string()));
                    return Err(e);
                }
            }
        }
    }

    /// Execute f dans la boucle USB et attend le resultat (commande brute).
    pub fn call(&self, f: CallFn, timeout: Duration) -> ApiResult {
        if !self.lock().connected {
            return Err(ApiError::device("GoXLR non connectée"));
        }
        let (tx, rx) = mpsc::channel();
        self.calls.lock().unwrap_or_else(|e| e.into_inner()).push_back(Call { f, tx });
        self.wake.set();
        match rx.recv_timeout(timeout) {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(e)) => Err(ApiError::device(e)),
            Err(_) => Err(ApiError::device("La GoXLR ne répond pas")),
        }
    }

    fn want_level(&self, tick: u64) -> bool {
        let g = self.lock();
        if g.fast_level {
            return true;
        }
        matches!(g.level_until, Some(t) if Instant::now() < t) && tick % 2 == 0
    }

    pub fn poll_once(&self, dev: &mut GoXlr, with_level: bool) -> Result<(), DevError> {
        let (mask, vols, enc) = dev.get_button_states()?;
        let lvl = if with_level { Some(level_db(dev.mic_level()?)) } else { None };
        let now = Instant::now();
        let mut g = self.lock();
        if let Some(l) = lvl {
            g.live.mic_db = Some(l);
            g.live.level_t = Some(now);
        } else if matches!(g.live.level_t, Some(t) if now.duration_since(t) > Duration::from_millis(1500)) {
            g.live.mic_db = None;
            g.live.level_t = None;
        }
        g.handle_inputs(mask, vols, now);
        g.live.encoders = enc;
        Ok(())
    }

    fn iteration(&self, dev: &mut GoXlr, tick: u64) -> Result<(), DevError> {
        {
            let mut g = self.lock();
            if matches!(g.listen_until, Some(t) if Instant::now() > t) {
                g.listen_until = None;
                g.mark(&[Key::Volume(ch("MicMonitor")), Key::Route(inp("Microphone"))], false);
            }
            if matches!(g.sound_test_until, Some(t) if Instant::now() > t) {
                g.sound_test_until = None;
                g.mark(&[Key::Route(inp("Samples"))], false);
            }
        }
        self.run_calls(dev)?;
        self.flush(dev)?;
        let lvl = self.want_level(tick);
        self.poll_once(dev, lvl)?;
        self.stream_tick(dev);
        self.flush(dev)?;
        let mut g = self.lock();
        if g.fail_streak > 0 && matches!(g.conn_mono, Some(t) if t.elapsed() > Duration::from_secs(30)) {
            g.fail_streak = 0;
        }
        Ok(())
    }

    /// Compteurs de transport USB du pilote, une fois par \`stats_interval\` : une requete au pilote, rien vers la GoXLR.
    fn stream_tick(&self, dev: &mut GoXlr) {
        {
            let mut iv = *self.stats_interval.lock().unwrap_or_else(|e| e.into_inner());
            if !self.drops.lock().unwrap_or_else(|e| e.into_inner()).streaming {
                iv *= 5; // aucun son ne passe : pas de coupure possible, un coup d'oeil toutes les 5 s suffit
            }
            let mut at = self.stats_at.lock().unwrap_or_else(|e| e.into_inner());
            if matches!(*at, Some(t) if t.elapsed() < iv) {
                return;
            }
            *at = Some(Instant::now());
        }
        let s = dev.stream_stats();
        self.drops.lock().unwrap_or_else(|e| e.into_inner()).update(s, now_epoch());
    }

    /// Appel par le seul fil de diagnostic : ni disque ni serialisation sous le verrou USB.
    pub fn save_drops(&self) {
        let pending = self.drops.lock().unwrap_or_else(|e| e.into_inner()).pending_history(now_epoch());
        if let Some(pending) = pending {
            match pending.write() {
                Ok(()) => self.drops.lock().unwrap_or_else(|e| e.into_inner()).history_saved(&pending),
                Err(e) => logf!("Historique des coupures non enregistré : {e}"),
            }
        }
    }

    /// Fil « drops » : pendant que le son passe, note chaque seconde les programmes que Windows vient de lancer,
    /// pour dire ce qui tournait au moment d'une coupure.
    pub fn drops_watch_loop(&self) {
        let mut prev: Option<HashMap<u32, String>> = None;
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let streaming = self.drops.lock().unwrap_or_else(|e| e.into_inner()).streaming;
            let mut names = vec![];
            if streaming {
                let cur = crate::win::process_ids();
                if let Some(p) = &prev {
                    names = cur.iter().filter(|(pid, _)| !p.contains_key(pid)).map(|(_, n)| n.clone()).collect();
                }
                prev = Some(cur);
            } else {
                prev = None;
            }
            let lines = {
                let mut d = self.drops.lock().unwrap_or_else(|e| e.into_inner());
                d.note_launched(now_epoch(), names);
                d.take_log_lines()
            };
            for line in lines {
                log(&line);
            }
            self.save_drops();
        }
    }

    pub fn device_loop(&self, stop: Option<Arc<AtomicBool>>) {
        let Some(mut dev) = self.dev.lock().unwrap_or_else(|e| e.into_inner()).take() else { return };
        self.loop_running.store(true, Ordering::SeqCst);
        let mut next_try = Instant::now();
        let mut tick: u64 = 0;
        while !stop.as_ref().map(|s| s.load(Ordering::SeqCst)).unwrap_or(false) {
            self.beat();
            let force = std::mem::take(&mut self.lock().force_reconnect);
            if force {
                if self.lock().connected {
                    self.lost(&mut dev, "reconnexion demandée");
                }
                next_try = Instant::now();
            }
            if !self.lock().connected {
                self.fail_calls("GoXLR non connectée");
                if Instant::now() >= next_try {
                    self.dev_info.lock().unwrap().stage = Some("liste USB de Windows".into());
                    let seen = self.usb_scan();
                    self.dev_info.lock().unwrap().stage = None;
                    self.lock().note_usb(seen);
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.connect(&mut dev, true)))
                        .unwrap_or_else(|p| Err(DevError::Device(panic_text(&p))));
                    match r {
                        Ok(()) => {
                            let mut g = self.lock();
                            if !matches!(g.usb.as_deref(), None | Some("mini")) {
                                g.note_usb(Some("mini".into())); // branchee entre le coup d'oeil a l'USB et la connexion
                            }
                        }
                        Err(e) => {
                            let absent = e.is_absent();
                            let msg = e.to_string();
                            self.lost(&mut dev, &msg);
                            let delay = {
                                let mut g = self.lock();
                                let changed = g.present != Some(!absent);
                                g.present = Some(!absent);
                                g.last_error = Some(msg.clone());
                                if changed {
                                    g.version += 1;
                                }
                                if absent {
                                    drop(g);
                                    log("GoXLR absente (éteinte ou débranchée) : nouvel essai toutes les 2 s");
                                    *self.absent_retry.lock().unwrap()
                                } else {
                                    g.fail_streak += 1;
                                    let d = Duration::from_secs_f64(30f64.min(2f64.powi(g.fail_streak.min(5) as i32)));
                                    if g.errors.len() >= 15 {
                                        g.errors.pop_front();
                                    }
                                    g.errors.push_back((now_epoch(), format!("connexion : {msg}")));
                                    drop(g);
                                    logf!("Connexion impossible : {msg}");
                                    d
                                }
                            };
                            next_try = Instant::now() + delay;
                        }
                    }
                }
                self.wake.wait(Duration::from_millis(250));
                continue;
            }
            // toute erreur, meme inattendue (panique comprise) : on ferme et on repart proprement
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.iteration(&mut dev, tick)))
                .unwrap_or_else(|p| Err(DevError::Device(panic_text(&p))));
            if let Err(e) = r {
                self.lost(&mut dev, &e.to_string());
                let fs = {
                    let mut g = self.lock();
                    g.fail_streak += 1;
                    g.fail_streak
                };
                next_try = Instant::now() + Duration::from_secs_f64(30f64.min(0.5 * 2f64.powi(fs.min(6) as i32)));
            }
            tick += 1;
            let pi = *self.poll_interval.lock().unwrap();
            self.wake.wait(pi);
        }
        dev.close();
        *self.dev.lock().unwrap_or_else(|e| e.into_inner()) = Some(dev);
        self.loop_running.store(false, Ordering::SeqCst);
    }

    /// Actions de l'API : seules les mises a jour, l'egaliseur et les analyses sortent du verrou.
    pub fn act(self: &Arc<Self>, action: &str, p: &Value) -> ApiResult {
        let empty = json!({});
        let p = if p.is_object() { p } else { &empty };
        match action {
            "update-check" => {
                self.updater.check(true).map_err(ApiError::bad)?;
                return Ok(json!({ "ok": true, "update": self.update_snapshot() }));
            }
            "update-install" => {
                self.updater.start_install(self.clone()).map_err(ApiError::bad)?;
                return Ok(json!({ "ok": true }));
            }
            "update-cancel" => {
                self.updater.cancel_install();
                return Ok(json!({ "ok": true }));
            }
            "update-ack" => {
                self.updater.ack_installed();
                return Ok(json!({ "ok": true }));
            }
            "update-remind" => {
                let v = p.get("version").filter(|v| v.as_str().and_then(rustudio_core::semver::parse).is_some()).cloned();
                let mut g = self.lock();
                g.state.prefs.update_reminded = v;
                g.version += 1;
                g.save_dirty = true;
                return Ok(json!({ "ok": true }));
            }
            "raw" => {
                let cid = u32::from_str_radix(need_str(p, "command_id")?.trim_start_matches("0x"), 16)
                    .map_err(|_| ApiError::bad("command_id invalide"))?;
                let hex = p.get("body_hex").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let body = (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16)).collect::<Result<Vec<u8>, _>>()
                    .map_err(|_| ApiError::bad("body_hex invalide"))?;
                let r = self.call(
                    Box::new(move |d| d.command(cid, &body).map(|b| json!(b.iter().map(|x| format!("{x:02x}")).collect::<String>()))),
                    Duration::from_secs(5),
                )?;
                return Ok(json!({ "ok": true, "response_hex": r }));
            }
            "hp-eq-setup" | "hp-eq-remove" => return self.hp_start(action == "hp-eq-setup"),
            "reconnect" => {
                self.lock().force_reconnect = true;
                self.wake.set();
                log("Reconnexion de la GoXLR demandée");
                return Ok(json!({ "ok": true }));
            }
            "restart-engine" => {
                if !self.cfg.supervised {
                    return Err(ApiError::bad("Le moteur n'est pas lancé par son service : redémarre le service « Rustudio »."));
                }
                log("Redémarrage du moteur demandé depuis l'interface");
                self.save_if_dirty();
                std::thread::spawn(|| {
                    std::thread::sleep(Duration::from_millis(400));
                    std::process::exit(crate::config::EXIT_RESTART);
                });
                return Ok(json!({ "ok": true }));
            }
            "detect-mic" | "auto-gain" | "voice-setup" => {
                let started = crate::jobs::start_job(self, action, p.clone())?;
                let mut g = self.lock();
                g.version += 1;
                return Ok(json!({ "ok": true, "job": started }));
            }
            _ => {}
        }
        let r = {
            let mut g = self.lock();
            let r = g.act_locked(action, p)?;
            g.version += 1;
            g.save_dirty = true;
            r
        };
        if action.starts_with("hp-eq") {
            self.hp_apply();
        }
        Ok(r)
    }

    pub fn save_if_dirty(&self) {
        if !self.persist {
            return;
        }
        let data = {
            let mut g = self.lock();
            if !g.save_dirty {
                return;
            }
            g.save_dirty = false;
            serde_json::to_string_pretty(&g.state).unwrap_or_default()
        };
        let sp = self.cfg.state_path();
        let saved = (|| -> std::io::Result<()> {
            std::fs::create_dir_all(&self.cfg.data_dir)?;
            {
                let mut lb = self.last_backup.lock().unwrap();
                if sp.exists() && now_epoch() - *lb > 600.0 && std::fs::copy(&sp, format!("{}.bak", sp.display())).is_ok() {
                    *lb = now_epoch();
                }
            }
            let tmp = format!("{}.tmp", sp.display());
            std::fs::write(&tmp, data)?;
            std::fs::rename(&tmp, &sp)
        })();
        if let Err(e) = saved {
            // Le fil de sauvegarde reessaiera, meme si aucun autre reglage ne change entre-temps.
            self.lock().save_dirty = true;
            logf!("Sauvegarde de l'état impossible : {e}");
        }
    }

    pub fn save_loop(self: &Arc<Self>) {
        loop {
            std::thread::sleep(Duration::from_secs(1));
            self.save_if_dirty();
        }
    }
}

