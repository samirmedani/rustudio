//! Profils sur mesure : à partir de l'empreinte vocale et du micro, réglages de la GoXLR Mini pour chaque
//! préréglage (égaliseur 6 bandes, fréquences comprises, noise gate, compresseur, de-esser) et leurs explications.
//!
//! Principe. La voix mesurée (spectre moyen de la parole, 1/6 d'octave) est comparée à un spectre de parole de
//! référence (ANSI S3.5-1997, effort vocal normal), adapté à la fondamentale de la voix : sous la voix, tout ce qui
//! reste (souffle, chocs, ventilation, ronflement) doit disparaître. Chaque profil ajoute sa couleur (chaleur,
//! médiums, présence, air) et tolère un écart : seul ce qui sort de la tolérance est corrigé, en partie, pour garder
//! le timbre de la voix. Là où la mesure est peu sûre (bruit de la pièce trop proche), la courbe publiée du micro
//! prend le relais. La courbe visée est enfin reproduite au mieux par les six bandes réelles de la Mini (forme et
//! largeur mesurées sur l'appareil), fréquences comprises.
use crate::consts::{eq_default_freq, eq_range, COMP_ATTACK_MS, COMP_RELEASE_MS, EQ_BANDS, RATIOS};
use crate::dsp::{self, Comp, Dsp, Fx, Gate, GATE_SOFT_DB, RESIDUAL_NOISE_DB};
use crate::mics::{self, MicModel};
use crate::state::{MicCal, State};
use crate::voice::{self, VoicePrint};
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::OnceLock;

const FS: f64 = 48000.0;
const GAINS: usize = 19; // -9..=+9 dB

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// cloche (RBJ)
    Peak,
    /// plateau grave (RBJ du second ordre, forme Q), coude à la fréquence réglée
    LowShelf,
    /// plateau aigu (RBJ, forme Q)
    HighShelf,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BandModel {
    pub band: &'static str,
    pub kind: Kind,
    pub q: f64,
}

/// Les six bandes de la Mini telles que mesurées sur l'appareil le 31/10/2026 : filtres RBJ centrés sur la
/// fréquence demandée (à ±3 %, réglages graves compris), coupes symétriques des montées, gain linéaire.
/// Voir docs/VOIX.md (« L'égaliseur de la Mini, mesuré »).
pub const MINI_EQ: [BandModel; 6] = [
    BandModel { band: "90", kind: Kind::LowShelf, q: 0.94 },
    BandModel { band: "250", kind: Kind::Peak, q: 1.38 },
    BandModel { band: "500", kind: Kind::Peak, q: 1.32 },
    BandModel { band: "1000", kind: Kind::Peak, q: 1.48 },
    BandModel { band: "3000", kind: Kind::Peak, q: 1.35 },
    BandModel { band: "8000", kind: Kind::HighShelf, q: 0.96 },
];

fn mag_db(c: &[f64; 6], f: f64) -> f64 {
    let w = -2.0 * std::f64::consts::PI * f / FS;
    let (z1r, z1i) = (w.cos(), w.sin());
    let (z2r, z2i) = ((2.0 * w).cos(), (2.0 * w).sin());
    let (nr, ni) = (c[0] + c[1] * z1r + c[2] * z2r, c[1] * z1i + c[2] * z2i);
    let (dr, di) = (c[3] + c[4] * z1r + c[5] * z2r, c[4] * z1i + c[5] * z2i);
    10.0 * ((nr * nr + ni * ni) / (dr * dr + di * di)).max(1e-24).log10()
}

impl BandModel {
    /// Réponse (dB) de la bande réglée à f0 et gain, au point x.
    pub fn db(&self, f0: f64, gain: f64, x: f64) -> f64 {
        if gain == 0.0 {
            return 0.0;
        }
        let kind = match self.kind {
            Kind::LowShelf => "LSC",
            Kind::Peak => "PK",
            Kind::HighShelf => "HSC",
        };
        mag_db(&crate::hpeq::biquad(kind, f0, gain, self.q, FS), x)
    }
}

/// Réponse (dB) d'une bande de la Mini réglée à f0 et gain, à la fréquence f.
pub fn band_db(m: &BandModel, f0: f64, gain: f64, f: f64) -> f64 {
    m.db(f0, gain, f)
}

/// Courbe totale de l'égaliseur micro (dB) aux fréquences données.
pub fn eq_curve(d: &Dsp, freqs: &[f64]) -> Vec<f64> {
    let bands: Vec<(&BandModel, f64, f64)> = MINI_EQ
        .iter()
        .map(|m| {
            let g = *d.eq.get(m.band).unwrap_or(&0) as f64;
            let f0 = d.eq_freq.get(m.band).copied().unwrap_or_else(|| eq_default_freq(m.band));
            (m, f0, g)
        })
        .filter(|(_, _, g)| *g != 0.0)
        .collect();
    freqs.iter().map(|&f| bands.iter().map(|(m, f0, g)| m.db(*f0, *g, f)).sum()).collect()
}

// ---------- candidats de l'optimisation (calculés une fois) ----------

struct Cands {
    /// fréquences demandées possibles (1/24 d'octave dans la plage de la bande)
    freqs: Vec<f64>,
    /// resp[(fi * GAINS + gi) * n + b] : réponse de la bande à la fréquence candidate fi, gain gi - 9, point b
    resp: Vec<f32>,
}

fn grid() -> &'static Vec<f64> {
    static G: OnceLock<Vec<f64>> = OnceLock::new();
    G.get_or_init(voice::band_centers)
}

fn cands() -> &'static Vec<Cands> {
    static C: OnceLock<Vec<Cands>> = OnceLock::new();
    C.get_or_init(|| {
        let grid = grid();
        let n = grid.len();
        MINI_EQ
            .iter()
            .map(|m| {
                let (lo, hi) = eq_range(m.band).unwrap_or((20.0, 20000.0));
                let steps = ((hi / lo).log2() * 24.0).ceil().max(1.0) as usize;
                let mut freqs: Vec<f64> = (0..=steps).map(|i| (lo * (hi / lo).powf(i as f64 / steps as f64)).round()).collect();
                freqs.push(eq_default_freq(m.band));
                freqs.sort_by(|a, b| a.partial_cmp(b).unwrap());
                freqs.dedup();
                let mut resp = vec![0f32; freqs.len() * GAINS * n];
                for (fi, &f0) in freqs.iter().enumerate() {
                    for gi in 0..GAINS {
                        let g = gi as f64 - 9.0;
                        if g == 0.0 {
                            continue;
                        }
                        for (b, &f) in grid.iter().enumerate() {
                            resp[(fi * GAINS + gi) * n + b] = m.db(f0, g, f) as f32;
                        }
                    }
                }
                Cands { freqs, resp }
            })
            .collect()
    })
}

fn fit_cost(target: &[f64], weight: &[f64], sum: &[f64]) -> f64 {
    target.iter().zip(weight).zip(sum).map(|((t, w), s)| w * (t - s) * (t - s)).sum()
}

/// Meilleurs réglages (fréquence, gain entier) des six bandes pour approcher la courbe visée sur la grille
/// d'analyse : descente coordonnée sur toutes les fréquences permises (1/24 d'octave) et tous les gains.
/// Jamais de remontée sous no_boost_below (Hz) ni avec la bande la plus grave : sous la voix, on ne fait que couper.
/// Une bande qui n'apporte presque rien (±1 dB) est remise à plat.
pub fn fit_bands(target: &[f64], weight: &[f64], no_boost_below: f64) -> Vec<(f64, i32)> {
    let cs = cands();
    let n = grid().len();
    assert_eq!(target.len(), n);
    let mut choice: Vec<(usize, usize)> = cs
        .iter()
        .enumerate()
        .map(|(i, c)| (c.freqs.iter().position(|f| *f == eq_default_freq(MINI_EQ[i].band)).unwrap_or(0), 9))
        .collect();
    let mut sum = vec![0f64; n];
    let add = |sum: &mut Vec<f64>, c: &Cands, fi: usize, gi: usize, sign: f64| {
        let base = (fi * GAINS + gi) * n;
        for b in 0..n {
            sum[b] += sign * c.resp[base + b] as f64;
        }
    };
    for _ in 0..6 {
        let mut changed = false;
        for i in 0..cs.len() {
            let (fi, gi) = choice[i];
            add(&mut sum, &cs[i], fi, gi, -1.0);
            let mut best = (f64::MAX, fi, gi);
            for fj in 0..cs[i].freqs.len() {
                for gj in 0..GAINS {
                    if gj == 9 && fj != fi {
                        continue; // gain nul : la fréquence n'a pas d'effet
                    }
                    if gj > 9 && (i == 0 || cs[i].freqs[fj] < no_boost_below) {
                        continue;
                    }
                    let g = gj as f64 - 9.0;
                    let mut cost = 0.03 * g * g;
                    let base = (fj * GAINS + gj) * n;
                    for b in 0..n {
                        let e = target[b] - sum[b] - cs[i].resp[base + b] as f64;
                        cost += weight[b] * e * e;
                    }
                    if cost < best.0 - 1e-9 {
                        best = (cost, fj, gj);
                    }
                }
            }
            if (best.1, best.2) != (fi, gi) {
                changed = true;
            }
            choice[i] = (best.1, best.2);
            add(&mut sum, &cs[i], best.1, best.2, 1.0);
        }
        if !changed {
            break;
        }
    }
    // élagage : une bande à ±1 dB qui ne rapproche presque pas de la cible est remise à plat
    let total: f64 = weight.iter().sum::<f64>().max(1e-9);
    for i in 0..cs.len() {
        let (fi, gi) = choice[i];
        if gi == 9 || (gi as i32 - 9).abs() > 1 {
            continue;
        }
        let with = fit_cost(target, weight, &sum);
        add(&mut sum, &cs[i], fi, gi, -1.0);
        let without = fit_cost(target, weight, &sum);
        if ((without - with) / total).sqrt() < 0.35 {
            choice[i] = (fi, 9);
        } else {
            add(&mut sum, &cs[i], fi, gi, 1.0);
        }
    }
    choice
        .iter()
        .enumerate()
        .map(|(i, (fi, gi))| {
            let g = *gi as i32 - 9;
            (if g == 0 { eq_default_freq(MINI_EQ[i].band) } else { cs[i].freqs[*fi] }, g)
        })
        .collect()
}

// ---------- référence et profils ----------

/// Spectre de parole de référence, effort normal (ANSI S3.5-1997, niveaux spectraux par tiers d'octave).
const SII: [(f64, f64); 18] = [
    (160.0, 32.41), (200.0, 34.48), (250.0, 34.75), (315.0, 33.98), (400.0, 34.59), (500.0, 34.27), (630.0, 32.06),
    (800.0, 28.30), (1000.0, 25.01), (1250.0, 23.00), (1600.0, 20.15), (2000.0, 17.32), (2500.0, 13.18),
    (3150.0, 11.55), (4000.0, 9.33), (5000.0, 5.31), (6300.0, 2.59), (8000.0, 1.13),
];

fn interp_log(pts: &[(f64, f64)], f: f64) -> f64 {
    if f <= pts[0].0 {
        return pts[0].1;
    }
    for w in pts.windows(2) {
        let ((f0, a), (f1, b)) = (w[0], w[1]);
        if f <= f1 {
            let t = (f / f0).ln() / (f1 / f0).ln();
            return a + t * (b - a);
        }
    }
    pts[pts.len() - 1].1
}

/// Niveau d'une bande de largeur proportionnelle (dB), relatif à la bande de 1 kHz : 160 Hz à 16 kHz.
fn sii_band(f: f64) -> f64 {
    let f = f.max(160.0);
    let lvl = |x: f64| {
        let spec = if x > 8000.0 { 1.13 - 4.0 * (x / 8000.0).log2() - 10.0 * (x / 8000.0).log10() } else { interp_log(&SII, x) };
        spec + 10.0 * x.log10()
    };
    lvl(f) - lvl(1000.0)
}

/// Ce qu'un bon micro de près ajoute à une voix entendue à 1 m : effet de proximité dans les graves (micro
/// cardioïde à 10–20 cm) et présence de la bouche dans l'axe. Le micro de casque, plus loin du centre, moins.
const PROXIMITY: [(f64, f64); 5] = [(125.0, 8.0), (200.0, 5.0), (315.0, 2.0), (500.0, 0.5), (800.0, 0.0)];
const PRESENCE: [(f64, f64); 8] =
    [(1500.0, 0.0), (2500.0, 2.0), (4000.0, 4.0), (6300.0, 4.0), (8000.0, 3.0), (10000.0, 2.0), (12500.0, 0.0), (16000.0, -2.0)];

/// Voix « neutre » de près à la fréquence f : référence ANSI, proximité et présence d'une bonne prise de près,
/// et rien sous la fondamentale (floor_hz).
fn neutral(f: f64, floor_hz: f64, close: f64) -> f64 {
    let n = sii_band(f) + close * interp_log(&PROXIMITY, f) + interp_log(&PRESENCE, f);
    if f < floor_hz {
        n - 18.0 * (floor_hz / f).log2()
    } else {
        n
    }
}

/// Un profil : sa couleur, sa tolérance, et la conduite des autres traitements.
#[derive(Clone, Copy, Debug)]
pub struct Profile {
    pub key: &'static str,
    /// part de l'écart corrigée (0..1)
    pub alpha: f64,
    /// écart toléré sans correction (dB)
    pub window: f64,
    /// couleur : chaleur, médiums, présence, air (dB)
    pub warmth: f64,
    pub mud: f64,
    pub presence: f64,
    pub air: f64,
    /// coupe sous la voix : multiplicateur de sa fondamentale basse
    pub floor: f64,
    /// compresseur : dynamique visée (dB), ratio max (index), seuil sous ta voix moyenne (dB), attaque et
    /// relâchement (index), part du gain perdu rendue
    pub comp: (f64, i32, f64, i32, i32, f64),
    /// noise gate : décalage du seuil (dB), réduction max (dB, None : coupure complète), relâchement (index)
    pub gate: (f64, Option<f64>, i32),
    /// de-esser : multiplicateur
    pub deess: f64,
}

pub const PROFILES: [Profile; 6] = [
    Profile { key: "naturel", alpha: 0.45, window: 3.5, warmth: 0.0, mud: 0.0, presence: 0.0, air: 0.0, floor: 0.8,
              comp: (14.0, 4, 2.0, 9, 13, 0.5), gate: (-4.0, Some(15.0), 20), deess: 0.6 },
    Profile { key: "studio", alpha: 0.55, window: 2.5, warmth: 0.5, mud: -1.0, presence: 1.0, air: 1.0, floor: 0.85,
              comp: (11.0, 6, 3.0, 7, 12, 0.6), gate: (0.0, Some(20.0), 20), deess: 0.9 },
    Profile { key: "radio", alpha: 0.7, window: 1.5, warmth: 2.5, mud: -2.0, presence: 2.5, air: 1.5, floor: 0.85,
              comp: (7.0, 9, 6.0, 4, 11, 0.8), gate: (0.0, Some(24.0), 19), deess: 1.2 },
    Profile { key: "stream", alpha: 0.65, window: 2.0, warmth: 0.5, mud: -1.5, presence: 3.0, air: 1.5, floor: 0.95,
              comp: (8.0, 8, 5.0, 3, 10, 0.75), gate: (1.0, Some(24.0), 18), deess: 1.0 },
    Profile { key: "podcast", alpha: 0.6, window: 2.0, warmth: 2.0, mud: -1.0, presence: 1.0, air: -0.5, floor: 0.85,
              comp: (9.0, 7, 4.0, 6, 12, 0.7), gate: (-2.0, Some(18.0), 21), deess: 1.1 },
    Profile { key: "anti-bruit", alpha: 0.55, window: 2.5, warmth: -1.0, mud: -1.0, presence: 2.0, air: 0.0, floor: 1.15,
              comp: (9.0, 8, 4.0, 3, 10, 0.7), gate: (3.0, None, 15), deess: 1.0 },
];

pub fn profile(key: &str) -> Option<&'static Profile> {
    PROFILES.iter().find(|p| p.key == key)
}

fn bump(f: f64, fc: f64, sigma: f64, g: f64) -> f64 {
    if g == 0.0 {
        return 0.0;
    }
    let d = (f / fc).log2() / sigma;
    g * (-0.5 * d * d).exp()
}

fn warmth_hz(f0: f64) -> f64 {
    (1.5 * f0).clamp(110.0, 300.0)
}

/// Couleur du profil (dB) à la fréquence f.
fn flavor(p: &Profile, f: f64, f0: f64) -> f64 {
    let air = if f >= 11000.0 { p.air } else { bump(f, 11000.0, 0.6, p.air) };
    bump(f, warmth_hz(f0), 0.6, p.warmth) + bump(f, 380.0, 0.7, p.mud) + bump(f, 3500.0, 0.7, p.presence) + air
}

fn dz(x: f64, w: f64) -> f64 {
    x.signum() * (x.abs() - w).max(0.0)
}

/// Moyenne en puissance (dB) des valeurs dont la fréquence est dans [lo, hi].
fn pmean(freqs: &[f64], v: &[f64], lo: f64, hi: f64) -> f64 {
    let sel: Vec<f64> = freqs.iter().zip(v).filter(|(f, _)| (lo..=hi).contains(*f)).map(|(_, x)| 10f64.powf(x / 10.0)).collect();
    if sel.is_empty() {
        return 0.0;
    }
    10.0 * (sel.iter().sum::<f64>() / sel.len() as f64).max(1e-30).log10()
}

fn smooth3(v: &[f64]) -> Vec<f64> {
    let n = v.len();
    (0..n)
        .map(|i| {
            let a = v[i.saturating_sub(1)];
            let c = v[(i + 1).min(n - 1)];
            0.25 * a + 0.5 * v[i] + 0.25 * c
        })
        .collect()
}

/// Ramène une courbe mesurée sur d'autres bandes sur la grille d'analyse.
fn on_grid(bands: &[f64], vals: &[f64]) -> Vec<f64> {
    let g = grid();
    if bands.len() == g.len() && bands.iter().zip(g.iter()).all(|(a, b)| (a / b - 1.0).abs() < 1e-3) {
        return vals.to_vec();
    }
    let pts: Vec<(f64, f64)> = bands.iter().copied().zip(vals.iter().copied()).filter(|(f, _)| *f > 0.0).collect();
    if pts.is_empty() {
        return vec![voice::FLOOR_DB; g.len()];
    }
    g.iter().map(|&f| interp_log(&pts, f)).collect()
}

fn fmt_hz(f: f64) -> String {
    if f < 1000.0 {
        format!("{} Hz", f.round())
    } else {
        let k = (f / 100.0).round() / 10.0;
        let s = if (k - k.round()).abs() < 1e-9 { format!("{}", k.round()) } else { format!("{k:.1}") };
        format!("{} kHz", s.replace('.', ","))
    }
}

fn fmt_db(g: f64) -> String {
    let v = g.round() as i64;
    if v > 0 {
        format!("+{v} dB")
    } else if v < 0 {
        format!("\u{2212}{} dB", -v)
    } else {
        "0 dB".into()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Note {
    pub lvl: &'static str,
    pub t: String,
}
fn note(lvl: &'static str, t: impl Into<String>) -> Note {
    Note { lvl, t: t.into() }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Chart {
    pub bands: Vec<f64>,
    pub voice: Vec<f64>,
    pub after: Vec<f64>,
    pub noise: Vec<f64>,
    pub target_lo: Vec<f64>,
    pub target_hi: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fitted {
    pub dsp: Dsp,
    pub fx: Fx,
    pub notes: Vec<Note>,
    pub chart: Option<Chart>,
}

fn r1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// Ce que l'égaliseur doit faire, bande d'analyse par bande d'analyse, et pourquoi.
struct Plan {
    target: Vec<f64>,
    weight: Vec<f64>,
    /// part corrective (écart de ta voix) et part « couleur du profil », pour expliquer chaque bande
    corr: Vec<f64>,
    color: Vec<f64>,
    prior: Vec<f64>,
    floor_hz: f64,
    voice_rel: Vec<f64>,
    noise_rel: Vec<f64>,
    lo: Vec<f64>,
    hi: Vec<f64>,
}

fn plan(p: &Profile, v: &VoicePrint, model: Option<&MicModel>) -> Plan {
    let g = grid();
    let total_db = on_grid(&v.bands, &v.voice_db);
    let noise_db = on_grid(&v.bands, &v.noise_db);
    // les sifflantes relèvent le spectre moyen vers 4–10 kHz : c'est le travail du de-esser, pas de l'égaliseur ;
    // l'égaliseur se règle sur la voix sans elles
    let r = v.sibilance.ratio;
    let voice_db: Vec<f64> = match (&v.sibilant_db, (0.0..0.6).contains(&r)) {
        (Some(s), true) if r > 0.0 => {
            let sib = on_grid(&v.bands, s);
            total_db
                .iter()
                .zip(&sib)
                .map(|(t, s)| {
                    let p = 10f64.powf(t / 10.0) - r * 10f64.powf(s / 10.0);
                    if p > 0.0 { 10.0 * (p / (1.0 - r)).log10() } else { *t - 10.0 }
                })
                .collect()
        }
        _ => total_db.clone(),
    };
    let close = match model.map(|m| m.kind) {
        Some("jack") => 0.5,
        _ => 1.0,
    };
    let f0 = v.f0_hz.unwrap_or(120.0).clamp(60.0, 400.0);
    let f0_low = v.f0_low_hz.unwrap_or(0.85 * f0).clamp(50.0, f0);
    let floor_hz = (p.floor * f0_low).clamp(55.0, 220.0);
    let anchor_v = pmean(g, &voice_db, 800.0, 2500.0);
    let neutral_raw: Vec<f64> = g.iter().map(|&f| neutral(f, floor_hz, close)).collect();
    let anchor_n = pmean(g, &neutral_raw, 800.0, 2500.0);
    let voice_rel: Vec<f64> = voice_db.iter().map(|x| x - anchor_v).collect();
    let noise_rel: Vec<f64> = noise_db.iter().map(|x| x - anchor_v).collect();
    let neutral_rel: Vec<f64> = neutral_raw.iter().map(|x| x - anchor_n).collect();
    let dev = smooth3(&voice_rel.iter().zip(&neutral_rel).map(|(a, b)| a - b).collect::<Vec<_>>());
    let resp: Vec<f64> = g.iter().map(|&f| model.map(|m| mics::response_db(m, f)).unwrap_or(0.0)).collect();
    let resp_mid = if model.map(|m| m.response.is_empty()).unwrap_or(true) { 0.0 } else { pmean(g, &resp, 800.0, 2500.0) };
    let mut corr = vec![0.0; g.len()];
    let mut color = vec![0.0; g.len()];
    let mut prior = vec![0.0; g.len()];
    let mut target = vec![0.0; g.len()];
    let mut weight = vec![0.0; g.len()];
    let (mut lo, mut hi) = (vec![0.0; g.len()], vec![0.0; g.len()]);
    let sib = v.sibilance.excess_db;
    for (i, &f) in g.iter().enumerate() {
        let snr = voice_db[i] - noise_db[i];
        let p_dev = resp[i] - resp_mid;
        color[i] = flavor(p, f, f0);
        if model.map(|m| m.bright).unwrap_or(false) {
            color[i] += bump(f, 11000.0, 0.5, -1.5); // micro brillant : ses aigus se calment toujours un peu
        }
        if f < floor_hz {
            // sous la voix : tout ce qui reste est à enlever (jamais de remontée)
            corr[i] = -p.alpha.max(0.75) * dev[i].max(0.0);
        } else {
            let trust = ((snr - 6.0) / 12.0).clamp(0.0, 1.0) * if voice_rel[i] > -45.0 { 1.0 } else { 0.3 };
            // on remonte peu (3 dB au plus) et seulement là où la voix est bien présente : sinon on monterait le souffle
            let measured = (-p.alpha * dz(dev[i], p.window)).clamp(-6.0, 3.0);
            let from_mic = -p.alpha * dz(p_dev, p.window * 0.5);
            corr[i] = trust * measured + (1.0 - trust) * from_mic;
            prior[i] = (1.0 - trust) * from_mic;
        }
        let mut c = corr[i] + color[i];
        if f >= floor_hz && (snr < 10.0 || voice_rel[i] < -30.0) {
            c = c.min(0.0); // ne jamais monter le bruit de la pièce, ni une zone où ta voix n'a presque rien
        }
        if f > 12000.0 {
            c = c.min(2.0); // l'air se dose : au-delà, ce serait du souffle
        }
        if f < floor_hz && noise_rel[i] > -40.0 {
            c = c.min(-(noise_rel[i] + 40.0) * 0.4); // sous la voix : ronflement, ventilation, on creuse
        } else if f < 1.4 * floor_hz && noise_rel[i] > -30.0 {
            c = c.min(-((noise_rel[i] + 30.0) * 0.15).min(2.0)); // fondamentale : à peine, pour garder la voix
        }
        if (4500.0..=10500.0).contains(&f) && sib >= 14.0 && c > 0.0 {
            c *= if sib >= 19.0 { 0.3 } else { 0.6 }; // sifflantes déjà fortes : pas d'éclat en plus
        }
        target[i] = c.clamp(-9.0, 6.0);
        lo[i] = neutral_rel[i] + color[i] - p.window;
        hi[i] = neutral_rel[i] + color[i] + p.window;
        let region = if f < floor_hz {
            0.7
        } else if f < 300.0 {
            1.0
        } else if f <= 5000.0 {
            1.5
        } else if f <= 12000.0 {
            1.0
        } else {
            0.4
        };
        weight[i] = region * if voice_rel[i] > -40.0 || f < floor_hz { 1.0 } else { 0.4 };
    }
    Plan { target: smooth3(&target), weight, corr, color, prior, floor_hz, voice_rel, noise_rel, lo, hi }
}

fn at(freqs: &[f64], v: &[f64], f: f64) -> f64 {
    let pts: Vec<(f64, f64)> = freqs.iter().copied().zip(v.iter().copied()).collect();
    interp_log(&pts, f)
}

/// Pourquoi une bande a été réglée ainsi.
/// Raison d'un réglage de bande, en clair.
fn band_reason(f: f64, gain: i32, pl: &Plan, label: &str, model: Option<&MicModel>) -> String {
    let g = grid();
    let (corr, color, prior) = (at(g, &pl.corr, f), at(g, &pl.color, f), at(g, &pl.prior, f));
    let up = gain > 0;
    let by_color = (up && color > corr.max(0.0)) || (!up && color < corr.min(0.0));
    let mic_name = model.filter(|m| !m.generic).map(|m| m.name).unwrap_or("micro");
    if f < pl.floor_hz * 1.15 && !up {
        "grondement sous ta voix (souffle, chocs, ventilation)".to_string()
    } else if f < 320.0 {
        match (up, by_color) {
            (false, false) => "graves gonflés (effet de proximité)".into(),
            (false, true) => format!("graves plus légers pour « {label} »"),
            (true, false) => "graves un peu maigres".into(),
            (true, true) => format!("chaleur « {label} »"),
        }
    } else if f < 900.0 {
        match (up, by_color) {
            (false, false) => "médiums chargés (son « carton »)".into(),
            (false, true) => format!("médiums allégés pour « {label} »"),
            (true, _) => "corps de la voix".into(),
        }
    } else if f < 2500.0 {
        if up { "médiums en retrait".into() } else { "médiums nasillards".into() }
    } else if f < 6000.0 {
        match (up, by_color) {
            (true, true) => "clarté et présence".into(),
            (true, false) => "voix un peu en retrait".into(),
            (false, _) => "présence agressive".into(),
        }
    } else if !up && prior < -0.5 && model.map(|m| m.bright).unwrap_or(false) {
        format!("brillance du {mic_name} vers {}", fmt_hz(f))
    } else if !up {
        if model.map(|m| m.bright).unwrap_or(false) { format!("aigus brillants du {mic_name}") } else { "aigus durs".into() }
    } else {
        "air et détail".into()
    }
}

/// Explications de l'égaliseur : une ligne par raison, les bandes voisines qui corrigent la même chose réunies
/// (« −4 dB à 5 kHz et −2 dB à 5,6 kHz : présence agressive. »).
fn band_notes(bands: &[(f64, i32)], pl: &Plan, label: &str, model: Option<&MicModel>) -> Vec<String> {
    let mut groups: Vec<(Vec<String>, String)> = vec![];
    for (f, gn) in bands.iter().filter(|(_, g)| *g != 0) {
        let why = band_reason(*f, *gn, pl, label, model);
        let head = format!("{} à {}", fmt_db(*gn as f64), fmt_hz(*f));
        match groups.last_mut() {
            Some((heads, w)) if *w == why => heads.push(head),
            _ => groups.push((vec![head], why)),
        }
    }
    groups
        .into_iter()
        .map(|(heads, why)| {
            let (last, first) = heads.split_last().unwrap();
            let all = if first.is_empty() { last.clone() } else { format!("{} et {last}", first.join(", ")) };
            format!("{all} : {why}.")
        })
        .collect()
}

fn ratio_idx_for(need: f64, max_idx: i32) -> i32 {
    let mut idx = 3;
    for (i, r) in RATIOS.iter().enumerate() {
        if (i as i32) <= max_idx && *r <= need + 1e-9 {
            idx = idx.max(i as i32);
        }
    }
    idx.min(max_idx).max(3)
}

fn ratio_txt(idx: i32) -> String {
    let r = RATIOS[idx.clamp(0, 14) as usize];
    let s = if r >= 10.0 { format!("{r:.0}") } else { format!("{r:.1}").trim_end_matches(".0").to_string() };
    format!("{}:1", s.replace('.', ","))
}

/// Seuils pour dire les sifflantes (dépassement mesuré par voice::analyze).
pub fn sibilance_label(excess_db: f64) -> &'static str {
    if excess_db < 8.0 {
        "faibles"
    } else if excess_db < 14.0 {
        "normales"
    } else if excess_db < 19.0 {
        "marquées"
    } else {
        "très marquées"
    }
}

pub fn room_label(rt60_ms: Option<f64>) -> Option<&'static str> {
    rt60_ms.map(|r| if r < 200.0 { "sèche" } else if r <= 400.0 { "un peu réverbérante" } else { "réverbérante" })
}

pub fn voice_type(f0: Option<f64>) -> Option<&'static str> {
    f0.map(|f| if f < 135.0 { "grave" } else if f < 185.0 { "médium" } else { "aiguë" })
}

fn deesser_amount(p: &Profile, v: Option<&VoicePrint>, model: Option<&MicModel>, hf_boost: f64) -> i32 {
    let base = match v {
        Some(v) => {
            let x = v.sibilance.excess_db;
            // dépassement des sifflantes (voice::analyze) : 2–8 dB faibles, 13–16 dB voix courante, 19 dB et plus très marquées
            let pts = [(6.0, 5.0), (11.0, 18.0), (16.0, 30.0), (20.0, 42.0), (25.0, 55.0)];
            let mut b = if x <= 6.0 { 5.0 } else if x >= 25.0 { 55.0 } else {
                let mut y = 5.0;
                for w in pts.windows(2) {
                    let ((x0, y0), (x1, y1)) = (w[0], w[1]);
                    if x <= x1 {
                        y = y0 + (x - x0) / (x1 - x0) * (y1 - y0);
                        break;
                    }
                }
                y
            };
            if v.sibilance.ratio < 0.02 {
                b *= 0.6;
            }
            b
        }
        None => 25.0,
    };
    let bright = if model.map(|m| m.bright).unwrap_or(false) { 6.0 } else { 0.0 };
    ((base + bright + 3.0 * hf_boost.max(0.0)) * p.deess).round().clamp(0.0, 70.0) as i32
}

/// Réglages complets d'un profil calé sur l'empreinte vocale et le micro.
pub fn fit_with_print(p: &Profile, v: &VoicePrint, cal: Option<&MicCal>, gain_db: i32, model: Option<&MicModel>) -> Fitted {
    let label = dsp::preset(p.key).map(|x| x.label).unwrap_or(p.key);
    let pl = plan(p, v, model);
    let bands = fit_bands(&pl.target, &pl.weight, pl.floor_hz * 1.1);
    let mut d = dsp::preset_dsp(p.key);
    for ((b, _, _), (f, g)) in EQ_BANDS.iter().zip(&bands) {
        d.eq.insert(b.to_string(), *g);
        d.eq_freq.insert(b.to_string(), dsp::eq_freq_in_range(b, *f));
    }
    let g = grid();
    let eqc = eq_curve(&d, g);
    let hf_boost = g.iter().zip(&eqc).filter(|(f, _)| (4500.0..=10500.0).contains(*f)).map(|(_, x)| *x).fold(0.0, f64::max);
    d.deesser = deesser_amount(p, Some(v), model, hf_boost);
    // compresseur et noise gate : niveaux de l'entrée (mesure du moteur) au gain actuel
    let (cal_noise, cal_low, cal_med, cal_peak, cal_loud) = match cal {
        Some(c) => {
            let s = (gain_db - c.gain_db) as f64;
            (Some(c.noise_peak_db + s), Some(c.voice_low_db + s), Some(c.voice_db + s), Some(c.voice_peak_db + s), c.loud_peak_db.map(|x| x + s))
        }
        None => (None, None, None, None, None),
    };
    let (range_target, max_idx, below, att, rel, mk) = p.comp;
    let range = if v.levels.range_db > 0.0 { v.levels.range_db } else { 15.0 };
    let ridx = ratio_idx_for(range / range_target, max_idx);
    let ratio = RATIOS[ridx as usize];
    let mut comp = Comp { threshold: d.comp.threshold, ratio: ridx, attack: att, release: rel, makeup: d.comp.makeup };
    if let (Some(med), Some(peak)) = (cal_med, cal_peak) {
        comp.threshold = (med - below).round().clamp(-40.0, -6.0) as i32;
        let thr = comp.threshold as f64;
        let mut makeup = (peak - thr).max(0.0) * (1.0 - 1.0 / ratio) * mk;
        if let Some(loud) = cal_loud {
            let out = thr + (loud - thr).max(0.0) / ratio;
            makeup = makeup.min(-3.0 - out);
        }
        comp.makeup = makeup.round().clamp(0.0, 12.0) as i32;
    }
    d.comp = comp;
    let (thr_off, max_red, gate_rel) = p.gate;
    let mut gate = Gate { enabled: true, threshold: d.gate.threshold, attenuation: d.gate.attenuation, attack: 0, release: gate_rel };
    let snr = match (cal_med, cal_noise) {
        (Some(m), Some(n)) => m - n,
        _ => v.levels.speech_p50_db - v.levels.noise_peak_db,
    };
    if let (Some(noise), Some(low)) = (cal_noise, cal_low) {
        gate.threshold = (dsp::gate_threshold(noise, low) + thr_off).round().clamp(-59.0, -10.0) as i32;
        gate.attenuation = match max_red {
            None => 100,
            Some(m) => dsp::gate_att_percent(m.min(GATE_SOFT_DB).min(noise.max(-72.2) + d.comp.makeup as f64 - RESIDUAL_NOISE_DB), 12.0),
        };
    } else if max_red.is_none() {
        gate.attenuation = 100;
    }
    if p.key == "naturel" && snr >= 30.0 {
        gate.enabled = false; // pièce calme : rien à couper entre les phrases
    }
    d.gate = gate;
    let fx = dsp::fx_from_dsp(&d);
    // explications
    let mut notes = vec![];
    for t in band_notes(&bands, &pl, label, model) {
        notes.push(note("ok", t));
    }
    if bands.iter().all(|(_, g)| *g == 0) {
        notes.push(note("ok", "Ton spectre est déjà dans la zone visée : égaliseur à plat."));
    }
    let sib = sibilance_label(v.sibilance.excess_db);
    notes.push(note("ok", format!("Sifflantes {sib} : de-esser à {} %.", d.deesser)));
    if fx.gate {
        let red = dsp::gate_attenuation(d.gate.attenuation);
        notes.push(note("ok", format!("Noise gate à {} : {} entre tes phrases.", fmt_db(d.gate.threshold as f64), if d.gate.attenuation >= 100 { "silence complet".to_string() } else { format!("{} de bruit en moins", fmt_db(red as f64).replace('\u{2212}', "")) })));
    } else {
        notes.push(note("ok", "Pièce calme : pas de noise gate, ta voix reste entière."));
    }
    notes.push(note("ok", format!(
        "Compresseur {} dès {} (attaque {} ms, relâchement {} ms), {} de compensation : ta voix reste régulière.",
        ratio_txt(d.comp.ratio), fmt_db(d.comp.threshold as f64), COMP_ATTACK_MS[d.comp.attack.clamp(0, 19) as usize],
        COMP_RELEASE_MS[d.comp.release.clamp(0, 19) as usize], fmt_db(d.comp.makeup as f64)
    )));
    let chart = Chart {
        bands: g.iter().map(|f| (f * 100.0).round() / 100.0).collect(),
        voice: pl.voice_rel.iter().map(|x| r1(x.clamp(-80.0, 30.0))).collect(),
        after: pl.voice_rel.iter().zip(&eqc).map(|(a, e)| r1((a + e).clamp(-80.0, 30.0))).collect(),
        noise: pl.noise_rel.iter().map(|x| r1(x.clamp(-80.0, 30.0))).collect(),
        target_lo: pl.lo.iter().map(|x| r1(x.clamp(-80.0, 30.0))).collect(),
        target_hi: pl.hi.iter().map(|x| r1(x.clamp(-80.0, 30.0))).collect(),
    };
    Fitted { dsp: d, fx, notes, chart: Some(chart) }
}

/// Sans empreinte : préréglage habituel, avec la correction connue du micro (courbe publiée).
pub fn fit_with_model(name: &str, cal: Option<&MicCal>, gain_db: i32, model: &MicModel) -> Dsp {
    let mut d = dsp::fit_dsp(name, cal, Some(gain_db));
    if model.response.is_empty() && !model.bright {
        return d;
    }
    let g = grid();
    let base = eq_curve(&d, g);
    let resp: Vec<f64> = g.iter().map(|&f| mics::response_db(model, f)).collect();
    let mid = pmean(g, &resp, 800.0, 2500.0);
    let target: Vec<f64> = g
        .iter()
        .enumerate()
        .map(|(i, &f)| {
            let dev = resp[i] - mid;
            let cut = if f >= 2000.0 { -0.6 * dev.max(0.0) } else { 0.0 };
            (base[i] + cut).clamp(-9.0, 6.0)
        })
        .collect();
    let weight: Vec<f64> = g.iter().map(|&f| if f < 60.0 || f > 16000.0 { 0.3 } else { 1.0 }).collect();
    for ((b, _, _), (f, gn)) in EQ_BANDS.iter().zip(fit_bands(&target, &weight, 80.0)) {
        d.eq.insert(b.to_string(), gn);
        d.eq_freq.insert(b.to_string(), dsp::eq_freq_in_range(b, f));
    }
    if model.bright && d.deesser > 0 {
        d.deesser = (d.deesser + 10).min(80);
    }
    d
}

/// Réglages d'un préréglage pour l'état actuel : sur mesure avec une empreinte valide, sinon corrigés pour le
/// micro connu, sinon comme toujours (calés sur la voix mesurée par « Régler ma voix »).
pub fn profile_dsp(name: &str, st: &State) -> Dsp {
    let cal = st.cal_for(None);
    let gain = st.mic.gain_db;
    let model = st.mic.model.as_deref().and_then(mics::model);
    if let (Some(p), Some(rec)) = (profile(name), st.voice_for_mic()) {
        return fit_with_print(p, &rec.print, cal, gain, model).dsp;
    }
    match model {
        Some(m) if dsp::preset_fit(name).is_some() || name == "naturel" => fit_with_model(name, cal, gain, m),
        _ => dsp::fit_dsp(name, cal, Some(gain)),
    }
}

/// Résumé lisible de l'empreinte.
pub fn summary(v: &VoicePrint, cal: Option<&MicCal>) -> Value {
    let snr = cal.map(|c| c.voice_db - c.noise_peak_db).unwrap_or(v.levels.speech_p50_db - v.levels.noise_peak_db);
    json!({
        "voice_type": voice_type(v.f0_hz), "f0_hz": v.f0_hz.map(|f| f.round()),
        "snr_db": (snr * 10.0).round() / 10.0,
        "sibilance": sibilance_label(v.sibilance.excess_db), "sibilance_db": r1(v.sibilance.excess_db),
        "plosives": v.plosives.count, "hum_hz": v.hum.as_ref().map(|h| h.hz),
        "room": room_label(v.room.rt60_ms), "rt60_ms": v.room.rt60_ms.map(|r| r.round()),
        "speech_s": r1(v.speech_s), "clipped": v.clipped, "warnings": v.warnings,
    })
}

/// Conseils tirés de l'empreinte (placement, pièce, câble).
pub fn print_notes(v: &VoicePrint, model: Option<&MicModel>, gain_db: i32) -> Vec<Note> {
    let mut n = vec![];
    if let Some(h) = &v.hum {
        n.push(note("warn", format!("Ronflement à {} Hz : éloigne le câble du micro des alimentations et des multiprises. L’égaliseur l’atténue en attendant.", h.hz.round())));
    }
    if v.plosives.count >= 2 {
        n.push(note("tip", format!("{} « p » ou « b » ont soufflé dans le micro : place le filtre anti-pop à 5 cm de la grille, ou tourne légèrement le micro.", v.plosives.count)));
    }
    match room_label(v.room.rt60_ms) {
        Some("réverbérante") => n.push(note("warn", "Ta pièce résonne : rapproche-toi du micro et ajoute du tissu autour (rideaux, tapis, couette derrière l’écran).")),
        Some("un peu réverbérante") => n.push(note("tip", "Un peu d’écho de la pièce : quelques centimètres de moins jusqu’au micro l’effacent.")),
        _ => {}
    }
    if v.clipped {
        n.push(note("warn", "Ta voix a saturé pendant l’enregistrement : le gain a été baissé, refais l’empreinte pour une mesure parfaite."));
    }
    if let Some(m) = model.filter(|m| m.kind == "condenser" && !m.generic) {
        if gain_db >= 45 {
            n.push(note("tip", format!("Le {} demande {} dB de gain : tu es sans doute loin de lui. À {}–{} cm, ta voix sera plus pleine et la pièce plus discrète.", m.name, gain_db, m.distance_cm[0], m.distance_cm[1])));
        }
    }
    n
}

/// Ce que lit l'interface : GET /api/modules/goxlr-mini/voice.
pub fn voice_view(st: &State) -> Value {
    let rec = st.voice_print.as_ref();
    let valid = st.voice_for_mic();
    let model = st.mic.model.as_deref().and_then(mics::model);
    let cal = st.cal_for(None);
    let mut profiles = serde_json::Map::new();
    for pr in dsp::MIC_PRESETS.iter().filter(|x| x.key != "brut") {
        let (d, fx, notes, chart) = match (profile(pr.key), valid) {
            (Some(p), Some(r)) => {
                let f = fit_with_print(p, &r.print, cal, st.mic.gain_db, model);
                (f.dsp, f.fx, f.notes, f.chart)
            }
            _ => {
                let d = profile_dsp(pr.key, st);
                let fx = dsp::fx_from_dsp(&d);
                (d, fx, vec![], None)
            }
        };
        profiles.insert(pr.key.into(), json!({"label": pr.label, "desc": pr.desc, "dsp": d, "fx": fx, "notes": notes, "chart": chart, "fitted": valid.is_some()}));
    }
    json!({
        "print": rec, "valid": valid.is_some(),
        "mic": {"type": st.mic.kind, "model": st.mic.model, "gain_db": st.mic.gain_db},
        "profiles": profiles,
        "summary": valid.map(|r| summary(&r.print, cal)),
        "notes": valid.map(|r| print_notes(&r.print, model, st.mic.gain_db)).unwrap_or_default(),
    })
}

/// Modèle des bandes pour l'interface (S.meta.eq_model) et plages (S.meta.eq_ranges).
pub fn eq_model_json() -> Value {
    let mut m = serde_json::Map::new();
    for b in MINI_EQ {
        m.insert(b.band.into(), json!({"kind": b.kind, "q": b.q}));
    }
    Value::Object(m)
}
pub fn eq_ranges_json() -> Value {
    let mut m = serde_json::Map::new();
    for (b, lo, hi) in crate::consts::EQ_RANGES {
        m.insert(b.into(), json!([lo, hi]));
    }
    Value::Object(m)
}
