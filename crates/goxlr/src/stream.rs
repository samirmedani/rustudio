//! Compteurs de transport USB tenus par le pilote Thesycon de la GoXLR (TUSBAUDIO_QueryDriverStatistics).
//!
//! Releves sur le pilote 5.57 (dernier officiel) et une GoXLR Mini, le 29 octobre 2026. Le bloc de statistiques
//! (0x297FD0 octets) porte l'identite USB de l'appareil (VID en 0x14, PID en 0x18), puis un enregistrement par flux
//! isochrone : entree (GoXLR -> PC) en 0x1C0, sortie (PC -> GoXLR) en 0x278. Dans chacun :
//! - +0x28 : 1 a l'arret, 0 pendant que le son passe (le pilote coupe le flux quand aucune appli ne joue) ;
//! - +0x2C : millisecondes depuis le demarrage du flux ;
//! - +0x30 : micro-trames USB servies (8 000 par seconde) ;
//! - +0x34 : micro-trames perdues.
//! Chaque coupure entendue fait avancer ce dernier compteur de 8 (une trame USB d'1 ms) ou 16, dans les deux sens a
//! la fois : test en boucle du 29/10, 4 coupures sur 4 retrouvees a la milliseconde. A l'arret du flux d'entree, le
//! pilote y ajoute quelques micro-trames : seules comptent les avances pendant que le flux tourne.
use serde::Serialize;

pub const DRIVER_STATS_SIZE: usize = 0x297FD0;
const VID: u32 = 0x1220;
const PID_MINI: u32 = 0x8FE4;
const INPUT: usize = 0x1C0;
const OUTPUT: usize = 0x278;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct StreamCounters {
    pub active: bool,
    pub ms: u32,
    pub microframes: u32,
    pub lost: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct StreamStats {
    pub input: StreamCounters,
    pub output: StreamCounters,
}

impl StreamStats {
    pub fn active(&self) -> bool {
        self.input.active || self.output.active
    }
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// Lit le bloc du pilote ; None si ce n'est pas le format releve (autre appareil, autre pilote).
pub fn parse(b: &[u8]) -> Option<StreamStats> {
    if b.len() != DRIVER_STATS_SIZE || u32_at(b, 0x14)? != VID || u32_at(b, 0x18)? != PID_MINI {
        return None;
    }
    let rec = |base: usize| -> Option<StreamCounters> {
        let idle = u32_at(b, base + 0x28)?;
        if idle > 1 {
            return None;
        }
        Some(StreamCounters { active: idle == 0, ms: u32_at(b, base + 0x2C)?, microframes: u32_at(b, base + 0x30)?, lost: u32_at(b, base + 0x34)? })
    };
    Some(StreamStats { input: rec(INPUT)?, output: rec(OUTPUT)? })
}

/// Bloc au format du pilote (tests et fausse GoXLR).
pub fn fake_block(s: &StreamStats) -> Vec<u8> {
    let mut b = vec![0u8; DRIVER_STATS_SIZE];
    let mut put = |o: usize, v: u32| b[o..o + 4].copy_from_slice(&v.to_le_bytes());
    put(0x14, VID);
    put(0x18, PID_MINI);
    for (base, c) in [(INPUT, s.input), (OUTPUT, s.output)] {
        put(base, 2);
        put(base + 0x28, if c.active { 0 } else { 1 });
        put(base + 0x2C, c.ms);
        put(base + 0x30, c.microframes);
        put(base + 0x34, c.lost);
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lecture_du_bloc_du_pilote() {
        let s = StreamStats {
            input: StreamCounters { active: true, ms: 203_104, microframes: 189_042_316, lost: 3036 },
            output: StreamCounters { active: false, ms: 5, microframes: 40, lost: 1680 },
        };
        let b = fake_block(&s);
        assert_eq!(parse(&b), Some(s));
        assert!(parse(&b[..1000]).is_none(), "taille differente : autre pilote");
        let mut other = b.clone();
        other[0x18..0x1C].copy_from_slice(&0x8FE0u32.to_le_bytes());
        assert!(parse(&other).is_none(), "GoXLR pleine taille : format non releve");
        let mut odd = b;
        odd[INPUT + 0x28..INPUT + 0x2C].copy_from_slice(&7u32.to_le_bytes());
        assert!(parse(&odd).is_none(), "drapeau de marche inattendu");
    }
}
