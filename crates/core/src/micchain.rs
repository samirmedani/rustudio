//! Chaîne de traitement de la voix du firmware Rustudio : réglages du micro (préréglage, profil sur mesure ou
//! réglages manuels) traduits en coefficients pour `micdsp`, et rendu sur le PC du même calcul, au bit près,
//! pour réécouter une prise avant et après.
//!
//! Les six bandes reprennent exactement les filtres de l'égaliseur de la Mini (`voicefit::MINI_EQ`) : un profil
//! sur mesure sonne pareil sous les deux firmwares. Le firmware Rustudio ajoute un coupe-bas (grondements,
//! chocs sur le pied) et un limiteur qui empêche toute saturation après la compensation du compresseur.
use crate::consts::{COMP_ATTACK_MS, COMP_RELEASE_MS, GATE_MS, RATIOS};
use crate::dsp::{self, Dsp};
use crate::micdsp::{self, Biquad, Chain, Params, BLOCK};
use crate::voicefit::{Kind, MINI_EQ};

const FS: f64 = 48000.0;
/// Rythme du calcul des gains : un sous-bloc de 8 échantillons.
const CONTROL_HZ: f64 = FS / 8.0;
/// Coupe-bas (Butterworth, 12 dB par octave) : sous la voix la plus grave, au-dessus des grondements.
pub const HPF_HZ: f64 = 65.0;
/// Plafond du limiteur (dBFS).
pub const CEILING_DB: f64 = -1.0;
/// Le de-esser ne baisse que les aigus au-dessus de cette fréquence.
pub const DEESS_HZ: f64 = 5000.0;

fn q28(x: f64) -> i32 {
    (x * (1u64 << 28) as f64).round().clamp(i32::MIN as f64, i32::MAX as f64) as i32
}

fn db256(db: f64) -> i32 {
    (db * 256.0).round() as i32
}

/// Coefficient Q30 d'un filtre à un pôle de constante de temps `ms`, au rythme des sous-blocs.
fn smoothing(ms: f64) -> i32 { smoothing_at(ms, FS) }

pub fn smoothing_at(ms: f64, rate: f64) -> i32 {
    let k = 1.0 - (-1.0 / (ms.max(0.2) / 1000.0 * (rate / 8.0))).exp();
    (k * (1u64 << 30) as f64).round() as i32
}

/// Biquad normalisé (a0 = 1) en Q28, depuis [b0, b1, b2, a0, a1, a2].
pub fn fixed(c: [f64; 6]) -> Biquad {
    let a0 = c[3];
    Biquad { b0: q28(c[0] / a0), b1: q28(c[1] / a0), b2: q28(c[2] / a0), a1: q28(c[4] / a0), a2: q28(c[5] / a0) }
}

fn high_pass(f0: f64, q: f64) -> [f64; 6] { high_pass_at(f0, q, FS) }

pub fn high_pass_at(f0: f64, q: f64, rate: f64) -> [f64; 6] {
    let w0 = 2.0 * std::f64::consts::PI * f0.min(rate * 0.49) / rate;
    let (cw, alpha) = (w0.cos(), w0.sin() / (2.0 * q));
    [(1.0 + cw) / 2.0, -(1.0 + cw), (1.0 + cw) / 2.0, 1.0 + alpha, -2.0 * cw, 1.0 - alpha]
}

fn band(kind: Kind, f0: f64, gain: f64, q: f64) -> [f64; 6] {
    let name = match kind {
        Kind::LowShelf => "LSC",
        Kind::Peak => "PK",
        Kind::HighShelf => "HSC",
    };
    crate::hpeq::biquad(name, f0, gain, q, FS)
}

fn index_ms(table: &[u32], index: i32) -> f64 {
    table.get(index.max(0) as usize).or(table.last()).copied().unwrap_or(10) as f64
}

/// Réglage du firmware pour des traitements déjà rendus effectifs (`dsp::effective_dsp`) : un traitement coupé
/// est absent. Rien d'actif : la voix sort brute, au bit près.
pub fn design(d: &Dsp) -> Params {
    let mut p = Params::default();
    for (n, m) in MINI_EQ.iter().enumerate() {
        let gain = (*d.eq.get(m.band).unwrap_or(&0) as f64).clamp(-9.0, 9.0);
        if gain == 0.0 {
            continue;
        }
        let f0 = d.eq_freq.get(m.band).copied().map(|f| dsp::eq_freq_in_range(m.band, f)).unwrap_or_else(|| crate::consts::eq_default_freq(m.band));
        p.eq[n] = fixed(band(m.kind, f0, gain, m.q));
        p.flags |= micdsp::FLAG_EQ0 << n;
    }
    let g = &d.gate;
    if g.enabled {
        p.flags |= micdsp::FLAG_GATE;
        p.gate_threshold = db256(g.threshold as f64);
        p.gate_hysteresis = db256(4.0);
        p.gate_range = db256(-dsp::gate_attenuation(g.attenuation) as f64);
        p.gate_open = smoothing(index_ms(&GATE_MS, g.attack));
        p.gate_close = smoothing(index_ms(&GATE_MS, g.release));
        // 80 ms de maintien : la fin des mots n'est pas coupée
        p.gate_hold = (0.080 * CONTROL_HZ) as i32;
    }
    let c = &d.comp;
    if c.ratio > 0 || c.makeup != 0 {
        let ratio = RATIOS[c.ratio.clamp(0, 14) as usize];
        p.flags |= micdsp::FLAG_COMP;
        p.comp_threshold = db256(c.threshold as f64);
        p.comp_slope = ((1.0 - 1.0 / ratio) * 65536.0).round() as i32;
        p.comp_makeup = db256(c.makeup.clamp(-6, 24) as f64);
        p.comp_attack = smoothing(index_ms(&COMP_ATTACK_MS, c.attack).max(0.5));
        p.comp_release = smoothing(index_ms(&COMP_RELEASE_MS, c.release));
    }
    if d.deesser > 0 {
        p.flags |= micdsp::FLAG_DEESS;
        p.deess_hp = fixed(high_pass(DEESS_HZ, std::f64::consts::FRAC_1_SQRT_2));
        // 100 % : jusqu'à 10 dB de moins sur les « s » les plus marqués
        p.deess_max = db256(d.deesser.clamp(0, 100) as f64 / 10.0);
        p.deess_offset = db256(6.0);
        p.deess_floor = db256(-50.0);
        p.deess_attack = smoothing(1.0);
        p.deess_release = smoothing(40.0);
        p.deess_scale = db256(2.0);
    }
    if p.active() {
        p.flags |= micdsp::FLAG_HPF | micdsp::FLAG_LIMIT;
        p.hpf = fixed(high_pass(HPF_HZ, std::f64::consts::FRAC_1_SQRT_2));
        p.limit_ceiling = db256(CEILING_DB);
        p.limit_release = smoothing(60.0);
    }
    p
}

/// La prise passée par la chaîne, comme le firmware la traiterait (échantillons de -1 à 1).
pub fn render(p: &Params, samples: &[f32]) -> Vec<f32> {
    let mut chain = Chain::default();
    let mut out = Vec::with_capacity(samples.len());
    for chunk in samples.chunks(BLOCK) {
        let mut block = [0i32; BLOCK];
        for (slot, &x) in block.iter_mut().zip(chunk) {
            let safe = if x.is_finite() { x.clamp(-1.0, 1.0) } else { 0.0 };
            *slot = (safe as f64 * micdsp::FULL as f64).round() as i32;
        }
        chain.process(p, &mut block);
        out.extend(block.iter().take(chunk.len()).map(|&y| (y as f64 / micdsp::FULL as f64) as f32));
    }
    out
}

/// Met deux prises au même niveau sur les passages de voix, par atténuation seulement.
/// Marge avant la protection d'écoute de Windows, qui reste active sur chaque prise.
pub fn match_voice_level(raw: &[f32], processed: &[f32], after: bool) -> Result<Vec<f32>, &'static str> {
    if raw.is_empty() || raw.len() != processed.len() {
        return Err("Les deux prises ne correspondent pas.");
    }
    const BLOCK_25_MS: usize = 1200;
    const PEAK: f64 = 0.16;
    let power = |block: &[f32]| block.iter().filter(|x| x.is_finite())
        .map(|x| f64::from(*x).powi(2)).sum::<f64>() / block.len() as f64;
    let loudest = raw.chunks(BLOCK_25_MS).map(power).fold(0.0, f64::max);
    let threshold = (loudest * 0.08).max(0.000_001);
    let peak = |samples: &[f32]| samples.iter().copied().filter(|x| x.is_finite())
        .map(|x| f64::from(x.abs())).fold(PEAK, f64::max);
    let (mut a, mut b) = (0., 0.);
    for (before, after) in raw.chunks(BLOCK_25_MS).zip(processed.chunks(BLOCK_25_MS)) {
        let level = power(before);
        if level >= threshold {
            a += level * before.len() as f64;
            b += power(after) * after.len() as f64;
        }
    }
    if a == 0. || b <= f64::EPSILON {
        return Err("La voix est trop faible pour comparer les prises à volume égal.");
    }
    let (a, b) = (a.sqrt(), b.sqrt());
    let target = (a * PEAK / peak(raw)).min(b * PEAK / peak(processed));
    let (samples, level) = if after { (processed, b) } else { (raw, a) };
    let gain = (target / level) as f32;
    Ok(samples.iter().map(|x| if x.is_finite() { *x * gain } else { 0. }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::micdsp::{gain_q24, level_db};

    #[test]
    fn comparaison_nivelle_la_voix_sans_remonter_les_pauses() {
        let mut raw = vec![0.002; 4800];
        raw.extend(vec![0.04; 2400]);
        let mut processed = vec![0.0002; 4800];
        processed.extend(vec![0.4; 2400]);
        let a = match_voice_level(&raw, &processed, false).unwrap();
        let b = match_voice_level(&raw, &processed, true).unwrap();
        assert!((a[4900] - b[4900]).abs() < 1e-7);
        assert!(a[4900] <= raw[4900]);
        assert!(b[100] < a[100] * 0.05);
        assert!(a.iter().chain(&b).all(|x| x.abs() <= 0.16));
    }

    #[test]
    fn comparaison_ecarte_le_silence_et_assainit_les_echantillons_invalides() {
        assert!(match_voice_level(&[], &[], false).is_err());
        assert!(match_voice_level(&[0.1], &[0.; 2], true).is_err());
        assert!(match_voice_level(&[0.1], &[0.], true).is_err());
        assert!(match_voice_level(&[f32::NAN], &[0.1], false).is_err());
        let out = match_voice_level(&[0.2, f32::NAN, 0.2], &[0.4, 0.6, 0.4], false).unwrap();
        assert!(out.iter().all(|x| x.abs() <= 0.16 && x.is_finite()));
        assert_eq!(out[1], 0.);
    }

    fn sine(hz: f64, db: f64, seconds: f64) -> Vec<f32> {
        let amp = 10f64.powf(db / 20.0);
        (0..(seconds * FS) as usize).map(|n| (amp * (2.0 * std::f64::consts::PI * hz * n as f64 / FS).sin()) as f32).collect()
    }

    /// Niveau (dBFS) de la fin d'une sinusoïde, depuis sa valeur efficace : indépendant de la phase des
    /// échantillons, même à 12 kHz.
    fn tail_db(x: &[f32], seconds: f64) -> f64 {
        let n = (seconds * FS) as usize;
        let power = x[x.len() - n..].iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>() / n as f64;
        10.0 * (2.0 * power).max(1e-24).log10()
    }

    fn only(flags: u32, d: &Dsp) -> Params {
        let mut p = design(d);
        p.flags &= flags;
        p
    }

    fn neutral() -> Dsp {
        dsp::effective_dsp(&dsp::preset_dsp("brut"), &dsp::Fx::default())
    }

    #[test]
    fn mesures_en_db() {
        for db in [-100.0, -60.0, -30.0, -12.5, -6.0, -0.5, 0.0, 6.0, 20.0] {
            let v = (10f64.powf(db / 20.0) * (1 << 23) as f64).round() as u32;
            let exact = 20.0 * (v as f64 / (1 << 23) as f64).log10();
            assert!((level_db(v) as f64 / 256.0 - exact).abs() < 0.02, "{db} → {}", level_db(v) as f64 / 256.0);
            let g = gain_q24(db256(db)) as f64 / (1 << 24) as f64;
            // à -100 dB, le gain ne vaut plus que 168 en Q24 : la quantification domine
            assert!((20.0 * g.log10() - db).abs() < if db < -80.0 { 0.06 } else { 0.02 }, "{db} → {g}");
        }
        assert_eq!(gain_q24(0), micdsp::UNITY);
        assert_eq!(level_db(0), micdsp::FLOOR_DB);
    }

    #[test]
    fn aller_retour_du_reglage() {
        let p = design(&dsp::preset_dsp("radio"));
        assert_eq!(Params::decode(&p.encode()), Some(p));
        assert_eq!(Params::decode(&[0; micdsp::BYTES]), None);
        assert_eq!(Params::decode(&p.encode()[..100]), None);
    }

    #[test]
    fn sans_traitement_la_voix_sort_brute() {
        let p = design(&neutral());
        assert!(!p.active());
        let x = sine(440.0, -6.0, 0.1);
        let y = render(&p, &x);
        for (a, b) in x.iter().zip(&y) {
            assert!((a - b).abs() < 1.0 / (1 << 22) as f32);
        }
    }

    #[test]
    fn egaliseur_identique_a_celui_de_la_mini() {
        let mut d = neutral();
        d.eq.insert("1000".into(), 6);
        d.eq.insert("8000".into(), -4);
        d.eq_freq.insert("1000".into(), 1500.0);
        let p = only(micdsp::FLAG_EQ0 * 0b111111, &d);
        for f in [200.0, 1500.0, 3000.0, 12000.0] {
            let y = render(&p, &sine(f, -20.0, 0.5));
            let want = -20.0 + crate::voicefit::eq_curve(&d, &[f])[0];
            assert!((tail_db(&y, 0.1) - want).abs() < 0.1, "{f} Hz : {} au lieu de {want}", tail_db(&y, 0.1));
        }
    }

    #[test]
    fn coupe_bas_et_graves_tres_bas_sans_bruit() {
        let p = only(micdsp::FLAG_HPF, &dsp::preset_dsp("studio"));
        assert!(tail_db(&render(&p, &sine(25.0, -6.0, 1.0)), 0.3) < -20.0);
        assert!((tail_db(&render(&p, &sine(1000.0, -6.0, 0.2)), 0.1) + 6.0).abs() < 0.05);
        // silence numérique : aucun bruit créé par l'arrondi
        assert!(render(&p, &vec![0.0; 48000]).iter().all(|&v| v == 0.0));
    }

    #[test]
    fn noise_gate_coupe_la_piece_et_laisse_la_voix() {
        let mut d = dsp::preset_dsp("stream");
        d.gate.threshold = -45;
        d.gate.attenuation = 100;
        let p = only(micdsp::FLAG_GATE, &d);
        // bruit de pièce à -60 dBFS : baissé de toute la profondeur (-61 dB) après le maintien et la fermeture
        assert!(tail_db(&render(&p, &sine(300.0, -60.0, 2.0)), 0.2) < -100.0);
        // voix à -20 dBFS : intacte
        assert!((tail_db(&render(&p, &sine(300.0, -20.0, 1.0)), 0.2) + 20.0).abs() < 0.05);
    }

    #[test]
    fn compresseur_et_compensation() {
        let mut d = neutral();
        d.comp = dsp::Comp { threshold: -20, ratio: 9, attack: 1, release: 9, makeup: 3 }; // ratio 4:1
        let p = only(micdsp::FLAG_COMP, &d);
        // -6 dBFS : 14 dB au-dessus du seuil, ramenés à 3,5 dB, + 3 dB de compensation
        let out = tail_db(&render(&p, &sine(500.0, -6.0, 1.0)), 0.2);
        assert!((out - (-20.0 + 3.5 + 3.0)).abs() < 0.3, "{out}");
        // sous le coude : seulement la compensation
        let quiet = tail_db(&render(&p, &sine(500.0, -40.0, 1.0)), 0.2);
        assert!((quiet - (-40.0 + 3.0)).abs() < 0.1, "{quiet}");
    }

    #[test]
    fn limiteur_jamais_au_dessus_du_plafond() {
        let mut d = neutral();
        d.comp = dsp::Comp { threshold: 0, ratio: 1, attack: 0, release: 9, makeup: 12 };
        let p = only(micdsp::FLAG_COMP | micdsp::FLAG_LIMIT, &d);
        let y = render(&p, &sine(200.0, -3.0, 0.5));
        let peak = y.iter().fold(0f32, |p, v| p.max(v.abs()));
        assert!(20.0 * (peak as f64).log10() <= CEILING_DB + 0.05, "{peak}");
    }

    #[test]
    fn de_esser_baisse_les_s_et_pas_la_voix() {
        let mut d = neutral();
        d.deesser = 100;
        let p = only(micdsp::FLAG_DEESS, &d);
        let s = tail_db(&render(&p, &sine(7000.0, -12.0, 0.5)), 0.1);
        assert!(s < -12.0 - 6.0, "« s » à {s} dB");
        let voice = tail_db(&render(&p, &sine(400.0, -12.0, 0.5)), 0.1);
        assert!((voice + 12.0).abs() < 0.1, "voix à {voice} dB");
    }

    /// Réglage « Broadcast », deux blocs du micro de l'émulateur du firmware et leur sortie : le firmware
    /// doit rendre exactement ces échantillons (`verify_firmware.py`). `RUSTUDIO_VECTEUR=<fichier>` le
    /// réécrit après un changement voulu du calcul.
    #[test]
    fn vecteur_du_firmware() {
        let p = design(&dsp::preset_dsp("radio"));
        let words: Vec<i32> = p.encode().chunks_exact(4).map(|w| i32::from_le_bytes([w[0], w[1], w[2], w[3]])).collect();
        let input: Vec<i32> = (0..2 * BLOCK).map(|n| if n % 2 == 0 { 0x123456 } else { -0x234567 }).collect();
        let mut chain = Chain::default();
        let mut output = vec![];
        for chunk in input.chunks(BLOCK) {
            let mut block = [0i32; BLOCK];
            block.copy_from_slice(chunk);
            chain.process(&p, &mut block);
            output.extend(block);
        }
        let vector = serde_json::json!({"params": words, "input": input, "output": output});
        if let Ok(path) = std::env::var("RUSTUDIO_VECTEUR") {
            std::fs::write(path, serde_json::to_string(&vector).unwrap() + "\n").unwrap();
            return;
        }
        let stored: serde_json::Value = serde_json::from_str(include_str!("micdsp-vecteur.json")).unwrap();
        assert_eq!(stored, vector, "calcul changé : régénérer micdsp-vecteur.json");
    }

    #[test]
    fn preregles_actifs_et_marge() {
        for preset in dsp::MIC_PRESETS.iter().filter(|p| p.key != "brut") {
            let p = design(&(preset.dsp)());
            assert!(p.flags & micdsp::FLAG_HPF != 0 && p.flags & micdsp::FLAG_LIMIT != 0, "{}", preset.key);
            let y = render(&p, &sine(150.0, -1.0, 0.5));
            assert!(y.iter().all(|v| v.abs() <= 1.0), "{}", preset.key);
        }
    }

    #[test]
    fn profil_radio_fm_skyrock_dense_et_sans_clipping() {
        let fm = design(&dsp::preset_dsp("antenne"));
        let doux = design(&dsp::preset_dsp("naturel"));
        let quiet_voice = sine(220.0, -24.0, 0.5);
        let loud_voice = sine(220.0, -2.0, 0.5);
        // Radio FM remonte nettement les passages doux tout en plafonnant les pics sous -1 dBFS.
        assert!(tail_db(&render(&fm, &quiet_voice), 0.15) > tail_db(&render(&doux, &quiet_voice), 0.15) + 3.0);
        let peak = render(&fm, &loud_voice).iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!(20.0 * (peak as f64).log10() <= CEILING_DB + 0.05, "pic FM trop haut : {peak}");
    }

    #[test]
    fn biquads_extremes_stables_et_echantillons_non_finis_filtres() {
        let mut d = neutral();
        d.eq.insert("90".into(), 9);
        d.eq_freq.insert("90".into(), 30.0);
        d.eq.insert("250".into(), -9);
        d.eq_freq.insert("250".into(), 100.0);
        d.eq.insert("8000".into(), 9);
        d.eq_freq.insert("8000".into(), 18000.0);
        d.comp = dsp::Comp { threshold: -40, ratio: 14, attack: 0, release: 0, makeup: 24 };
        d.deesser = 100;
        let p = design(&d);
        let mut x = sine(180.0, -0.5, 0.5);
        x[10] = f32::NAN;
        x[11] = f32::INFINITY;
        x[12] = f32::NEG_INFINITY;
        let y = render(&p, &x);
        assert_eq!(y.len(), x.len());
        assert!(y.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
        let peak = y.iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!(20.0 * (peak as f64).log10() <= CEILING_DB + 0.05, "pic non limite : {peak}");
    }

    #[test]
    fn compresseur_ratio_un_applique_quand_meme_la_compensation() {
        let mut d = neutral();
        d.comp = dsp::Comp { threshold: -20, ratio: 0, attack: 1, release: 9, makeup: 6 };
        let p = design(&d);
        assert!(p.flags & micdsp::FLAG_COMP != 0 && p.flags & micdsp::FLAG_LIMIT != 0);
        let out = tail_db(&render(&p, &sine(500.0, -24.0, 0.4)), 0.1);
        assert!((out - (-18.0)).abs() < 0.15, "compensation seule attendue a -18 dB, obtenu {out}");
    }
}
