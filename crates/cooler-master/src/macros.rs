//! Commandes de macros natives ; aucune injection de touches par Windows.
use crate::{Keyboard, NO_RESPONSE, REPORT, Result, Transport, command};
use rustudio_core::keyboard_macro::{
    self as data, Assignment, Binding, MATRIX, Macro, Mode, RECORD_BYTES, Slot,
};

/// Trame continue USB : deux octets d'en-tête, puis 62 octets de données.
pub fn frames(op: u8, payload: &[u8]) -> Vec<[u8; REPORT]> {
    let count = payload.len().div_ceil(62).max(1);
    let mut out = Vec::new();
    for i in 0..count {
        let mut r = [0; REPORT];
        r[0] = op;
        r[1] = if i == 0 { 0x80 | count as u8 } else { i as u8 };
        let at = i * 62;
        let end = (at + 62).min(payload.len());
        if at < end {
            r[2..2 + end - at].copy_from_slice(&payload[at..end]);
        }
        out.push(r);
    }
    out
}
impl<T: Transport> Keyboard<T> {
    fn macro_profile(&mut self) -> Result<u8> {
        let p = self.read_config(804, 1)?[0];
        if p > 3 {
            return Err("Profil du clavier inconnu.".into());
        }
        Ok(p)
    }
    fn same_macro_profile(&mut self, profile: u8) -> Result<()> {
        if self.macro_profile()? != profile {
            return Err(
                "Le profil du clavier a changé : relis les macros avant de continuer.".into(),
            );
        }
        Ok(())
    }
    fn macro_send(&mut self, op: u8, payload: &[u8]) -> Result<()> {
        for r in frames(op, payload) {
            self.transport.send(&r)?;
        }
        Ok(())
    }
    fn macro_receive(&mut self, op: u8, len: usize) -> Result<Vec<u8>> {
        let count = len.div_ceil(62);
        let mut bytes = Vec::with_capacity(len + 62);
        let mut next = 0;
        for _ in 0..count + 12 {
            let Some(r) = self.transport.receive(300)? else {
                break;
            };
            if r.len() != REPORT || r[0] != op {
                continue;
            }
            let expected = if next == 0 {
                0x80 | count as u8
            } else {
                next as u8
            };
            if r[1] != expected {
                return Err("Réponse de macro incomplète ou désordonnée.".into());
            }
            bytes.extend_from_slice(&r[2..]);
            next += 1;
            if next == count {
                bytes.truncate(len);
                return Ok(bytes);
            }
        }
        Err(format!("{NO_RESPONSE} (macros)"))
    }
    fn macro_mapping(&mut self) -> Result<Vec<u16>> {
        if !self.transport.wired() {
            return Err("Branche le câble USB du MK770 pour ses macros.".into());
        }
        self.drain()?;
        self.transport.send(&command(9, &[])?)?;
        Ok(self
            .macro_receive(9, 208)?
            .chunks_exact(2)
            .map(|p| u16::from_le_bytes([p[0], p[1]]))
            .collect())
    }
    fn macro_record(&mut self, slot: u8) -> Result<Vec<u8>> {
        self.drain()?;
        self.transport.send(&command(5, &[slot])?)?;
        let record = self.macro_receive(5, RECORD_BYTES)?;
        if record[0] != slot {
            return Err("Le clavier a répondu avec un autre emplacement de macro.".into());
        }
        Ok(record)
    }
    fn macro_bind(&mut self, index: usize, value: u16) -> Result<()> {
        let v = value.to_le_bytes();
        // Paramètres de SET_RECORD_KEY : index de départ, nombre de touches, puis codes LE16.
        self.transport
            .send(&command(8, &[index as u8, 1, v[0], v[1]])?)
    }
    pub fn read_macros(&mut self) -> Result<Vec<Slot>> {
        let profile = self.macro_profile()?;
        let map = self.macro_mapping()?;
        let mut slots = Vec::new();
        if map.iter().any(|v| {
            v >> 12 > 2 && (Mode::from_code(v >> 12).is_none() || v & 0xfff >= data::SLOTS as u16)
        }) {
            return Err("Une affectation du clavier utilise un format de macro inconnu.".into());
        }
        for id in 0..data::SLOTS {
            let bindings: Vec<_> = map
                .iter()
                .enumerate()
                .filter_map(|(i, &v)| {
                    (v & 0xfff == id as u16)
                        .then(|| {
                            Mode::from_code(v >> 12).map(|mode| Binding {
                                key: MATRIX[i],
                                mode,
                            })
                        })
                        .flatten()
                })
                .collect();
            if let Some(b) = bindings.first() {
                let data = Macro::decode(&self.macro_record(id)?, b.mode)?;
                slots.push(Slot { id, bindings, data });
            }
        }
        // Un changement de profil pendant la lecture ne doit pas mélanger deux configurations.
        if self.macro_mapping()? != map {
            return Err(
                "Les affectations ont changé pendant la lecture ; actualise les macros.".into(),
            );
        }
        self.same_macro_profile(profile)?;
        Ok(slots)
    }
    pub fn assign_macro(&mut self, assignment: &Assignment) -> Result<Vec<Slot>> {
        let index = assignment.validate()?;
        let encoded = assignment.data.encode(assignment.slot)?;
        let profile = self.macro_profile()?;
        let map = self.macro_mapping()?;
        let old = map[index];
        if old != 0
            && !(old & 0xfff == assignment.slot as u16 && Mode::from_code(old >> 12).is_some())
        {
            return Err("Cette touche a déjà une affectation. Retire sa macro avant d’en choisir une autre.".into());
        }
        let previous = self.macro_record(assignment.slot)?;
        let changes: Vec<_> = map
            .iter()
            .enumerate()
            .filter_map(|(i, &v)| {
                let same_slot =
                    v & 0xfff == assignment.slot as u16 && Mode::from_code(v >> 12).is_some();
                let value = if i == index {
                    assignment.data.mode.code() << 12 | assignment.slot as u16
                } else if same_slot {
                    0
                } else {
                    v
                };
                (value != v).then_some((i, value))
            })
            .collect();
        let result = (|| {
            self.same_macro_profile(profile)?;
            self.macro_send(4, &encoded)?;
            if self.macro_record(assignment.slot)? != encoded {
                return Err("La macro écrite n’a pas été relue à l’identique.".into());
            }
            // Revalider juste avant de remplacer une affectation.
            if self.macro_mapping()? != map {
                return Err("Les affectations ont changé pendant l’envoi.".into());
            }
            self.same_macro_profile(profile)?;
            for &(i, v) in &changes {
                self.same_macro_profile(profile)?;
                self.macro_bind(i, v)?;
            }
            let mut expected = map.clone();
            for &(i, v) in &changes {
                expected[i] = v;
            }
            if self.macro_mapping()? != expected {
                return Err("L’affectation de la macro n’a pas été confirmée.".into());
            }
            let slots = self.read_macros()?;
            self.same_macro_profile(profile)?;
            Ok(slots)
        })();
        if let Err(error) = result {
            let rollback = (|| -> Result<()> {
                self.same_macro_profile(profile)?;
                self.macro_send(4, &previous)?;
                for &(i, _) in &changes {
                    self.same_macro_profile(profile)?;
                    self.macro_bind(i, map[i])?;
                }
                if self.macro_record(assignment.slot)? != previous || self.macro_mapping()? != map {
                    return Err("relecture différente".into());
                }
                Ok(())
            })();
            return Err(if rollback.is_ok() {
                format!("{error} Les réglages précédents ont été restaurés.")
            } else {
                format!(
                    "{error} Restauration non confirmée : rebranche le clavier et relis ses macros."
                )
            });
        }
        result
    }
    pub fn remove_macro(&mut self, slot: u8) -> Result<Vec<Slot>> {
        if slot >= data::SLOTS {
            return Err("Emplacement de macro inconnu.".into());
        }
        let profile = self.macro_profile()?;
        let map = self.macro_mapping()?;
        let affected: Vec<_> = map
            .iter()
            .enumerate()
            .filter(|(_, v)| **v & 0xfff == slot as u16 && Mode::from_code(**v >> 12).is_some())
            .map(|(i, _)| i)
            .collect();
        let previous = self.macro_record(slot)?;
        let mut expected = map.clone();
        for &i in &affected {
            expected[i] = 0;
        }
        let result = (|| {
            self.same_macro_profile(profile)?;
            for &i in &affected {
                self.same_macro_profile(profile)?;
                self.macro_bind(i, 0)?;
            }
            if self.macro_mapping()? != expected {
                return Err("Le retrait de la macro n’a pas été confirmé.".into());
            }
            self.same_macro_profile(profile)?;
            self.transport.send(&command(6, &[slot])?)?;
            let slots = self.read_macros()?;
            self.same_macro_profile(profile)?;
            Ok(slots)
        })();
        if let Err(error) = result {
            let restore = (|| -> Result<()> {
                self.same_macro_profile(profile)?;
                self.macro_send(4, &previous)?;
                for &i in &affected {
                    self.same_macro_profile(profile)?;
                    self.macro_bind(i, map[i])?;
                }
                if self.macro_record(slot)? != previous || self.macro_mapping()? != map {
                    return Err("relecture différente".into());
                }
                Ok(())
            })();
            return Err(if restore.is_ok() {
                format!("{error} Les réglages précédents ont été restaurés.")
            } else {
                format!(
                    "{error} Restauration non confirmée : rebranche le clavier et relis ses macros."
                )
            });
        }
        result
    }
}
