//! Protocole GoXLR Mini (constantes reprises du projet libre goxlr-utility, licence MIT).
//!
//! Regles d'or :
//! 1. EnumerateDevices() invalide TOUS les handles ouverts : seulement handle ferme, juste avant une ouverture.
//! 2. L'index de commande est sur 16 bits : commande 0 (remise a zero) avant 65 535.
//! 3. Un seul fil parle a l'USB (celui qui possede GoXlr).
//! 4. Toute erreur pendant un echange ferme le handle ; l'appelant relance une connexion propre.
use rustudio_core::consts::*;
use rustudio_core::routing::routing_bytes;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::{Duration, Instant};

#[cfg(windows)]
pub mod dll;
pub mod sim;
pub mod stream;
#[cfg(windows)]
pub mod winusb;

/// API Thesycon du pilote (goxlr_audioapi_x64.dll), ou son equivalent simule.
pub trait Tusb: Send {
    fn enumerate(&mut self) -> u32;
    fn device_count(&mut self) -> u32;
    fn open_by_index(&mut self, idx: u32) -> Result<u32, u32>;
    fn instance_id(&mut self, h: u32) -> Option<String>;
    fn vendor_out(&mut self, h: u32, request: u32, data: &[u8]) -> u32;
    fn vendor_in(&mut self, h: u32, request: u32, len: usize) -> Result<Vec<u8>, u32>;
    fn close(&mut self, h: u32);
    fn status_text(&self, code: u32) -> String;
    /// Compteurs de transport USB du pilote (voir stream.rs) : une requete au pilote, rien vers l'appareil.
    fn stream_stats(&mut self, _h: u32) -> Option<stream::StreamStats> {
        None
    }
}

pub type Loader = Box<dyn Fn() -> Result<Box<dyn Tusb>, String> + Send>;

/// Requêtes de lecture propres au firmware Rustudio (jamais envoyées au pilote officiel) : compteurs du flux
/// USB Audio (6 mots) et de la sortie casque (19 mots), dans l'ordre de `FirmwareCounters`.
pub const REQ_FIRMWARE_AUDIO: u32 = 0x109;
pub const REQ_FIRMWARE_HEADPHONES: u32 = 0x10A;

/// Compteurs du firmware Rustudio, depuis l'ouverture du flux (audio) ou depuis le démarrage (boucle).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FirmwareCounters {
    pub streaming: bool,
    pub packets: u32,
    pub missed: u32,
    pub underruns: u32,
    pub drops: u32,
    pub repeats: u32,
    pub queue_min: u32,
    pub queue_max: u32,
    /// Tour de boucle le plus long depuis la lecture précédente, en microsecondes.
    pub max_loop_us: u32,
    pub slow_loops: u32,
}

impl FirmwareCounters {
    pub fn from_words(audio: &[u32], headphones: &[u32]) -> FirmwareCounters {
        let a = |i: usize| audio.get(i).copied().unwrap_or(0);
        let h = |i: usize| headphones.get(i).copied().unwrap_or(0);
        FirmwareCounters {
            streaming: a(0) != 0,
            packets: a(1),
            missed: a(4),
            underruns: h(3),
            drops: h(8),
            repeats: h(9),
            queue_min: h(10),
            queue_max: h(11),
            // ticks de 5,12 µs
            max_loop_us: (u64::from(h(12)) * 512 / 100) as u32,
            slow_loops: h(13),
        }
    }
}

/// Pilote officiel et firmware Rustudio dans une seule liste d'appareils : ceux du pilote d'abord.
pub struct Combined {
    official: Option<Box<dyn Tusb>>,
    rustudio: Box<dyn Tusb>,
    official_count: u32,
}

/// Bit des handles du firmware Rustudio, absents des handles du pilote officiel.
const RUSTUDIO_HANDLE: u32 = 1 << 31;

impl Combined {
    pub fn new(official: Option<Box<dyn Tusb>>, rustudio: Box<dyn Tusb>) -> Self {
        Combined { official, rustudio, official_count: 0 }
    }
    fn split(&mut self, h: u32) -> (&mut dyn Tusb, u32) {
        match self.official.as_deref_mut() {
            Some(official) if h & RUSTUDIO_HANDLE == 0 => (official, h),
            _ => (self.rustudio.as_mut(), h & !RUSTUDIO_HANDLE),
        }
    }
}

impl Tusb for Combined {
    fn enumerate(&mut self) -> u32 {
        let code = self.official.as_mut().map(|t| t.enumerate()).unwrap_or(0);
        self.rustudio.enumerate();
        code
    }
    fn device_count(&mut self) -> u32 {
        self.official_count = self.official.as_mut().map(|t| t.device_count()).unwrap_or(0);
        self.official_count + self.rustudio.device_count()
    }
    fn open_by_index(&mut self, idx: u32) -> Result<u32, u32> {
        match self.official.as_mut() {
            Some(official) if idx < self.official_count => official.open_by_index(idx),
            _ => self.rustudio.open_by_index(idx - self.official_count).map(|h| h | RUSTUDIO_HANDLE),
        }
    }
    fn instance_id(&mut self, h: u32) -> Option<String> {
        let (t, h) = self.split(h);
        t.instance_id(h)
    }
    fn vendor_out(&mut self, h: u32, request: u32, data: &[u8]) -> u32 {
        let (t, h) = self.split(h);
        t.vendor_out(h, request, data)
    }
    fn vendor_in(&mut self, h: u32, request: u32, len: usize) -> Result<Vec<u8>, u32> {
        let (t, h) = self.split(h);
        t.vendor_in(h, request, len)
    }
    fn close(&mut self, h: u32) {
        let (t, h) = self.split(h);
        t.close(h)
    }
    fn status_text(&self, code: u32) -> String {
        self.official.as_ref().map(|t| t.status_text(code)).unwrap_or_else(|| self.rustudio.status_text(code))
    }
    fn stream_stats(&mut self, h: u32) -> Option<stream::StreamStats> {
        if h & RUSTUDIO_HANDLE != 0 {
            return None;
        }
        self.official.as_mut()?.stream_stats(h)
    }
}

#[derive(Debug, Clone)]
pub enum DevError {
    Tusb { what: String, code: u32, text: String },
    Device(String),
}
impl fmt::Display for DevError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            DevError::Tusb { what, code, text } => write!(f, "{what}: {code}={text}"),
            DevError::Device(m) => write!(f, "{m}"),
        }
    }
}
impl std::error::Error for DevError {}
impl DevError {
    pub fn code(&self) -> Option<u32> {
        match self {
            DevError::Tusb { code, .. } => Some(*code),
            _ => None,
        }
    }
    pub fn is_absent(&self) -> bool {
        matches!(self, DevError::Device(m) if m.contains("Aucune GoXLR"))
    }
}
pub type Res<T> = Result<T, DevError>;

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Stats {
    pub commands: u64,
    pub stalls: u64,
    pub resyncs: u64,
    pub index_resets: u64,
    pub opens: u64,
    /// Temps de réponse de la dernière commande (envoi → réponse de la GoXLR), en millisecondes.
    pub reply_ms: f64,
    /// Moyenne glissante de ce temps de réponse (les dix dernières commandes environ).
    pub reply_avg_ms: f64,
}

impl Stats {
    /// Note un aller-retour réussi ; simple horodatage, aucun échange USB en plus.
    pub fn note_reply(&mut self, ms: f64) {
        self.reply_avg_ms = if self.reply_avg_ms > 0.0 { self.reply_avg_ms * 0.9 + ms * 0.1 } else { ms };
        self.reply_ms = ms;
    }
}

/// Ce que les autres fils peuvent lire sans toucher a l'USB (diagnostic).
#[derive(Debug, Clone, Default)]
pub struct DevInfo {
    pub serial: Option<String>,
    pub firmware: Option<String>,
    pub fw: [u32; 4],
    /// Mini sur le firmware Rustudio (USB 1209:0002), pilotee par WinUSB.
    pub rustudio: bool,
    pub stats: Stats,
    pub busy_since: Option<Instant>,
    pub stage: Option<String>,
    pub count: u32,
}
impl DevInfo {
    pub fn mix2(&self) -> bool {
        (self.fw[0], self.fw[1]) >= (1, 3)
    }
    pub fn submixes(&self) -> bool {
        self.fw >= [1, 2, 0, 46]
    }
    /// Golden : firmware de secours (1.0.0.x) que la Mini lance quand son firmware principal ne démarre plus.
    pub fn golden(&self) -> bool {
        self.fw[..3] == [1, 0, 0]
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub first_read_wait: Duration,
    pub retry_wait: Duration,
    pub response_timeout: Duration,
}
impl Default for Timing {
    fn default() -> Self {
        Timing {
            first_read_wait: Duration::from_millis(30), // lire trop tot fait caler le tube de controle (STALL)
            retry_wait: Duration::from_millis(10),
            response_timeout: Duration::from_secs(2),
        }
    }
}

pub const INDEX_MAX: u32 = 0xFFFF;

pub struct GoXlr {
    loader: Loader,
    tusb: Option<Box<dyn Tusb>>,
    h: Option<u32>,
    pub count: u32,
    pub info: Arc<Mutex<DevInfo>>,
    pub timing: Timing,
    pub log: Box<dyn Fn(&str) + Send>,
}

impl GoXlr {
    pub fn new(loader: Loader) -> Self {
        GoXlr {
            loader,
            tusb: None,
            h: None,
            count: 0,
            info: Arc::new(Mutex::new(DevInfo::default())),
            timing: Timing::default(),
            log: Box::new(|_| {}),
        }
    }

    pub fn is_open(&self) -> bool {
        self.h.is_some()
    }
    fn stage(&self, s: Option<&str>) {
        self.info.lock().unwrap().stage = s.map(String::from);
    }
    pub fn fw(&self) -> [u32; 4] {
        self.info.lock().unwrap().fw
    }
    pub fn mix2(&self) -> bool {
        self.info.lock().unwrap().mix2()
    }
    pub fn submixes(&self) -> bool {
        self.info.lock().unwrap().submixes()
    }
    fn err(&self, what: &str, code: u32) -> DevError {
        let text = self.tusb.as_ref().map(|t| t.status_text(code)).unwrap_or_else(|| code.to_string());
        DevError::Tusb { what: what.into(), code, text }
    }

    pub fn open(&mut self) -> Res<()> {
        if self.h.is_some() {
            return Ok(());
        }
        if self.tusb.is_none() {
            // charge ici, dans la boucle USB : l'interface repond deja si le pilote bloque
            self.stage(Some("chargement du pilote USB"));
            match (self.loader)() {
                Ok(t) => self.tusb = Some(t),
                Err(e) => {
                    self.stage(None);
                    return Err(DevError::Device(format!("Pilote GoXLR introuvable : {e}")));
                }
            }
        }
        self.stage(Some("recherche de la GoXLR"));
        let t = self.tusb.as_mut().unwrap();
        let code = t.enumerate(); // handle ferme : sans risque
        if code != 0 {
            self.stage(None);
            return Err(self.err("EnumerateDevices", code));
        }
        let n = self.tusb.as_mut().unwrap().device_count();
        for idx in 0..n {
            let t = self.tusb.as_mut().unwrap();
            let Ok(h) = t.open_by_index(idx) else { continue };
            let id = t.instance_id(h).unwrap_or_default();
            let rustudio = id.contains("VID_1209&PID_0002");
            if id.contains("VID_1220") || rustudio {
                self.h = Some(h);
                {
                    let mut info = self.info.lock().unwrap();
                    info.stats.opens += 1;
                    info.rustudio = rustudio;
                }
                self.stage(Some("ouverture de la GoXLR"));
                let r = self.activate().and_then(|_| self.read_info());
                self.stage(None);
                if let Err(e) = r {
                    self.close();
                    return Err(e);
                }
                return Ok(());
            }
            t.close(h);
        }
        self.stage(None);
        Err(DevError::Device("Aucune GoXLR trouvée par le pilote".into()))
    }

    /// Coupures du son comptees par le pilote (diagnostic des crepitements) ; ne parle pas a l'appareil.
    pub fn stream_stats(&mut self) -> Option<stream::StreamStats> {
        let h = self.h?;
        self.tusb.as_mut()?.stream_stats(h)
    }

    pub fn close(&mut self) {
        if let (Some(h), Some(t)) = (self.h.take(), self.tusb.as_mut()) {
            t.close(h);
        }
    }

    fn req_out(&mut self, request: u32, data: &[u8]) -> Res<()> {
        let h = self.h.ok_or_else(|| DevError::Device("GoXLR non connectée".into()))?;
        self.info.lock().unwrap().busy_since = Some(Instant::now());
        let code = self.tusb.as_mut().unwrap().vendor_out(h, request, data);
        self.info.lock().unwrap().busy_since = None;
        if code != 0 {
            return Err(self.err(&format!("VendorOut({request})"), code));
        }
        Ok(())
    }

    fn req_in(&mut self, request: u32, len: usize) -> Res<Vec<u8>> {
        let h = self.h.ok_or_else(|| DevError::Device("GoXLR non connectée".into()))?;
        self.info.lock().unwrap().busy_since = Some(Instant::now());
        let r = self.tusb.as_mut().unwrap().vendor_in(h, request, len);
        self.info.lock().unwrap().busy_since = None;
        r.map_err(|code| self.err(&format!("VendorIn({request})"), code))
    }

    /// Le firmware Rustudio traite la commande des sa reception : sa reponse se lit sans les 30 ms d'attente
    /// qu'exige le pilote officiel.
    fn read_timing(&self) -> Timing {
        if self.info.lock().unwrap().rustudio {
            Timing { first_read_wait: Duration::ZERO, retry_wait: Duration::from_millis(1), ..self.timing }
        } else {
            self.timing
        }
    }

    fn activate(&mut self) -> Res<()> {
        self.req_in(0, 24)?;
        self.req_out(1, &[])?;
        sleep(Duration::from_millis(200));
        match self.req_in(3, 1040) {
            // vidange ; peut caler si rien n'est en attente
            Err(e) if e.code() != Some(STATUS_STALL) => return Err(e),
            _ => {}
        }
        self.set_count(0);
        Ok(())
    }

    fn set_count(&mut self, c: u32) {
        self.count = c;
        self.info.lock().unwrap().count = c;
    }

    pub fn command(&mut self, command_id: u32, body: &[u8]) -> Res<Vec<u8>> {
        self.command_ex(command_id, body, true)
    }

    fn command_ex(&mut self, command_id: u32, body: &[u8], retry: bool) -> Res<Vec<u8>> {
        if self.h.is_none() {
            return Err(DevError::Device("GoXLR non connectée".into()));
        }
        if command_id == 0 {
            self.set_count(0);
        } else {
            if self.count >= INDEX_MAX {
                self.info.lock().unwrap().stats.index_resets += 1; // comme goxlr-utility : remise a zero avant debordement
                self.command_ex(0, &[], false)?;
            }
            self.set_count(self.count + 1);
        }
        let idx = self.count as u16;
        let mut frame = Vec::with_capacity(16 + body.len());
        frame.extend_from_slice(&command_id.to_le_bytes());
        frame.extend_from_slice(&(body.len() as u16).to_le_bytes());
        frame.extend_from_slice(&idx.to_le_bytes());
        frame.extend_from_slice(&[0u8; 8]);
        frame.extend_from_slice(body);
        let sent = Instant::now();
        self.req_out(2, &frame)?;
        self.info.lock().unwrap().stats.commands += 1;
        let t0 = Instant::now();
        let timing = self.read_timing();
        let mut wait = timing.first_read_wait;
        while t0.elapsed() < timing.response_timeout {
            if !wait.is_zero() {
                sleep(wait);
            }
            wait = timing.retry_wait;
            let resp = match self.req_in(3, 1040) {
                Ok(r) => r,
                Err(e) if e.code() == Some(STATUS_STALL) => {
                    self.info.lock().unwrap().stats.stalls += 1;
                    if wait.is_zero() {
                        std::thread::yield_now();
                    }
                    continue;
                }
                Err(e) => return Err(e),
            };
            if resp.len() >= 16 {
                let rlen = u16::from_le_bytes([resp[4], resp[5]]) as usize;
                let ridx = u16::from_le_bytes([resp[6], resp[7]]);
                if ridx == idx {
                    let end = (16 + rlen).min(resp.len());
                    self.info.lock().unwrap().stats.note_reply(sent.elapsed().as_secs_f64() * 1000.0);
                    return Ok(resp[16..end].to_vec());
                }
            }
        }
        if retry && command_id != 0 {
            self.info.lock().unwrap().stats.resyncs += 1;
            (self.log)("Index de commande désynchronisé : remise à zéro et nouvel essai");
            self.command_ex(0, &[], false)?;
            return self.command_ex(command_id, body, false);
        }
        Err(DevError::Device("Pas de réponse de la GoXLR".into()))
    }

    fn read_info(&mut self) -> Res<()> {
        let body = self.command((0x80F << 12) | 1, &[])?;
        let raw = &body[..body.len().min(24)];
        let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
        let serial = String::from_utf8_lossy(&raw[..end]).to_string();
        let body = self.command(0x80F << 12, &[])?;
        if body.len() < 8 {
            return Err(DevError::Device("Version du firmware illisible".into()));
        }
        let p = u32::from_le_bytes([body[0], body[1], body[2], body[3]]);
        let build = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
        let fw = [p >> 12, (p >> 8) & 0xF, p & 0xFF, build];
        let mut i = self.info.lock().unwrap();
        i.serial = Some(serial);
        i.fw = fw;
        let version = fw.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(".");
        i.firmware = Some(if i.rustudio { format!("Rustudio ({version})") } else { version });
        Ok(())
    }

    pub fn set_volume(&mut self, ch: usize, value: i32) -> Res<()> {
        self.command((0x806 << 12) | ch as u32, &[value.clamp(0, 255) as u8]).map(|_| ())
    }
    pub fn set_monitored_mix(&mut self, mix: u8) -> Res<()> {
        self.command(0x818 << 12, &[mix]).map(|_| ())
    }
    /// Chaque sortie sur le mix A, aucune sur le mix B : ce qu'envoie goxlr-utility sur les firmwares a sous-mixages.
    pub fn set_channel_mixes(&mut self) -> Res<()> {
        let mut outs = vec!["BroadcastMix", "ChatMic", "Sampler", "LineOut"];
        if self.mix2() {
            outs.push("StreamMix2");
        }
        let mut body: Vec<u8> =
            outs.iter().map(|o| (MIX_OUTPUT_ORDER.iter().position(|x| x == o).unwrap() * 2) as u8).collect();
        body.extend(std::iter::repeat_n(0xFF, outs.len()));
        self.command(0x817 << 12, &body).map(|_| ())
    }
    pub fn set_channel_state(&mut self, ch: usize, muted: bool) -> Res<()> {
        self.command((0x809 << 12) | ch as u32, &[muted as u8]).map(|_| ())
    }
    pub fn set_fader(&mut self, fader: usize, ch: usize) -> Res<()> {
        self.command((0x805 << 12) | fader as u32, &[ch as u8, 0, 0, 0]).map(|_| ())
    }
    pub fn set_fader_display(&mut self, fader: usize, gradient: bool, meter: bool) -> Res<()> {
        self.command((0x814 << 12) | fader as u32, &[gradient as u8, meter as u8]).map(|_| ())
    }
    pub fn set_animation(&mut self, mode: u8, mod1: i32, mod2: i32, waterfall: u8) -> Res<()> {
        let enabled = if mode == 5 { 0 } else { 1 };
        self.command(0x816 << 12, &[enabled, mode, mod1.clamp(0, 100) as u8, mod2.clamp(0, 100) as u8, waterfall])
            .map(|_| ())
    }
    pub fn set_routing(&mut self, inp: &str, outs: &indexmap::IndexMap<String, bool>) -> Res<()> {
        let (left, right) = routing_bytes(outs, self.mix2());
        let (gl, gr) = input_ids(inp).ok_or_else(|| DevError::Device(format!("entrée inconnue {inp}")))?;
        self.command((0x804 << 12) | gl, &left)?;
        self.command((0x804 << 12) | gr, &right).map(|_| ())
    }
    pub fn set_colour_map(&mut self, data: &[u8]) -> Res<()> {
        self.command(0x803 << 12, data).map(|_| ())
    }
    pub fn set_button_states(&mut self, states: &[u8]) -> Res<()> {
        self.command(0x808 << 12, states).map(|_| ())
    }
    /// (masque des boutons, niveaux des 4 voies de faders, encodeurs).
    /// Une commande de volume logiciel ne deplace pas les poignees non motorisees.
    /// Conserver cette lecture separee du niveau logiciel demande.
    pub fn get_button_states(&mut self) -> Res<(u32, [u8; 4], [i8; 4])> {
        let b = self.command(0x800 << 12, &[])?;
        if b.len() < 12 {
            return Err(DevError::Device(format!("État des boutons tronqué ({} octets)", b.len())));
        }
        let mask = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        let enc = [b[4] as i8, b[5] as i8, b[6] as i8, b[7] as i8];
        Ok((mask, [b[8], b[9], b[10], b[11]], enc))
    }
    /// Compteurs du firmware Rustudio : erreur sur une Mini au firmware officiel, qui ne les connaît pas.
    pub fn firmware_counters(&mut self) -> Res<FirmwareCounters> {
        if !self.info.lock().unwrap().rustudio {
            return Err(DevError::Device("Compteurs réservés au firmware Rustudio".into()));
        }
        let words = |bytes: Vec<u8>| bytes.chunks_exact(4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]])).collect::<Vec<_>>();
        let audio = words(self.req_in(REQ_FIRMWARE_AUDIO, 24)?);
        let headphones = words(self.req_in(REQ_FIRMWARE_HEADPHONES, 76)?);
        Ok(FirmwareCounters::from_words(&audio, &headphones))
    }

    pub fn mic_level(&mut self) -> Res<u16> {
        let b = self.command(0x80C << 12, &[])?;
        if b.len() < 2 {
            return Err(DevError::Device("Niveau micro tronqué".into()));
        }
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    pub fn set_mic_params(&mut self, pairs: &[(u32, [u8; 4])]) -> Res<()> {
        let mut body = Vec::with_capacity(pairs.len() * 8);
        for (k, v) in pairs {
            body.extend_from_slice(&k.to_le_bytes());
            body.extend_from_slice(v);
        }
        self.command(0x80B << 12, &body).map(|_| ())
    }
    pub fn set_effects(&mut self, pairs: &[(u32, i32)]) -> Res<()> {
        let mut body = Vec::with_capacity(pairs.len() * 8);
        for (k, v) in pairs {
            body.extend_from_slice(&k.to_le_bytes());
            body.extend_from_slice(&v.to_le_bytes());
        }
        self.command(0x801 << 12, &body).map(|_| ())
    }
    pub fn set_mic_gain(&mut self, mic_type: &str, gain_db: i32) -> Res<()> {
        let g = gain_db.clamp(0, 72) as u16;
        let mut val = [0u8; 4];
        val[2..4].copy_from_slice(&g.to_le_bytes());
        let phantom = if mic_type == "condenser" { [1, 0, 0, 0] } else { [0; 4] };
        self.set_mic_params(&[(0x000, phantom), (mic_gain_key(mic_type), val)])
    }
}

/// Niveau micro (dB) depuis la valeur brute du firmware.
pub fn level_db(level: u16) -> f64 {
    if level == 0 {
        return -72.2;
    }
    let db = 20.0 * (level as f64).log10() - 72.2;
    (db.clamp(-72.2, 0.0) * 10.0).round() / 10.0
}

pub fn f32_bytes(v: f64) -> [u8; 4] {
    (v as f32).to_le_bytes()
}


#[cfg(test)]
mod tests {
    use super::*;

    /// Appareil qui repond des la reception de la commande, comme le firmware Rustudio.
    #[derive(Default)]
    struct Immediate {
        frame: Vec<u8>,
    }

    impl Tusb for Immediate {
        fn enumerate(&mut self) -> u32 {
            0
        }
        fn device_count(&mut self) -> u32 {
            1
        }
        fn open_by_index(&mut self, _idx: u32) -> Result<u32, u32> {
            Ok(1)
        }
        fn instance_id(&mut self, _h: u32) -> Option<String> {
            None
        }
        fn vendor_out(&mut self, _h: u32, _request: u32, data: &[u8]) -> u32 {
            self.frame = data.to_vec();
            0
        }
        fn vendor_in(&mut self, _h: u32, _request: u32, _len: usize) -> Result<Vec<u8>, u32> {
            let mut answer = self.frame[..16].to_vec();
            answer[4..6].copy_from_slice(&0u16.to_le_bytes());
            Ok(answer)
        }
        fn close(&mut self, _h: u32) {}
        fn status_text(&self, _code: u32) -> String {
            String::new()
        }
    }

    fn device(rustudio: bool) -> GoXlr {
        let mut goxlr = GoXlr::new(Box::new(|| Err(String::new())));
        goxlr.tusb = Some(Box::new(Immediate::default()));
        goxlr.h = Some(1);
        goxlr.info.lock().unwrap().rustudio = rustudio;
        goxlr
    }

    fn elapsed(rustudio: bool) -> Duration {
        let mut goxlr = device(rustudio);
        let t0 = Instant::now();
        for _ in 0..5 {
            goxlr.command(0x80F, &[]).unwrap();
        }
        t0.elapsed()
    }

    #[test]
    fn compteurs_du_firmware_rustudio() {
        let audio = [1, 480_000, 2_880_000, 0, 3, 2400];
        let mut headphones = [0u32; 19];
        headphones[3] = 2;
        headphones[8] = 5;
        headphones[9] = 7;
        headphones[10] = 1026;
        headphones[11] = 1086;
        headphones[12] = 11; // ticks de 5,12 µs
        headphones[13] = 4;
        let c = FirmwareCounters::from_words(&audio, &headphones);
        assert!(c.streaming);
        assert_eq!((c.packets, c.missed, c.underruns, c.drops, c.repeats), (480_000, 3, 2, 5, 7));
        assert_eq!((c.queue_min, c.queue_max, c.max_loop_us, c.slow_loops), (1026, 1086, 56, 4));
        // réponse tronquée : les mots absents valent zéro, jamais de panique
        assert_eq!(FirmwareCounters::from_words(&[], &[]), FirmwareCounters::default());
    }

    #[test]
    fn compteurs_refuses_au_firmware_officiel() {
        let mut goxlr = device(false);
        assert!(goxlr.firmware_counters().is_err());
    }

    #[test]
    fn firmware_rustudio_lu_sans_attente() {
        assert!(elapsed(true) < Duration::from_millis(50), "5 commandes sans attente de 30 ms");
        assert!(elapsed(false) >= Duration::from_millis(150), "pilote officiel : 30 ms avant chaque lecture");
    }
}
