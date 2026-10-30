//! R5 Ultra : interface vendor uniquement, jamais souris/clavier ni bootloader.
//! Documentation du protocole : docs/R5-ULTRA.md. Aucun firmware ou macro executee.
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub type Result<T> = std::result::Result<T, String>;
pub const VID: u16 = 0x373e;
pub const WIRED: u16 = 0x0046;
pub const WIRELESS: u16 = 0x0047;
/// Limite du modele R5 Ultra (y compris son palier overclocke documente par le fabricant).
pub const MAX_DPI: u16 = 42000;
pub const RATES: &[(u32, u8)] = &[(125, 8), (250, 4), (500, 2), (1000, 1), (2000, 32), (4000, 64), (8000, 128)];

pub trait Transport: Send {
    fn exchange(&mut self, request: &[u8; 64]) -> Result<Vec<u8>>;
    fn wired(&self) -> bool;
}

impl<T: Transport> Transport for &mut T {
    fn exchange(&mut self, request: &[u8; 64]) -> Result<Vec<u8>> { (**self).exchange(request) }
    fn wired(&self) -> bool { (**self).wired() }
}
impl<T: Transport + ?Sized> Transport for Box<T> {
    fn exchange(&mut self, request: &[u8; 64]) -> Result<Vec<u8>> { (**self).exchange(request) }
    fn wired(&self) -> bool { (**self).wired() }
}

pub fn frame(target: u8, command: u8, payload: &[u8]) -> Result<[u8; 64]> {
    if payload.len() > 58 { return Err("Commande HID trop longue".into()); }
    let mut b = [0; 64];
    b[2] = 2; b[3] = payload.len() as u8; b[4] = target; b[5] = command;
    b[6..6 + payload.len()].copy_from_slice(payload);
    Ok(b)
}

/// Normalise le report ID Windows et valide l'enveloppe avant toute lecture.
pub fn payload<'a>(reply: &'a [u8], request: &[u8; 64]) -> Result<&'a [u8]> {
    let r = if reply.len() == 65 && reply[0] == 0 { &reply[1..] } else { reply };
    if r.len() < 6 || ![0xa1, 0x02].contains(&r[0]) || r[2] != request[2] || r[4] != request[4] || r[5] != request[5] {
        return Err("Réponse HID inattendue (commande ou appareil différent)".into());
    }
    let n = r[3] as usize;
    if n == 0 || n > 58 || r.len() < 6 + n { return Err("Réponse HID tronquée".into()); }
    Ok(&r[6..6 + n])
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dpi { pub x: u16, pub y: u16 }
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub firmware: String, pub wired: bool, pub profile: u8,
    pub battery: u8, pub charging: bool, pub stages: Vec<Dpi>, pub active_stage: u8,
    pub dpi_max: u16, pub polling_hz: u32, pub lod_mm: f64, pub debounce_ms: u8,
    pub sleep_seconds: u16, pub angle_snap: bool, pub motion_sync: bool, pub ripple_control: bool,
}

pub struct Mouse<T: Transport> { transport: T, deadline: Option<std::time::Instant> }
impl<T: Transport> Mouse<T> {
    pub fn new(transport: T) -> Self { Self { transport, deadline: None } }
    pub fn set_deadline(&mut self, deadline: Option<std::time::Instant>) { self.deadline = deadline; }
    fn query(&mut self, target: u8, command: u8, body: &[u8]) -> Result<Vec<u8>> {
        if self.deadline.is_some_and(|d| std::time::Instant::now() >= d) { return Err("Commande expirée : aucune nouvelle requête envoyée".into()); }
        let request = frame(target, command, body)?;
        let reply = self.transport.exchange(&request).map_err(|e| format!("{e} (cible {target}, commande {command:02x})"))?;
        Ok(payload(&reply, &request)?.to_vec())
    }
    fn value(&mut self, target: u8, command: u8, profile: u8) -> Result<u8> {
        self.query(target, command, &[profile])?.get(1).copied().ok_or("Valeur HID absente".into())
    }
    pub fn read(&mut self) -> Result<Settings> {
        let fw = self.query(0, 0x81, &[0; 16])?;
        if fw.len() < 4 { return Err("Version de firmware absente".into()); }
        let firmware = fw[..4].iter().map(u8::to_string).collect::<Vec<_>>().join(".");
        let profile = self.query(0, 0x85, &[0])?[0];
        if !(1..=4).contains(&profile) { return Err("Profil de souris inconnu".into()); }
        let battery = self.query(0, 0x83, &[0, 0])?;
        if battery.len() < 2 || battery[0] > 1 || battery[1] > 100 { return Err("Batterie illisible (souris en veille ?)".into()); }
        let data = self.query(1, 0x81, &[profile, 6])?;
        let count = *data.get(1).ok_or("Paliers DPI absents")? as usize;
        if !(1..=6).contains(&count) || data.len() < 2 + 4 * count { return Err("Paliers DPI invalides".into()); }
        let stages = data[2..2 + 4 * count].chunks_exact(4).map(|b| Dpi { x: u16::from_be_bytes([b[0], b[1]]), y: u16::from_be_bytes([b[2], b[3]]) }).collect();
        let active_stage = self.value(1, 0x82, profile)?;
        if active_stage == 0 || active_stage as usize > count { return Err("Palier DPI actif invalide".into()); }
        let dpi_max = MAX_DPI;
        let rate = self.value(1, 0x80, profile)?;
        let polling_hz = RATES.iter().find(|(_, b)| *b == rate || (rate == 16 && *b == 1)).ok_or("Fréquence inconnue")?.0;
        let lod = self.value(1, 0x88, profile)?;
        let lod_mm = match lod { 0x87 => 0.7, 1 => 1.0, 2 => 2.0, _ => return Err("Hauteur de décrochage inconnue".into()) };
        let debounce_ms = self.value(0, 0x88, profile)?;
        let sleep = self.query(0, 0x87, &[profile])?;
        if sleep.len() < 3 { return Err("Veille illisible".into()); }
        let sleep_seconds = u16::from_be_bytes([sleep[1], sleep[2]]);
        let mut toggle = |cmd| -> Result<bool> { match self.value(1, cmd, profile)? { 0 => Ok(false), 1 => Ok(true), _ => Err("Option HID inconnue".into()) } };
        let angle_snap = toggle(0x84)?; let motion_sync = toggle(0x89)?; let ripple_control = toggle(0x8a)?;
        Ok(Settings { firmware, wired: self.transport.wired(), profile, battery: battery[1], charging: battery[0] == 1,
            stages, active_stage, dpi_max, polling_hz, lod_mm, debounce_ms, sleep_seconds, angle_snap, motion_sync, ripple_control })
    }
    /// Une seule valeur par commande, profil et limites relus avant ecriture, verification par relecture.
    pub fn apply(&mut self, name: &str, value: &Value) -> Result<Settings> {
        let before = self.read()?;
        let p = before.profile;
        let number = || value.as_u64().ok_or_else(|| "Valeur entière attendue".to_string());
        let (target, cmd, bytes) = match name {
            "active_stage" => { let n = number()?; if n == 0 || n > before.stages.len() as u64 { return Err("Palier invalide".into()); } (1, 2, vec![p, n as u8]) }
            "polling_hz" => { let n = number()?; if before.wired && n > 1000 { return Err("Le mode filaire est limité à 1 000 Hz".into()); }
                let raw = RATES.iter().find(|(hz, _)| *hz as u64 == n).ok_or("Fréquence non prise en charge")?.1; (1, 0, vec![p, raw]) }
            "debounce_ms" => { let n = number()?; if n > 30 { return Err("Délai autorisé : 0 à 30 ms".into()); } (0, 8, vec![p, n as u8]) }
            "sleep_seconds" => { let n = number()?; if n > 65535 { return Err("Veille invalide".into()); } let b = (n as u16).to_be_bytes(); (0, 7, vec![p, b[0], b[1]]) }
            "lod_mm" => { let n = value.as_f64().ok_or("Hauteur invalide")?; let raw = if n == 0.7 { 0x87 } else if n == 1.0 { 1 } else if n == 2.0 { 2 } else { return Err("Hauteur autorisée : 0,7, 1 ou 2 mm".into()); }; (1, 8, vec![p, raw]) }
            "angle_snap" | "motion_sync" | "ripple_control" => { let on = value.as_bool().ok_or("Booléen attendu")?; (1, match name { "angle_snap" => 4, "motion_sync" => 9, _ => 10 }, vec![p, on as u8]) }
            "stages" => { let stages: Vec<Dpi> = serde_json::from_value(value.clone()).map_err(|_| "Paliers DPI invalides")?;
                if stages.is_empty() || stages.len() > 6 || stages.len() < before.active_stage as usize || stages.iter().any(|s| s.x < 50 || s.y < 50 || s.x > before.dpi_max || s.y > before.dpi_max || s.x % 50 != 0 || s.y % 50 != 0) { return Err("DPI hors limites (pas de 50), ou palier actif absent".into()); }
                let mut b = vec![p, stages.len() as u8]; for s in stages { b.extend(s.x.to_be_bytes()); b.extend(s.y.to_be_bytes()); } (1, 1, b) }
            _ => return Err("Réglage inconnu".into()),
        };
        // Un ACK perdu n'est pas une preuve d'echec : seule la relecture fait autorite.
        let sent = self.query(target, cmd, &bytes);
        let after = self.read().map_err(|e| format!("Réglage non confirmé : {e}. Actualise avant de réessayer. ACK : {}", if sent.is_ok() { "reçu" } else { "absent" }))?;
        let actual = serde_json::to_value(&after).map_err(|e| e.to_string())?;
        if actual[name] != *value || after.profile != p { return Err("La souris n’a pas confirmé le réglage. Actualise avant de réessayer.".into()); }
        Ok(after)
    }
}

#[cfg(windows)]
pub struct Hid { device: hidapi::HidDevice, wired: bool }
#[cfg(windows)]
impl Hid {
    pub fn open() -> Result<Self> {
        let api = hidapi::HidApi::new().map_err(|e| e.to_string())?;
        let mut devices: Vec<_> = api.device_list().filter(|d| d.vendor_id() == VID && [WIRED, WIRELESS].contains(&d.product_id()) && d.usage_page() == 0xffff).collect();
        devices.sort_by_key(|d| d.product_id()); // Le cable actif prime si le recepteur est reste branche.
        for info in devices {
            if let Ok(device) = info.open_device(&api) {
                if std::env::var_os("RUSTUDIO_HID_TRACE").is_some() { eprintln!("PID {:04x}, usage {:04x}, interface {}", info.product_id(), info.usage(), info.interface_number()); }
                return Ok(Self { device, wired: info.product_id() == WIRED });
            }
        }
        Err("R5 Ultra absente : branche son récepteur 2,4 GHz ou son câble USB".into())
    }
}
#[cfg(windows)]
impl Transport for Hid {
    fn wired(&self) -> bool { self.wired }
    fn exchange(&mut self, request: &[u8; 64]) -> Result<Vec<u8>> {
        let mut report = [0; 65]; report[1..].copy_from_slice(request);
        // Les requetes de lecture perdues en 2,4 GHz peuvent etre renvoyees. Jamais les ecritures.
        let attempts = if request[5] & 0x80 != 0 { 3 } else { 1 };
        for _ in 0..attempts {
            self.device.send_feature_report(&report).map_err(|_| "Écriture HID impossible".to_string())?;
            std::thread::sleep(std::time::Duration::from_millis(150));
            for _ in 0..8 {
                let mut response = [0; 65];
                let n = self.device.get_feature_report(&mut response).map_err(|_| "Lecture HID impossible".to_string())?;
                if payload(&response[..n], request).is_ok() { return Ok(response[..n].to_vec()); }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
        Err("La R5 Ultra ne répond pas (veille ou liaison perdue)".into())
    }
}

pub mod sim;
