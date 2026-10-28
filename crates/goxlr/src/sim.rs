//! Fausse GoXLR Mini : memes fonctions que le pilote Thesycon, memes codes d'erreur, memes pieges.
//!  - EnumerateDevices() avec un handle ouvert invalide TOUS les handles ;
//!  - debranchement : TSTATUS_INVALID_HANDLE sur le handle en cours ;
//!  - pilote bloque : la prochaine requete dort N secondes ;
//!  - longueurs de corps fausses (routage, carte couleurs...) notees dans errors.
use crate::Tusb;
use rustudio_core::consts::{button_bit, CHANNELS, OUTPUTS};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const TSTATUS_INVALID_HANDLE: u32 = 3992977480;
pub const TSTATUS_NO_DEVICE: u32 = 3992977484;
pub const TSTATUS_STALL: u32 = 4009754628;

pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn random(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

const INPUT_IDS: [(u32, &str); 16] = [
    (0x02, "Microphone"), (0x03, "Microphone"), (0x04, "LineIn"), (0x05, "LineIn"), (0x06, "Console"),
    (0x07, "Console"), (0x08, "System"), (0x09, "System"), (0x0A, "Game"), (0x0B, "Game"), (0x0C, "Chat"),
    (0x0D, "Chat"), (0x0E, "Music"), (0x0F, "Music"), (0x10, "Samples"), (0x11, "Samples"),
];

/// Ce que le firmware de la Mini a recu et applique.
pub struct FakeDevice {
    pub firmware: [u32; 4],
    pub serial: String,
    pub volumes: [u8; 11],
    pub muted: [bool; 11],
    pub faders: [u8; 4],
    pub routing: HashMap<u32, Vec<u8>>,
    pub colour_map: Vec<u8>,
    pub button_states: Vec<u8>,
    pub mic_params: HashMap<u32, [u8; 4]>,
    pub effects: BTreeMap<u32, i32>,
    pub fader_display: HashMap<u32, Vec<u8>>,
    pub animation: Option<Vec<u8>>,
    pub phantom: bool,
    pub monitored_mix: Option<u8>,
    pub channel_mixes: Option<Vec<u8>>,
    pub mic_type: &'static str,
    pub gains: HashMap<&'static str, i32>,
    pub fader_pos: [u8; 4],
    pub held: HashSet<String>,
    pub timed: HashMap<String, Instant>,
    pub mic_input: Option<String>,
    pub talking: bool,
    pub voice_db: f64,
    pub noise_db: f64,
    pub errors: Vec<String>,
    pub commands: u64,
    pub index_resets: u64,
    pub max_index: u32,
    pub counter: u32,
    pub index_mismatches: u64,
    pub rng: Rng,
}

impl FakeDevice {
    pub fn new(firmware: [u32; 4]) -> FakeDevice {
        FakeDevice {
            firmware,
            serial: "SIM-GOXLR-MINI".into(),
            volumes: [0; 11],
            muted: [false; 11],
            faders: [0, 7, 5, 3],
            routing: HashMap::new(),
            colour_map: vec![0; 520],
            button_states: vec![0; 24],
            mic_params: HashMap::new(),
            effects: BTreeMap::new(),
            fader_display: HashMap::new(),
            animation: None,
            phantom: false,
            monitored_mix: None,
            channel_mixes: None,
            mic_type: "dynamic",
            gains: [("dynamic", 0), ("condenser", 0), ("jack", 0)].into_iter().collect(),
            fader_pos: [128; 4],
            held: HashSet::new(),
            timed: HashMap::new(),
            mic_input: Some("dynamic".into()),
            talking: false,
            voice_db: -30.0,
            noise_db: -66.0,
            errors: vec![],
            commands: 0,
            index_resets: 0,
            max_index: 0,
            counter: 0,
            index_mismatches: 0,
            rng: Rng::new(42),
        }
    }
    pub fn mix2(&self) -> bool {
        (self.firmware[0], self.firmware[1]) >= (1, 3)
    }
    fn mask(&mut self) -> u32 {
        let now = Instant::now();
        let mut m = 0u32;
        for b in &self.held {
            if let Some(bit) = button_bit(b) {
                m |= 1 << bit;
            }
        }
        self.timed.retain(|_, until| now < *until);
        for b in self.timed.keys() {
            if let Some(bit) = button_bit(b) {
                m |= 1 << bit;
            }
        }
        m
    }
    fn sensitivity(t: &str) -> f64 {
        match t {
            "condenser" => 18.0,
            "jack" => 8.0,
            _ => 0.0,
        }
    }
    /// Niveau selon le micro branche, l'entree choisie et le gain. La voix varie d'une syllabe a l'autre.
    pub fn mic_db(&mut self) -> Option<f64> {
        let t = self.mic_input.clone()?;
        let typ = self.mic_type;
        let mut xlr_ok = (t == "dynamic" || t == "condenser") && (typ == "dynamic" || typ == "condenser");
        if t == "condenser" && !(typ == "condenser" && self.phantom) {
            xlr_ok = false;
        }
        if !(xlr_ok || (t == "jack" && typ == "jack")) {
            return None;
        }
        let gain = (*self.gains.get(typ).unwrap_or(&0) - 40) as f64 + Self::sensitivity(&t);
        let db = if self.talking {
            let r = self.rng.random();
            self.voice_db + gain - r * r * 24.0
        } else {
            self.noise_db + gain + self.rng.random() * 1.5
        };
        Some(db.clamp(-72.0, 0.0))
    }
    fn mic_raw(&mut self) -> u16 {
        match self.mic_db() {
            None => 0,
            Some(db) => (10f64.powf((db + 72.2) / 20.0)).round().clamp(0.0, 65535.0) as u16,
        }
    }

    fn execute(&mut self, cid: u32, body: &[u8], idx: u32) -> Vec<u8> {
        self.commands += 1;
        if cid == 0 {
            self.index_resets += 1;
            self.counter = 0;
            return vec![];
        }
        if idx > 0xFFFF {
            self.errors.push(format!("index {idx} hors 16 bits"));
        }
        self.counter = (self.counter + 1) & 0xFFFF;
        if idx != self.counter {
            self.index_mismatches += 1;
        }
        self.max_index = self.max_index.max(idx);
        let (cmd, sub) = (cid >> 12, cid & 0xFFF);
        let b0 = body.first().copied().unwrap_or(0);
        match cmd {
            0x80F => {
                if sub == 1 {
                    let mut s = self.serial.as_bytes().to_vec();
                    s.resize(24, 0);
                    let mut d = b"2021-03-09".to_vec();
                    d.resize(16, 0);
                    s.extend(d);
                    return s;
                }
                let p = (self.firmware[0] << 12) | (self.firmware[1] << 8) | self.firmware[2];
                let mut out = vec![];
                for v in [p, self.firmware[3], 0, 1] {
                    out.extend_from_slice(&v.to_le_bytes());
                }
                return out;
            }
            0x806 => {
                if sub < 11 {
                    self.volumes[sub as usize] = b0;
                }
            }
            0x809 => {
                if sub < 11 {
                    self.muted[sub as usize] = b0 != 0;
                }
            }
            0x805 => {
                if sub < 4 {
                    self.faders[sub as usize] = b0;
                }
            }
            0x804 => {
                let want = if self.mix2() { 26 } else { 22 };
                if body.len() != want {
                    self.errors.push(format!("routage {} octets au lieu de {want}", body.len()));
                }
                self.routing.insert(sub, body.to_vec());
            }
            0x803 => {
                if body.len() != 520 {
                    self.errors.push(format!("carte couleurs {} octets", body.len()));
                }
                self.colour_map = body.to_vec();
            }
            0x808 => {
                if body.len() != 24 {
                    self.errors.push(format!("etat des boutons {} octets", body.len()));
                }
                self.button_states = body.to_vec();
            }
            0x800 => {
                let mut out = self.mask().to_le_bytes().to_vec();
                out.extend_from_slice(&[0; 4]);
                out.extend_from_slice(&self.fader_pos);
                out.extend_from_slice(&[0; 4]);
                return out;
            }
            0x80C => return self.mic_raw().to_le_bytes().to_vec(),
            0x80B => {
                let mut off = 0;
                while off + 8 <= body.len() {
                    let key = u32::from_le_bytes([body[off], body[off + 1], body[off + 2], body[off + 3]]);
                    let val = [body[off + 4], body[off + 5], body[off + 6], body[off + 7]];
                    self.mic_params.insert(key, val);
                    match key {
                        0 => self.phantom = val[0] == 1,
                        1..=3 => {
                            self.mic_type = ["dynamic", "condenser", "jack"][(key - 1) as usize];
                            self.gains.insert(self.mic_type, u16::from_le_bytes([val[2], val[3]]) as i32);
                        }
                        _ => {}
                    }
                    off += 8;
                }
            }
            0x801 => {
                let mut off = 0;
                while off + 8 <= body.len() {
                    let key = u32::from_le_bytes([body[off], body[off + 1], body[off + 2], body[off + 3]]);
                    let val = i32::from_le_bytes([body[off + 4], body[off + 5], body[off + 6], body[off + 7]]);
                    self.effects.insert(key, val);
                    off += 8;
                }
            }
            0x814 => {
                self.fader_display.insert(sub, body.to_vec());
            }
            0x816 => self.animation = Some(body.to_vec()),
            0x818 => self.monitored_mix = Some(b0),
            0x817 => {
                let want = if self.mix2() { 10 } else { 8 };
                if body.len() != want {
                    self.errors.push(format!("sous-mixages {} octets au lieu de {want}", body.len()));
                }
                self.channel_mixes = Some(body.to_vec());
            }
            _ => self.errors.push(format!("commande inconnue {cid:#x}")),
        }
        vec![]
    }

    pub fn route_on(&self, inp: &str, out: &str) -> bool {
        let left = INPUT_IDS.iter().find(|(_, n)| *n == inp).map(|(i, _)| *i).unwrap_or(0);
        let pos = OUTPUTS.iter().find(|(o, _, _)| *o == out).map(|(_, l, _)| *l).unwrap_or(0);
        self.routing.get(&left).map(|d| d.get(pos) == Some(&0x20)).unwrap_or(false)
    }
    pub fn colour(&self, index: usize) -> String {
        let b = &self.colour_map[index * 4..index * 4 + 4];
        format!("{:02X}{:02X}{:02X}", b[2], b[1], b[0])
    }
    pub fn volume(&self, ch: &str) -> u8 {
        self.volumes[CHANNELS.iter().position(|c| *c == ch).unwrap()]
    }
    pub fn is_muted(&self, ch: &str) -> bool {
        self.muted[CHANNELS.iter().position(|c| *c == ch).unwrap()]
    }
}

pub struct SimInner {
    pub dev: FakeDevice,
    pub plugged: bool,
    pub handles: HashMap<u32, bool>,
    pub next_h: u32,
    pub pending: VecDeque<(Instant, Vec<u8>)>,
    pub latency: Duration,
    pub hang: Duration,
    pub fail_next: Vec<u32>,
    pub stall_rate: f64,
    pub enumerate_calls: u64,
    pub enumerate_violations: u64,
    pub unplugged_usb: String,
    pub apps: HashSet<String>,
    rng: Rng,
}

/// Fausse GoXLR partagee entre le fil USB (via SimTusb) et le banc de test.
#[derive(Clone)]
pub struct Sim(pub Arc<Mutex<SimInner>>);

impl Sim {
    pub fn new(firmware: [u32; 4]) -> Sim {
        Sim(Arc::new(Mutex::new(SimInner {
            dev: FakeDevice::new(firmware),
            plugged: true,
            handles: HashMap::new(),
            next_h: 100,
            pending: VecDeque::new(),
            latency: Duration::from_millis(3),
            hang: Duration::ZERO,
            fail_next: vec![],
            stall_rate: 0.0,
            enumerate_calls: 0,
            enumerate_violations: 0,
            unplugged_usb: "none".into(),
            apps: HashSet::new(),
            rng: Rng::new(7),
        })))
    }
    pub fn lock(&self) -> std::sync::MutexGuard<'_, SimInner> {
        self.0.lock().unwrap()
    }
    pub fn tusb(&self) -> SimTusb {
        SimTusb(self.clone())
    }
    /// ce que « Windows » voit sur l'USB
    pub fn usb_scan(&self) -> String {
        let s = self.lock();
        if s.plugged { "mini".into() } else { s.unplugged_usb.clone() }
    }
    pub fn unplug(&self) {
        let mut s = self.lock();
        s.plugged = false;
        for v in s.handles.values_mut() {
            *v = false;
        }
        s.pending.clear();
    }
    pub fn plug(&self) {
        self.lock().plugged = true;
    }
    pub fn hang(&self, seconds: f64) {
        self.lock().hang = Duration::from_secs_f64(seconds.max(0.0));
    }
    pub fn press(&self, button: &str, ms: f64) {
        self.lock().dev.timed.insert(button.into(), Instant::now() + Duration::from_secs_f64(ms.max(60.0) / 1000.0));
    }
    pub fn button_down(&self, button: &str) {
        self.lock().dev.held.insert(button.into());
    }
    pub fn button_up(&self, button: &str) {
        self.lock().dev.held.remove(button);
    }
    pub fn move_fader(&self, index: usize, value: i32) {
        let mut s = self.lock();
        let v = value.clamp(0, 255) as u8;
        s.dev.fader_pos[index] = v;
        let ch = s.dev.faders[index] as usize;
        if ch < 11 {
            s.dev.volumes[ch] = v; // comme le vrai : le fader regle le volume
        }
    }
    pub fn describe(&self) -> serde_json::Value {
        let s = self.lock();
        let d = &s.dev;
        let mut apps: Vec<String> = s.apps.iter().cloned().collect();
        apps.sort();
        serde_json::json!({
            "plugged": s.plugged, "firmware": d.firmware.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("."),
            "commands": d.commands, "index_resets": d.index_resets, "max_index": d.max_index,
            "enumerate_violations": s.enumerate_violations, "errors": d.errors.iter().rev().take(20).rev().collect::<Vec<_>>(),
            "volumes": CHANNELS.iter().zip(d.volumes.iter()).map(|(c, v)| (c.to_string(), serde_json::json!(v))).collect::<serde_json::Map<_, _>>(),
            "muted": CHANNELS.iter().zip(d.muted.iter()).map(|(c, v)| (c.to_string(), serde_json::json!(v))).collect::<serde_json::Map<_, _>>(),
            "faders": d.faders.iter().map(|i| CHANNELS.get(*i as usize).copied().unwrap_or("?")).collect::<Vec<_>>(),
            "button_states": d.button_states, "fader_pos": d.fader_pos, "mic_type": d.mic_type,
            "gains": d.gains, "phantom": d.phantom, "mic_input": d.mic_input, "talking": d.talking,
            "apps": apps,
        })
    }
}

pub struct SimTusb(pub Sim);

impl SimTusb {
    fn maybe_hang(&self) {
        let h = std::mem::take(&mut self.0.lock().hang);
        if !h.is_zero() {
            std::thread::sleep(h);
        }
    }
}

fn bad_handle(s: &mut SimInner, h: u32) -> u32 {
    if !s.handles.get(&h).copied().unwrap_or(false) {
        return TSTATUS_INVALID_HANDLE;
    }
    if !s.fail_next.is_empty() {
        return s.fail_next.remove(0);
    }
    0
}

impl Tusb for SimTusb {
    fn enumerate(&mut self) -> u32 {
        let mut s = self.0.lock();
        s.enumerate_calls += 1;
        if s.handles.values().any(|v| *v) {
            s.enumerate_violations += 1; // le vrai pilote tue alors tous les handles
            for v in s.handles.values_mut() {
                *v = false;
            }
        }
        0
    }
    fn device_count(&mut self) -> u32 {
        if self.0.lock().plugged { 1 } else { 0 }
    }
    fn open_by_index(&mut self, idx: u32) -> Result<u32, u32> {
        let mut s = self.0.lock();
        if !s.plugged || idx != 0 {
            return Err(TSTATUS_NO_DEVICE);
        }
        let h = s.next_h;
        s.next_h += 1;
        s.handles.insert(h, true);
        Ok(h)
    }
    fn instance_id(&mut self, _h: u32) -> Option<String> {
        Some("USB\\VID_1220&PID_8FE4\\SIM".into())
    }
    fn vendor_out(&mut self, h: u32, request: u32, data: &[u8]) -> u32 {
        self.maybe_hang();
        let mut s = self.0.lock();
        let err = bad_handle(&mut s, h);
        if err != 0 {
            return err;
        }
        if request == 1 {
            s.pending.clear();
            s.dev.counter = 0;
            return 0;
        }
        if request == 2 && data.len() >= 16 {
            let cid = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
            let blen = u16::from_le_bytes([data[4], data[5]]) as usize;
            let idx = u16::from_le_bytes([data[6], data[7]]) as u32;
            let body = &data[16..(16 + blen).min(data.len())];
            let resp = s.dev.execute(cid, body, idx);
            let mut frame = cid.to_le_bytes().to_vec();
            frame.extend_from_slice(&(resp.len() as u16).to_le_bytes());
            frame.extend_from_slice(&(s.dev.counter as u16).to_le_bytes());
            frame.extend_from_slice(&[0; 8]);
            frame.extend(resp);
            let at = Instant::now() + s.latency;
            s.pending.push_back((at, frame));
        }
        0
    }
    fn vendor_in(&mut self, h: u32, request: u32, len: usize) -> Result<Vec<u8>, u32> {
        self.maybe_hang();
        let mut s = self.0.lock();
        let err = bad_handle(&mut s, h);
        if err != 0 {
            return Err(err);
        }
        let out = match request {
            0 => vec![0; 24],
            3 => {
                let ready = s.pending.front().map(|(t, _)| *t <= Instant::now()).unwrap_or(false);
                let stall = s.stall_rate > 0.0 && s.rng.random() < s.stall_rate;
                if !ready || stall {
                    return Err(TSTATUS_STALL);
                }
                s.pending.pop_front().unwrap().1
            }
            _ => vec![],
        };
        Ok(out[..out.len().min(len)].to_vec())
    }
    fn close(&mut self, h: u32) {
        self.0.lock().handles.remove(&h);
    }
    fn status_text(&self, code: u32) -> String {
        match code {
            0 => "TSTATUS_SUCCESS".into(),
            TSTATUS_INVALID_HANDLE => "TSTATUS_INVALID_HANDLE".into(),
            3992977495 => "TSTATUS_INVALID_DEVICE_STATE".into(),
            TSTATUS_NO_DEVICE => "TSTATUS_NO_DEVICES".into(),
            TSTATUS_STALL => "TSTATUS_USBD_STATUS_STALL_PID".into(),
            c => format!("TSTATUS_{c}"),
        }
    }
}
