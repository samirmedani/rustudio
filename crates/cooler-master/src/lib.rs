//! Cooler Master MK770, en USB : canal de commande vendor (interface 3, usage page FF00), rapports de 64 octets.
//! Protocole : docs/MK770.md. Faits repris de CoolerHacker-MK770 (Defacedz, MIT, voir LICENSE-CoolerHacker-MK770),
//! complétés par des mesures sur un MK770 réel : vitesse et sens d'un effet, tampon par touche en trame continue.
//! Aucun firmware n'est écrit ; seules la configuration des effets et la couleur des touches sont modifiées.
use rustudio_core::mk770::{self as data, format_colour, parse_colour};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type Result<T> = std::result::Result<T, String>;
pub const VID: u16 = 0x2516;
/// Clavier branché par câble.
pub const WIRED: u16 = 0x01D5;
/// Récepteur 2,4 GHz : pas encore pris en charge (rapports de 33 octets, découpage différent).
pub const DONGLE: u16 = 0x01C3;
/// Rapport sans son report ID (Windows en ajoute un, à 0).
pub const REPORT: usize = 64;
/// Le clavier ne répond pas sur son canal de commande.
pub const NO_RESPONSE: &str = "Le MK770 ne répond pas sur son canal de commande";

const OP_WRITE: u8 = 0x01;
const OP_READ: u8 = 0x02;
const OP_KEYS_WRITE: u8 = 0x0A;
const OP_KEYS_READ: u8 = 0x0B;
const MARK: u8 = 0x81;
/// Adresse de l'effet actif, juste après les 20 fiches de 10 octets.
const ACTIVE: u16 = 200;
const RECORD: usize = 10;
/// Lecture de configuration : 16 octets par requête (taille vérifiée sur le clavier).
const READ_CHUNK: usize = 16;
/// Tampon par touche : 104 LED écrites, 102 relues, 4 octets chacune (R, V, B, FF).
const KEYS_WRITE_BYTES: usize = data::LEDS * 4;
const KEYS_READ_BYTES: usize = data::LEDS_READ * 4;
/// Trame continue du câble : 58 octets dans le premier rapport, 62 dans les suivants.
const FIRST_DATA: usize = REPORT - 6;
const NEXT_DATA: usize = REPORT - 2;

pub trait Transport: Send {
    /// Envoie un rapport de 64 octets (sans report ID).
    fn send(&mut self, report: &[u8; REPORT]) -> Result<()>;
    /// Reçoit un rapport (sans report ID) ; None si rien n'arrive avant le délai.
    fn receive(&mut self, timeout_ms: i32) -> Result<Option<Vec<u8>>>;
    fn wired(&self) -> bool;
    /// Version du firmware annoncée par l'USB (bcdDevice), si connue.
    fn firmware(&self) -> Option<String> {
        None
    }
}
impl<T: Transport> Transport for &mut T {
    fn send(&mut self, report: &[u8; REPORT]) -> Result<()> { (**self).send(report) }
    fn receive(&mut self, timeout_ms: i32) -> Result<Option<Vec<u8>>> { (**self).receive(timeout_ms) }
    fn wired(&self) -> bool { (**self).wired() }
    fn firmware(&self) -> Option<String> { (**self).firmware() }
}
impl<T: Transport + ?Sized> Transport for Box<T> {
    fn send(&mut self, report: &[u8; REPORT]) -> Result<()> { (**self).send(report) }
    fn receive(&mut self, timeout_ms: i32) -> Result<Option<Vec<u8>>> { (**self).receive(timeout_ms) }
    fn wired(&self) -> bool { (**self).wired() }
    fn firmware(&self) -> Option<String> { (**self).firmware() }
}

/// Commande : [opcode][0x81][charge utile…], complétée de zéros.
pub fn command(opcode: u8, payload: &[u8]) -> Result<[u8; REPORT]> {
    if payload.len() > REPORT - 2 {
        return Err("Commande clavier trop longue".into());
    }
    let mut b = [0; REPORT];
    b[0] = opcode;
    b[1] = MARK;
    b[2..2 + payload.len()].copy_from_slice(payload);
    Ok(b)
}

/// Rapports d'une écriture du tampon par touche sur le câble : en-tête [0x0A][0x80 | nombre de rapports]
/// [adresse][longueur], puis les données sans découpage par LED, et les suites numérotées [0x0A][n].
pub fn key_frames(colours: &[[u8; 3]]) -> Vec<[u8; REPORT]> {
    let mut payload = vec![0u8; KEYS_WRITE_BYTES];
    for (i, c) in colours.iter().take(data::LEDS).enumerate() {
        payload[i * 4..i * 4 + 4].copy_from_slice(&[c[0], c[1], c[2], 0xFF]);
    }
    for i in colours.len().min(data::LEDS)..data::LEDS {
        payload[i * 4 + 3] = 0xFF;
    }
    let count = 1 + (KEYS_WRITE_BYTES - FIRST_DATA).div_ceil(NEXT_DATA);
    let mut frames = Vec::with_capacity(count);
    let mut first = [0u8; REPORT];
    first[0] = OP_KEYS_WRITE;
    first[1] = 0x80 | count as u8;
    first[4..6].copy_from_slice(&(KEYS_WRITE_BYTES as u16).to_le_bytes());
    first[6..].copy_from_slice(&payload[..FIRST_DATA]);
    frames.push(first);
    for (n, chunk) in payload[FIRST_DATA..].chunks(NEXT_DATA).enumerate() {
        let mut f = [0u8; REPORT];
        f[0] = OP_KEYS_WRITE;
        f[1] = n as u8 + 1;
        f[2..2 + chunk.len()].copy_from_slice(chunk);
        frames.push(f);
    }
    frames
}

/// Un effet tel que le clavier l'enregistre.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Effect {
    pub id: u8,
    pub brightness: u8,
    pub speed: u8,
    pub reverse: bool,
    pub colour: String,
}

impl Effect {
    fn from_record(id: u8, r: &[u8]) -> Effect {
        Effect { id, brightness: r[0], speed: r[2] & 0x0F, reverse: r[2] & 0x10 != 0, colour: format_colour([r[3], r[4], r[5]]) }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub wired: bool,
    pub firmware: Option<String>,
    /// effet affiché (0 à 19 ; 17 à 19 n'affichent rien)
    pub active_effect: u8,
    /// les 20 fiches, dans l'ordre
    pub effects: Vec<Effect>,
    /// couleur relue de chaque LED (102 ; les deux dernières ne se relisent pas)
    pub keys: Vec<String>,
}

/// Modification demandée : chaque champ absent reste tel quel.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Changes {
    pub active_effect: Option<u8>,
    /// fiche à modifier (par défaut celle de l'effet choisi, sinon de l'effet actif)
    pub effect: Option<u8>,
    pub brightness: Option<u8>,
    pub speed: Option<u8>,
    pub reverse: Option<bool>,
    pub colour: Option<String>,
    /// une couleur par LED (104 au plus) ; active l'effet « Personnalisé »
    pub keys: Option<Vec<String>>,
}

pub struct Keyboard<T: Transport> {
    transport: T,
}

impl<T: Transport> Keyboard<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    /// Jette les rapports restés en attente (réponse arrivée après un délai dépassé).
    fn drain(&mut self) -> Result<()> {
        for _ in 0..32 {
            if self.transport.receive(0)?.is_none() {
                break;
            }
        }
        Ok(())
    }

    fn read_config(&mut self, address: u16, len: usize) -> Result<Vec<u8>> {
        if len == 0 || len > READ_CHUNK {
            return Err("Lecture de configuration invalide".into());
        }
        self.drain()?;
        let a = address.to_le_bytes();
        let l = (len as u16).to_le_bytes();
        self.transport.send(&command(OP_READ, &[a[0], a[1], l[0], l[1]])?)?;
        for _ in 0..8 {
            let Some(r) = self.transport.receive(300)? else { break };
            // réponse : [0x02][?][adresse][longueur][données…] ; tout autre rapport est ignoré
            if r.len() >= 6 + len && r[0] == OP_READ && r[2..4] == a && r[4..6] == l {
                return Ok(r[6..6 + len].to_vec());
            }
        }
        Err(format!("{NO_RESPONSE} (lecture à l'adresse {address})"))
    }

    fn write_config(&mut self, address: u16, bytes: &[u8]) -> Result<()> {
        let a = address.to_le_bytes();
        let l = (bytes.len() as u16).to_le_bytes();
        let mut payload = vec![a[0], a[1], l[0], l[1]];
        payload.extend_from_slice(bytes);
        self.transport.send(&command(OP_WRITE, &payload)?)
    }

    /// Fiches des 20 effets puis effet actif : 208 octets en 13 lectures.
    fn read_memory(&mut self) -> Result<Vec<u8>> {
        let mut mem = Vec::with_capacity(208);
        for address in (0..208).step_by(READ_CHUNK) {
            mem.extend(self.read_config(address as u16, READ_CHUNK)?);
        }
        Ok(mem)
    }

    pub fn read_keys(&mut self) -> Result<Vec<[u8; 3]>> {
        self.drain()?;
        let l = (KEYS_READ_BYTES as u16).to_le_bytes();
        self.transport.send(&command(OP_KEYS_READ, &[0, 0, l[0], l[1]])?)?;
        let mut bytes = Vec::with_capacity(KEYS_READ_BYTES + NEXT_DATA);
        let mut next = 1u8;
        for _ in 0..16 {
            let Some(r) = self.transport.receive(300)? else { break };
            if r.len() < REPORT || r[0] != OP_KEYS_READ {
                continue;
            }
            if bytes.is_empty() && r[1] & 0x80 != 0 && r[2..4] == [0, 0] && r[4..6] == l {
                bytes.extend_from_slice(&r[6..]);
            } else if !bytes.is_empty() && r[1] == next {
                bytes.extend_from_slice(&r[2..]);
                next += 1;
            }
            if bytes.len() >= KEYS_READ_BYTES {
                return Ok(bytes[..KEYS_READ_BYTES].chunks_exact(4).map(|c| [c[0], c[1], c[2]]).collect());
            }
        }
        Err(format!("{NO_RESPONSE} (couleurs des touches)"))
    }

    fn write_keys(&mut self, colours: &[[u8; 3]]) -> Result<()> {
        for frame in key_frames(colours) {
            self.transport.send(&frame)?;
        }
        Ok(())
    }

    pub fn read(&mut self) -> Result<Settings> {
        if !self.transport.wired() {
            return Err("Le récepteur 2,4 GHz n'est pas encore pris en charge : branche le câble du clavier".into());
        }
        let mem = self.read_memory()?;
        let active_effect = mem[ACTIVE as usize];
        if active_effect >= data::RECORDS {
            return Err("Effet actif inconnu".into());
        }
        let effects = (0..data::RECORDS).map(|id| Effect::from_record(id, &mem[id as usize * RECORD..][..RECORD])).collect();
        let keys = self.read_keys()?.into_iter().map(format_colour).collect();
        Ok(Settings { wired: true, firmware: self.transport.firmware(), active_effect, effects, keys })
    }

    /// Vérifie toute la demande avant d'écrire quoi que ce soit.
    fn validate(changes: &Changes) -> Result<(Option<[u8; 3]>, Option<Vec<[u8; 3]>>)> {
        if let Some(id) = changes.active_effect {
            if data::effect(id).is_none() {
                return Err("Effet inconnu".into());
            }
        }
        if let Some(id) = changes.effect {
            if data::effect(id).is_none() {
                return Err("Effet inconnu".into());
            }
        }
        if changes.speed.is_some_and(|s| s > data::SPEED_MAX) {
            return Err("Vitesse autorisée : 0 à 4".into());
        }
        let colour = match &changes.colour {
            Some(c) => Some(parse_colour(c).ok_or("Couleur attendue au format #RRGGBB")?),
            None => None,
        };
        let keys = match &changes.keys {
            Some(list) if list.is_empty() || list.len() > data::LEDS => return Err("Couleurs des touches : 1 à 104".into()),
            Some(list) => Some(list.iter().map(|c| parse_colour(c).ok_or("Couleur de touche attendue au format #RRGGBB")).collect::<std::result::Result<Vec<_>, _>>()?),
            None => None,
        };
        let edits = changes.brightness.is_some() || changes.speed.is_some() || changes.reverse.is_some() || colour.is_some();
        if !edits && changes.active_effect.is_none() && keys.is_none() {
            return Err("Aucun réglage à enregistrer".into());
        }
        Ok((colour, keys))
    }

    /// Applique une demande puis relit tout : rien n'est annoncé réussi sans relecture conforme.
    /// Ordre du logiciel officiel : effet choisi d'abord, sa fiche ensuite, couleurs des touches en dernier.
    pub fn apply(&mut self, changes: &Changes) -> Result<Settings> {
        let (colour, keys) = Self::validate(changes)?;
        let before = self.read()?;
        let active = match (changes.active_effect, &keys) {
            (Some(id), _) => Some(id),
            (None, Some(_)) => Some(data::CUSTOM),
            (None, None) => None,
        };
        let target = changes.effect.or(changes.active_effect).unwrap_or(before.active_effect);
        if let Some(id) = active {
            if id != before.active_effect {
                self.write_config(ACTIVE, &[id])?;
            }
        }
        let edits = changes.brightness.is_some() || changes.speed.is_some() || changes.reverse.is_some() || colour.is_some();
        let mut record = Vec::new();
        if edits {
            if data::effect(target).is_none() {
                return Err("Cet effet ne se règle pas".into());
            }
            // fiche relue juste avant : les octets que Rustudio ne connaît pas sont conservés
            record = self.read_config(target as u16 * RECORD as u16, RECORD)?;
            if let Some(b) = changes.brightness {
                record[0] = b;
            }
            if let Some(s) = changes.speed {
                record[2] = (record[2] & !0x0F) | s;
            }
            if let Some(r) = changes.reverse {
                record[2] = if r { record[2] | 0x10 } else { record[2] & !0x10 };
            }
            if let Some(c) = colour {
                record[3..6].copy_from_slice(&c);
            }
            self.write_config(target as u16 * RECORD as u16, &record)?;
        }
        if let Some(k) = &keys {
            self.write_keys(k)?;
        }
        let after = self.read().map_err(|e| format!("Enregistrement non confirmé : {e}. Actualise avant de réessayer."))?;
        if let Some(id) = active {
            if after.active_effect != id {
                return Err("Enregistrement non confirmé : l'effet choisi n'a pas été relu. Actualise avant de réessayer.".into());
            }
        }
        if edits {
            let relu = &after.effects[target as usize];
            if *relu != Effect::from_record(target, &record) {
                return Err("Enregistrement non confirmé : les réglages de l'effet n'ont pas été relus. Actualise avant de réessayer.".into());
            }
        }
        if let Some(k) = &keys {
            let sent: Vec<String> = k.iter().take(data::LEDS_READ).map(|c| format_colour(*c)).collect();
            let padded = (k.len()..data::LEDS_READ).map(|_| format_colour([0, 0, 0]));
            let expected: Vec<String> = sent.into_iter().chain(padded).collect();
            if after.keys != expected {
                return Err("Enregistrement non confirmé : les couleurs des touches relues diffèrent. Réessaie.".into());
            }
        }
        Ok(after)
    }

    /// Demande au format de l'API : {"active_effect":1} ou {"brightness":255,"keys":[…]}.
    pub fn apply_json(&mut self, body: &Value) -> Result<Settings> {
        let changes: Changes = serde_json::from_value(body.clone()).map_err(|_| "Réglage du clavier inconnu ou invalide".to_string())?;
        self.apply(&changes)
    }
}

#[cfg(windows)]
pub struct Hid {
    device: hidapi::HidDevice,
    firmware: Option<String>,
}

#[cfg(windows)]
impl Hid {
    /// Interface de commande du clavier branché : interface 3, usage page FF00. Les interfaces clavier,
    /// souris et notifications ne sont jamais ouvertes.
    pub fn open() -> Result<Self> {
        let api = hidapi::HidApi::new().map_err(|e| e.to_string())?;
        let info = api
            .device_list()
            .find(|d| d.vendor_id() == VID && d.product_id() == WIRED && d.interface_number() == 3 && d.usage_page() == 0xFF00)
            .ok_or("MK770 absent : branche son câble USB (le récepteur 2,4 GHz n'est pas encore pris en charge)")?;
        let device = info.open_device(&api).map_err(|_| "MK770 présent mais indisponible (canal déjà réservé ?)".to_string())?;
        let r = info.release_number();
        let firmware = (r != 0).then(|| format!("{}.{:02X}", r >> 8, r & 0xFF));
        Ok(Self { device, firmware })
    }
}

#[cfg(windows)]
impl Transport for Hid {
    fn send(&mut self, report: &[u8; REPORT]) -> Result<()> {
        let mut out = [0u8; REPORT + 1];
        out[1..].copy_from_slice(report);
        self.device.write(&out).map_err(|_| "Écriture vers le clavier impossible".to_string())?;
        // le câble ne perd rien même sans pause ; 2 ms laissent le clavier traiter chaque rapport
        std::thread::sleep(std::time::Duration::from_millis(2));
        Ok(())
    }
    fn receive(&mut self, timeout_ms: i32) -> Result<Option<Vec<u8>>> {
        let mut buf = [0u8; REPORT + 1];
        let n = self.device.read_timeout(&mut buf, timeout_ms).map_err(|_| "Lecture du clavier impossible".to_string())?;
        if n == 0 {
            return Ok(None);
        }
        // selon le chemin Windows, le report ID 0 précède ou non les données
        let r = if n == REPORT + 1 && buf[0] == 0 { &buf[1..n] } else { &buf[..n] };
        Ok(Some(r.to_vec()))
    }
    fn wired(&self) -> bool {
        true
    }
    fn firmware(&self) -> Option<String> {
        self.firmware.clone()
    }
}

pub mod sim;
