//! Traitement de la voix en virgule fixe, partagé tel quel entre le firmware Rustudio de la GoXLR Mini
//! (ARM926 à 200 MHz, sans division matérielle ni flottant) et le PC (aperçu avant/après, tests).
//!
//! Contraintes du firmware, à garder : aucun `std`, aucune division, aucun indice qui puisse paniquer (le
//! firmware est lié sans gestion de panique), rien d'alloué.
//!
//! Chaîne : coupe-bas → noise gate → égaliseur 6 bandes → compresseur → de-esser → limiteur.
//! - échantillons : entiers signés, pleine échelle 24 bits (±2^23), marge interne jusqu'à ±2^27 (+24 dB) ;
//! - filtres : biquads en forme directe I, coefficients Q28, reste de l'arrondi réinjecté (bruit des graves
//!   très bas, même à 30 Hz) ;
//! - niveaux et gains en dB × 256 ; dynamique calculée par sous-blocs de 8 échantillons (6 kHz), gain
//!   interpolé échantillon par échantillon (aucun escalier audible) ;
//! - constantes de temps : coefficient Q30 d'un filtre à un pôle au rythme des sous-blocs.
//!
//! Le bloc de 32 échantillons passe en trois étapes (`STAGES`) que le firmware répartit sur trois tours de
//! boucle, pour laisser l'USB servi entre deux.

// Le firmware n'utilise qu'une partie de ce module (ni encodage ni traitement d'un bloc d'un coup).
#![allow(dead_code)]

pub const BLOCK: usize = 32;
pub const SUB: usize = 8;
pub const STAGES: u8 = 3;
/// Mots de 32 bits du réglage envoyé au firmware.
pub const WORDS: usize = 64;
pub const BYTES: usize = 4 * WORDS;
/// « MIC1 », premier mot du réglage.
pub const MAGIC: u32 = 0x3143_494D;

pub const FLAG_HPF: u32 = 1;
/// Bande n de l'égaliseur : bit 1 + n.
pub const FLAG_EQ0: u32 = 1 << 1;
pub const FLAG_GATE: u32 = 1 << 7;
pub const FLAG_COMP: u32 = 1 << 8;
pub const FLAG_DEESS: u32 = 1 << 9;
pub const FLAG_LIMIT: u32 = 1 << 10;

/// Pleine échelle et marge interne.
pub const FULL: i32 = (1 << 23) - 1;
const HEADROOM: i64 = (1 << 27) - 1;
/// Niveau d'un silence numérique (dB × 256).
pub const FLOOR_DB: i32 = -200 * 256;
/// Gain unité, Q24.
pub const UNITY: i32 = 1 << 24;

// Position des champs dans le réglage (mots).
const W_FLAGS: usize = 1;
const W_HPF: usize = 2;
const W_EQ: usize = 7;
const W_DEESS_HP: usize = 37;
const W_GATE: usize = 42;
const W_COMP: usize = 48;
const W_DEESS: usize = 53;
const W_LIMIT: usize = 59;

/// log2(1 + i/32), Q16.
const LOG2: [i32; 33] = [
    0, 2909, 5732, 8473, 11136, 13727, 16248, 18704, 21098, 23433, 25711, 27936, 30109, 32234, 34312, 36346, 38336,
    40286, 42196, 44068, 45904, 47705, 49472, 51207, 52911, 54584, 56229, 57845, 59434, 60997, 62534, 64047, 65536,
];
/// 2^(i/32), Q29.
const EXP2: [i32; 33] = [
    536870912, 548626854, 560640218, 572916640, 585461881, 598281827, 611382493, 624770026, 638450708, 652430958,
    666717336, 681316545, 696235434, 711481005, 727060411, 742980960, 759250125, 775875538, 792865000, 810226483,
    827968132, 846098274, 864625413, 883558244, 902905651, 922676710, 942880699, 963527098, 984625594, 1006186087,
    1028218693, 1050733751, 1073741824,
];

fn at(table: &[i32; 33], i: usize) -> i32 {
    table.get(i).copied().unwrap_or(0)
}

/// Niveau en dB × 256 d'une amplitude (pleine échelle 2^23 = 0 dB). Précision : 0,01 dB.
pub fn level_db(v: u32) -> i32 {
    if v == 0 {
        return FLOOR_DB;
    }
    let e = 31 - v.leading_zeros() as i32;
    let norm = if e >= 16 { v >> (e - 16) } else { v << (16 - e) };
    let frac = norm.wrapping_sub(1 << 16) & 0xFFFF;
    let i = (frac >> 11) as usize;
    let t = (frac & 0x7FF) as i32;
    let (a, b) = (at(&LOG2, i), at(&LOG2, i + 1));
    let log2 = ((e - 23) << 16) + a + (((b - a) * t) >> 11);
    // 20·log10(2) × 256 = 1541,27 ≈ 98642 / 64
    ((i64::from(log2) * 98642) >> 22) as i32
}

/// Gain linéaire Q24 d'un gain en dB × 256, de -150 à +30 dB.
pub fn gain_q24(db: i32) -> i32 {
    let db = db.clamp(-150 * 256, 30 * 256);
    // 1 / (20·log10(2) × 256) en Q16, ×4096
    let log2 = ((i64::from(db) * 174165) >> 12) as i32;
    let n = log2 >> 16;
    let frac = (log2 & 0xFFFF) as u32;
    let i = (frac >> 11) as usize;
    let t = (frac & 0x7FF) as i32;
    let (a, b) = (at(&EXP2, i), at(&EXP2, i + 1));
    let m = a + (((i64::from(b - a) * i64::from(t)) >> 11) as i32);
    let shift = 5 - n;
    if shift >= 31 {
        0
    } else if shift >= 0 {
        m >> shift
    } else {
        m << (-shift).min(1)
    }
}

/// Coefficients d'un biquad, Q28 : y = b0·x + b1·x1 + b2·x2 − a1·y1 − a2·y2.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Biquad {
    pub b0: i32,
    pub b1: i32,
    pub b2: i32,
    pub a1: i32,
    pub a2: i32,
}

#[derive(Clone, Copy, Default)]
struct Memory {
    x1: i32,
    x2: i32,
    y1: i32,
    y2: i32,
    rest: i32,
}

#[inline(always)]
fn biquad(c: &Biquad, m: &mut Memory, x: i32) -> i32 {
    let acc = i64::from(c.b0) * i64::from(x) + i64::from(c.b1) * i64::from(m.x1) + i64::from(c.b2) * i64::from(m.x2)
        - i64::from(c.a1) * i64::from(m.y1)
        - i64::from(c.a2) * i64::from(m.y2)
        + i64::from(m.rest);
    let y = (acc >> 28).clamp(-HEADROOM, HEADROOM) as i32;
    m.rest = (acc & ((1 << 28) - 1)) as i32;
    m.x2 = m.x1;
    m.x1 = x;
    m.y2 = m.y1;
    m.y1 = y;
    y
}

/// Réglage complet, tel que le moteur le calcule et l'envoie.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Params {
    pub flags: u32,
    pub hpf: Biquad,
    pub eq: [Biquad; 6],
    pub deess_hp: Biquad,
    /// seuil, hystérésis, profondeur (positive), en dB × 256 ; ouverture et fermeture Q30 ; maintien en
    /// sous-blocs
    pub gate_threshold: i32,
    pub gate_hysteresis: i32,
    pub gate_range: i32,
    pub gate_open: i32,
    pub gate_close: i32,
    pub gate_hold: i32,
    /// seuil (dB × 256), pente 1 − 1/ratio (Q16), gain de compensation (dB × 256), attaque et relâchement Q30
    pub comp_threshold: i32,
    pub comp_slope: i32,
    pub comp_makeup: i32,
    pub comp_attack: i32,
    pub comp_release: i32,
    /// réduction maximale (positive) et écart aigus/voix à partir duquel on réduit (dB × 256), aigus en
    /// dessous de ce niveau ignorés, attaque et relâchement Q30, réduction par dB d'excès (× 256)
    pub deess_max: i32,
    pub deess_offset: i32,
    pub deess_floor: i32,
    pub deess_attack: i32,
    pub deess_release: i32,
    pub deess_scale: i32,
    /// plafond (dB × 256) et relâchement Q30
    pub limit_ceiling: i32,
    pub limit_release: i32,
}

impl Params {
    pub fn active(&self) -> bool {
        self.flags != 0
    }

    /// Réglage à envoyer, dans l'ordre que lit `decode`.
    pub fn encode(&self) -> [u8; BYTES] {
        let bq = |b: &Biquad| [b.b0, b.b1, b.b2, b.a1, b.a2];
        let e = &self.eq;
        let fields = [MAGIC as i32, self.flags as i32]
            .into_iter()
            .chain(bq(&self.hpf))
            .chain(bq(&e[0]))
            .chain(bq(&e[1]))
            .chain(bq(&e[2]))
            .chain(bq(&e[3]))
            .chain(bq(&e[4]))
            .chain(bq(&e[5]))
            .chain(bq(&self.deess_hp))
            .chain([self.gate_threshold, self.gate_hysteresis, self.gate_range, self.gate_open, self.gate_close, self.gate_hold])
            .chain([self.comp_threshold, self.comp_slope, self.comp_makeup, self.comp_attack, self.comp_release])
            .chain([self.deess_max, self.deess_offset, self.deess_floor, self.deess_attack, self.deess_release, self.deess_scale])
            .chain([self.limit_ceiling, self.limit_release]);
        let mut bytes = [0u8; BYTES];
        for (slot, word) in bytes.chunks_exact_mut(4).zip(fields) {
            slot.copy_from_slice(&word.to_le_bytes());
        }
        bytes
    }

    /// Réglage reçu ; None si ce n'en est pas un (taille ou en-tête).
    pub fn decode(bytes: &[u8]) -> Option<Params> {
        if bytes.len() < BYTES {
            return None;
        }
        let word = |i: usize| -> i32 {
            match bytes.get(4 * i..4 * i + 4) {
                Some(&[a, b, c, d]) => i32::from_le_bytes([a, b, c, d]),
                _ => 0,
            }
        };
        if word(0) as u32 != MAGIC {
            return None;
        }
        let bq = |at: usize| Biquad { b0: word(at), b1: word(at + 1), b2: word(at + 2), a1: word(at + 3), a2: word(at + 4) };
        Some(Params {
            flags: word(W_FLAGS) as u32,
            hpf: bq(W_HPF),
            eq: [bq(W_EQ), bq(W_EQ + 5), bq(W_EQ + 10), bq(W_EQ + 15), bq(W_EQ + 20), bq(W_EQ + 25)],
            deess_hp: bq(W_DEESS_HP),
            gate_threshold: word(W_GATE),
            gate_hysteresis: word(W_GATE + 1),
            gate_range: word(W_GATE + 2),
            gate_open: word(W_GATE + 3),
            gate_close: word(W_GATE + 4),
            gate_hold: word(W_GATE + 5),
            comp_threshold: word(W_COMP),
            comp_slope: word(W_COMP + 1),
            comp_makeup: word(W_COMP + 2),
            comp_attack: word(W_COMP + 3),
            comp_release: word(W_COMP + 4),
            deess_max: word(W_DEESS),
            deess_offset: word(W_DEESS + 1),
            deess_floor: word(W_DEESS + 2),
            deess_attack: word(W_DEESS + 3),
            deess_release: word(W_DEESS + 4),
            deess_scale: word(W_DEESS + 5),
            limit_ceiling: word(W_LIMIT),
            limit_release: word(W_LIMIT + 1),
        })
    }
}

/// Un pas d'un filtre à un pôle : `now` va vers `target` de la fraction `k` (Q30).
#[inline(always)]
fn toward(now: i32, target: i32, k: i32) -> i32 {
    now + ((i64::from(target - now) * i64::from(k)) >> 30) as i32
}

/// Amplitude maximale d'un sous-bloc.
fn peak(samples: &[i32]) -> u32 {
    samples.iter().fold(0, |p, &x| p.max(x.unsigned_abs()))
}

/// Applique un gain qui passe de `from` à `to` (Q24) au fil du sous-bloc.
fn ramp(samples: &mut [i32], from: i32, to: i32) {
    let step = (to - from) >> 3;
    let mut gain = from;
    for x in samples.iter_mut() {
        gain += step;
        *x = ((i64::from(*x) * i64::from(gain)) >> 24).clamp(-HEADROOM, HEADROOM) as i32;
    }
}

/// État de la chaîne entre deux blocs. Réduction de gain de chaque étage en dB × 256 (≤ 0), et le gain
/// linéaire appliqué à la fin du sous-bloc précédent.
#[derive(Clone, Copy)]
pub struct Chain {
    hpf: Memory,
    eq: [Memory; 6],
    deess_hp: Memory,
    gate_open: bool,
    gate_hold: i32,
    gate_db: i32,
    gate_gain: i32,
    comp_db: i32,
    comp_gain: i32,
    deess_db: i32,
    deess_gain: i32,
    limit_db: i32,
    limit_gain: i32,
    /// réduction la plus forte depuis le dernier relevé (dB × 256, positive) : gate, compresseur, de-esser,
    /// limiteur
    pub reduction: [i32; 4],
}

impl Default for Chain {
    fn default() -> Self {
        Chain {
            hpf: Memory::default(),
            eq: [Memory::default(); 6],
            deess_hp: Memory::default(),
            gate_open: true,
            gate_hold: 0,
            gate_db: 0,
            gate_gain: UNITY,
            comp_db: 0,
            comp_gain: UNITY,
            deess_db: 0,
            deess_gain: UNITY,
            limit_db: 0,
            limit_gain: UNITY,
            reduction: [0; 4],
        }
    }
}

impl Chain {
    /// Une étape du bloc : 0 coupe-bas, gate et bandes 1 à 3 ; 1 bandes 4 à 6 et compresseur ; 2 de-esser et
    /// limiteur. Le bloc ressort à la pleine échelle 24 bits après l'étape 2.
    pub fn stage(&mut self, p: &Params, stage: u8, block: &mut [i32; BLOCK]) {
        self.stage_samples(p, stage, block);
        if stage >= 2 {
            for x in block { *x = (*x).clamp(-FULL, FULL); }
        }
    }

    fn stage_samples(&mut self, p: &Params, stage: u8, block: &mut [i32]) {
        match stage {
            0 => {
                if p.flags & FLAG_HPF != 0 {
                    for x in block.iter_mut() {
                        *x = biquad(&p.hpf, &mut self.hpf, *x);
                    }
                }
                if p.flags & FLAG_GATE != 0 {
                    self.gate(p, block);
                }
                self.bands(p, block, 0..3);
            }
            1 => {
                self.bands(p, block, 3..6);
                if p.flags & FLAG_COMP != 0 {
                    self.compressor(p, block);
                }
            }
            _ => {
                if p.flags & FLAG_DEESS != 0 {
                    self.deesser(p, block);
                }
                if p.flags & FLAG_LIMIT != 0 {
                    self.limiter(p, block);
                }
            }
        }
    }

    /// Les trois étapes d'un coup (PC).
    pub fn process(&mut self, p: &Params, block: &mut [i32; BLOCK]) {
        for stage in 0..STAGES {
            self.stage(p, stage, block);
        }
    }

    /// Une unité de contrôle, en gardant la marge pour le limiteur à anticipation du PC.
    pub fn process_quantum(&mut self, p: &Params, samples: &mut [i32; SUB]) {
        for stage in 0..STAGES {
            self.stage_samples(p, stage, samples);
        }
    }

    fn bands(&mut self, p: &Params, block: &mut [i32], bands: core::ops::Range<usize>) {
        for n in bands {
            if p.flags & (FLAG_EQ0 << n) == 0 {
                continue;
            }
            if let (Some(c), Some(m)) = (p.eq.get(n), self.eq.get_mut(n)) {
                for x in block.iter_mut() {
                    *x = biquad(c, m, *x);
                }
            }
        }
    }

    fn note(&mut self, stage: usize, db: i32) {
        if let Some(r) = self.reduction.get_mut(stage) {
            *r = (*r).max(-db);
        }
    }

    fn gate(&mut self, p: &Params, block: &mut [i32]) {
        for sub in block.chunks_exact_mut(SUB) {
            let level = level_db(peak(sub));
            if level >= p.gate_threshold {
                self.gate_open = true;
                self.gate_hold = p.gate_hold;
            } else if level < p.gate_threshold - p.gate_hysteresis {
                if self.gate_hold > 0 {
                    self.gate_hold -= 1;
                } else {
                    self.gate_open = false;
                }
            }
            let target = if self.gate_open { 0 } else { -p.gate_range };
            let k = if target > self.gate_db { p.gate_open } else { p.gate_close };
            self.gate_db = toward(self.gate_db, target, k);
            let gain = gain_q24(self.gate_db);
            ramp(sub, self.gate_gain, gain);
            self.gate_gain = gain;
            self.note(0, self.gate_db);
        }
    }

    fn compressor(&mut self, p: &Params, block: &mut [i32]) {
        // coude doux de 6 dB
        const HALF_KNEE: i32 = 3 * 256;
        for sub in block.chunks_exact_mut(SUB) {
            let over = level_db(peak(sub)) - p.comp_threshold;
            let excess = if over <= -HALF_KNEE {
                0
            } else if over >= HALF_KNEE {
                over
            } else {
                // (over + W/2)² / (2W), W = 6 dB = 1536 : 1 / 3072 ≈ 1365 / 2^22
                let t = i64::from(over + HALF_KNEE);
                ((t * t * 1365) >> 22) as i32
            };
            let target = -(((i64::from(excess) * i64::from(p.comp_slope)) >> 16) as i32);
            let k = if target < self.comp_db { p.comp_attack } else { p.comp_release };
            self.comp_db = toward(self.comp_db, target, k);
            let gain = gain_q24(self.comp_db + p.comp_makeup);
            ramp(sub, self.comp_gain, gain);
            self.comp_gain = gain;
            self.note(1, self.comp_db);
        }
    }

    fn deesser(&mut self, p: &Params, block: &mut [i32]) {
        for sub in block.chunks_exact_mut(SUB) {
            let mut high_peak = 0;
            for &sample in sub.iter() {
                high_peak = high_peak.max(biquad(&p.deess_hp, &mut self.deess_hp, sample).unsigned_abs());
            }
            let high = level_db(high_peak);
            let excess = high - level_db(peak(sub)) + p.deess_offset;
            let target = if excess > 0 && high > p.deess_floor {
                -(((i64::from(excess) * i64::from(p.deess_scale)) >> 8) as i32).min(p.deess_max)
            } else {
                0
            };
            let k = if target < self.deess_db { p.deess_attack } else { p.deess_release };
            self.deess_db = toward(self.deess_db, target, k);
            let gain = gain_q24(self.deess_db);
            // réduction de toute la voix, le temps du « s » : exacte, sans déphasage entre graves et aigus
            ramp(sub, self.deess_gain, gain);
            self.deess_gain = gain;
            self.note(2, self.deess_db);
        }
    }

    fn limiter(&mut self, p: &Params, block: &mut [i32]) {
        for sub in block.chunks_exact_mut(SUB) {
            let over = level_db(peak(sub)) - p.limit_ceiling;
            let target = if over > 0 { -over } else { 0 };
            if target < self.limit_db {
                // attaque immédiate sur tout le sous-bloc : la crête ne passe jamais
                self.limit_db = target;
                let gain = gain_q24(target);
                ramp(sub, gain, gain);
                self.limit_gain = gain;
            } else {
                self.limit_db = toward(self.limit_db, target, p.limit_release);
                let gain = gain_q24(self.limit_db);
                ramp(sub, self.limit_gain, gain);
                self.limit_gain = gain;
            }
            self.note(3, self.limit_db);
        }
    }
}
