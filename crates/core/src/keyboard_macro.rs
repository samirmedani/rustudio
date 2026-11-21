//! Macros embarquées du MK770. Limites et codes du protocole du fabricant : docs/MK770-MACROS.md.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const SLOTS: u8 = 12;
pub const MAX_EVENTS: usize = 70;
pub const RECORD_BYTES: usize = 355;
/// Ordre de la matrice de touches, indépendant de l'ordre des LED et de la disposition Windows.
pub const MATRIX: [u16; 104] = [
    41, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 76, 73, 75, 78, 2, 53, 30, 31, 32, 33, 34,
    35, 36, 37, 38, 39, 45, 46, 42, 83, 84, 85, 86, 43, 20, 26, 8, 21, 23, 28, 24, 12, 18, 19, 47,
    48, 49, 95, 96, 97, 87, 57, 4, 22, 7, 9, 10, 11, 13, 14, 15, 51, 52, 40, 92, 93, 94, 225, 100,
    29, 27, 6, 25, 5, 17, 16, 54, 55, 56, 229, 82, 89, 90, 91, 88, 224, 227, 226, 145, 44, 144,
    230, 1, 228, 80, 81, 79, 98, 99, 222, 223,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    Once,
    Hold,
    Toggle,
}
impl Mode {
    pub fn code(self) -> u16 {
        match self {
            Self::Once => 3,
            Self::Hold => 4,
            Self::Toggle => 5,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Once => "Une fois",
            Self::Hold => "Tant que la touche est tenue",
            Self::Toggle => "Un appui démarre, un autre arrête",
        }
    }
    pub fn from_code(code: u16) -> Option<Self> {
        Some(match code {
            3 => Self::Once,
            4 => Self::Hold,
            5 => Self::Toggle,
            _ => return None,
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub key: u16,
    pub down: bool,
    /// Délai avant cet événement, en millisecondes (15 bits dans le clavier).
    pub delay_ms: u16,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Macro {
    pub name: String,
    pub mode: Mode,
    pub events: Vec<Event>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub key: u16,
    pub mode: Mode,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Slot {
    pub id: u8,
    pub bindings: Vec<Binding>,
    pub data: Macro,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assignment {
    pub slot: u8,
    pub key: u16,
    pub data: Macro,
}

pub fn valid_key(key: u16) -> bool {
    matches!(key, 4..=0x73 | 0x87..=0x8f | 0xe0..=0xe7)
}
pub fn source_index(key: u16) -> Result<usize, String> {
    MATRIX
        .iter()
        .position(|&k| k == key)
        .filter(|_| valid_key(key))
        .ok_or_else(|| "Cette touche ne peut pas déclencher une macro.".into())
}
impl Macro {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty()
            || self.name.len() > 39
            || self.name.chars().any(char::is_control)
        {
            return Err("Donne à la macro un nom court, sans retour à la ligne.".into());
        }
        if self.events.is_empty() || self.events.len() > MAX_EVENTS {
            return Err(format!(
                "Une macro contient de 1 à {MAX_EVENTS} événements."
            ));
        }
        let mut held = BTreeSet::new();
        for e in &self.events {
            if !valid_key(e.key) || e.delay_ms > 32767 {
                return Err("Touche ou délai invalide (0 à 32 767 ms).".into());
            }
            if e.down {
                if !held.insert(e.key) {
                    return Err("Une touche est déjà appuyée : ajoute son relâchement.".into());
                }
            } else if !held.remove(&e.key) {
                return Err("Une touche est relâchée sans avoir été appuyée.".into());
            }
        }
        if !held.is_empty() {
            return Err("Relâche toutes les touches avant la fin de la macro.".into());
        }
        Ok(())
    }
    pub fn encode(&self, slot: u8) -> Result<Vec<u8>, String> {
        self.validate()?;
        if slot >= SLOTS {
            return Err("Emplacement de macro inconnu.".into());
        }
        let mut b = vec![0; RECORD_BYTES];
        b[0] = slot;
        b[1] = if self.mode == Mode::Once { 1 } else { 255 };
        b[2] = self.events.len() as u8;
        b[3..3 + self.name.len()].copy_from_slice(self.name.as_bytes());
        for (i, e) in self.events.iter().enumerate() {
            let at = 43 + i * 3;
            b[at] = e.key as u8;
            b[at + 1..at + 3]
                .copy_from_slice(&(e.delay_ms | if e.down { 0x8000 } else { 0 }).to_le_bytes());
        }
        Ok(b)
    }
    pub fn decode(b: &[u8], mode: Mode) -> Result<Self, String> {
        if b.len() != RECORD_BYTES
            || b[2] as usize > MAX_EVENTS
            || b[1] != if mode == Mode::Once { 1 } else { 255 }
        {
            return Err("Macro du clavier illisible ou incompatible.".into());
        }
        let name = &b[3..43];
        let name =
            String::from_utf8(name[..name.iter().position(|&x| x == 0).unwrap_or(40)].to_vec())
                .map_err(|_| {
                    "Nom de macro non UTF-8 : exporte une sauvegarde avant de la remplacer."
                        .to_owned()
                })?;
        let mut events = Vec::new();
        for chunk in b[43..43 + b[2] as usize * 3].chunks_exact(3) {
            let info = u16::from_le_bytes([chunk[1], chunk[2]]);
            events.push(Event {
                key: chunk[0] as u16,
                down: info & 0x8000 != 0,
                delay_ms: info & 0x7fff,
            });
        }
        // Les anciennes macros sont lisibles même si elles ne satisfont pas nos règles d'édition.
        Ok(Self { name, mode, events })
    }
}
impl Assignment {
    pub fn validate(&self) -> Result<usize, String> {
        self.data.validate()?;
        if self.slot >= SLOTS {
            return Err("Emplacement de macro inconnu.".into());
        }
        source_index(self.key)
    }
}

pub fn key_name(key: u16) -> String {
    match key {
        4..=29 => ((b'A' + (key - 4) as u8) as char).to_string(),
        30..=38 => (key - 29).to_string(),
        39 => "0".into(),
        40 => "Entrée".into(),
        41 => "Échap".into(),
        42 => "Retour arrière".into(),
        43 => "Tab".into(),
        44 => "Espace".into(),
        58..=69 => format!("F{}", key - 57),
        76 => "Suppr".into(),
        79 => "Droite".into(),
        80 => "Gauche".into(),
        81 => "Bas".into(),
        82 => "Haut".into(),
        224 => "Ctrl gauche".into(),
        225 => "Maj gauche".into(),
        226 => "Alt gauche".into(),
        227 => "Windows gauche".into(),
        228 => "Ctrl droit".into(),
        229 => "Maj droite".into(),
        230 => "Alt droit".into(),
        231 => "Windows droit".into(),
        _ => format!("HID {key:02X}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> Macro {
        Macro {
            name: "Test".into(),
            mode: Mode::Once,
            events: vec![
                Event {
                    key: 4,
                    down: true,
                    delay_ms: 0,
                },
                Event {
                    key: 4,
                    down: false,
                    delay_ms: 125,
                },
            ],
        }
    }
    #[test]
    fn vendor_record_layout() {
        let m = sample();
        let b = m.encode(11).unwrap();
        assert_eq!(b.len(), 355);
        assert_eq!(&b[..8], &[11, 1, 2, b'T', b'e', b's', b't', 0]);
        assert_eq!(&b[43..49], &[4, 0, 0x80, 4, 125, 0]);
        assert_eq!(Macro::decode(&b, Mode::Once).unwrap(), m);
        for mode in [Mode::Hold, Mode::Toggle] {
            let mut m = m.clone();
            m.mode = mode;
            assert_eq!(m.encode(0).unwrap()[1], 255);
            assert_eq!(Macro::decode(&m.encode(0).unwrap(), mode).unwrap(), m);
        }
    }
    #[test]
    fn refuse_stuck_keys_truncation_and_invalid_sources() {
        let mut m = sample();
        m.events.pop();
        assert!(m.validate().is_err());
        m = sample();
        m.events[1].key = 5;
        assert!(m.validate().is_err());
        m = sample();
        m.events[0].delay_ms = 32768;
        assert!(m.validate().is_err());
        m = sample();
        m.name = "é".repeat(20);
        assert!(m.validate().is_err());
        m = sample();
        m.events = m.events.repeat(36);
        assert!(m.validate().is_err());
        assert!(sample().encode(12).is_err());
        assert!(source_index(1).is_err());
        assert_eq!(source_index(4).unwrap(), 55);
        assert_eq!(source_index(69).unwrap(), 12);
        assert_eq!(MATRIX.iter().collect::<BTreeSet<_>>().len(), 104);
    }
}
