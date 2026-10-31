//! Empreinte vocale : analyse de signaux construits, sans matériel.
use rustudio_core::voice::{analyze, band_centers, synth_speech, Phase, Segment, VoicePrint, FLOOR_DB};
use std::f64::consts::PI;

fn seg(phase: Phase, start_s: f64, end_s: f64, gain_db: i32) -> Segment {
    Segment { phase, start_s, end_s, gain_db }
}

/// Valeur efficace d'un niveau en dB (une sinusoïde pleine échelle vaut 0 dB).
fn rms(db: f64) -> f64 {
    10f64.powf(db / 20.0) / 2f64.sqrt()
}

fn band(f: f64) -> usize {
    band_centers().iter().position(|c| (c / f - 1.0).abs() < 0.01).expect("bande")
}

/// Bruit blanc déterministe de variance 1 (xorshift).
struct White(u64);
impl White {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0) * 3f64.sqrt()
    }
}

/// Signal en construction : on ajoute des morceaux, puis on le donne à l'analyse.
struct Sig {
    fs: f64,
    x: Vec<f64>,
    noise: White,
}
impl Sig {
    fn new(fs: f64, secs: f64) -> Sig {
        Sig { fs, x: vec![0.0; (secs * fs) as usize], noise: White(0x1234_5678_9ABC_DEF1) }
    }
    fn range(&self, a: f64, b: f64) -> std::ops::Range<usize> {
        ((a * self.fs) as usize).min(self.x.len())..((b * self.fs) as usize).min(self.x.len())
    }
    fn noise(&mut self, a: f64, b: f64, db: f64) -> &mut Self {
        let r = rms(db);
        for i in self.range(a, b) {
            self.x[i] += r * self.noise.next();
        }
        self
    }
    fn sine(&mut self, a: f64, b: f64, f: f64, amp: f64) -> &mut Self {
        for i in self.range(a, b) {
            self.x[i] += amp * (2.0 * PI * f * i as f64 / self.fs).sin();
        }
        self
    }
    /// Voyelle : n harmoniques de f0 en 1/h, niveau efficace level_db, rampes de 10 ms.
    fn vowel(&mut self, a: f64, b: f64, f0: f64, n: usize, level_db: f64) -> &mut Self {
        let norm: f64 = (1..=n).map(|h| 1.0 / (h * h) as f64).sum::<f64>().sqrt();
        let amp = rms(level_db) * 2f64.sqrt() / norm;
        let r = self.range(a, b);
        let (start, len) = (r.start, r.len());
        let ramp = (0.01 * self.fs) as usize;
        for i in r {
            let t = i as f64 / self.fs;
            let k = i - start;
            let g = (k.min(len - 1 - k) as f64 / ramp as f64).min(1.0);
            let v: f64 = (1..=n).map(|h| (2.0 * PI * f0 * h as f64 * t).sin() / h as f64).sum();
            self.x[i] += g * amp * v;
        }
        self
    }
    /// Sifflante : bruit blanc passe-bande (fc, Q 2), niveau efficace level_db.
    fn hiss(&mut self, a: f64, b: f64, fc: f64, level_db: f64) -> &mut Self {
        let w0 = 2.0 * PI * fc / self.fs;
        let al = w0.sin() / 4.0;
        let (b0, b2, a0, a1, a2) = (al, -al, 1.0 + al, -2.0 * w0.cos(), 1.0 - al);
        let r = self.range(a, b);
        let mut buf = vec![0.0; r.len()];
        let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
        for v in buf.iter_mut() {
            let x0 = self.noise.next();
            let y = (b0 * x0 + b2 * x2 - a1 * y1 - a2 * y2) / a0;
            (x2, x1, y2, y1) = (x1, x0, y1, y);
            *v = y;
        }
        let ms = buf.iter().map(|v| v * v).sum::<f64>() / buf.len() as f64;
        let k = rms(level_db) / ms.sqrt();
        for (i, v) in r.zip(buf) {
            self.x[i] += k * v;
        }
        self
    }
    /// Coup de souffle : 35 ms de f Hz sous une demi-sinusoïde, crête amp.
    fn thump(&mut self, a: f64, f: f64, amp: f64, dur: f64) -> &mut Self {
        let r = self.range(a, a + dur);
        let (start, len) = (r.start, r.len());
        for i in r {
            let k = (i - start) as f64;
            self.x[i] += amp * (PI * k / len as f64).sin() * (2.0 * PI * f * k / self.fs).cos();
        }
        self
    }
    fn samples(&self) -> Vec<f32> {
        self.x.iter().map(|v| *v as f32).collect()
    }
}

fn power_sum(db: &[f64]) -> f64 {
    10.0 * db.iter().filter(|v| **v > FLOOR_DB).map(|v| 10f64.powf(v / 10.0)).sum::<f64>().log10()
}

#[test]
fn sinus_1khz_dans_sa_bande() {
    let mut s = Sig::new(16000.0, 3.5);
    s.noise(0.0, 3.5, -90.0).sine(1.4, 3.5, 1000.0, 0.25);
    let segs = [seg(Phase::Silence, 0.0, 1.5, 40), seg(Phase::Reading, 1.5, 3.5, 40)];
    let p = analyze(&s.samples(), 16000, &segs).unwrap();
    let i = band(1000.0);
    let top = (0..p.voice_db.len()).max_by(|a, b| p.voice_db[*a].total_cmp(&p.voice_db[*b])).unwrap();
    assert_eq!(top, i, "{:?}", p.voice_db);
    let expected = 20.0 * 0.25f64.log10();
    assert!((p.voice_db[i] - expected).abs() < 1.5, "{} contre {expected}", p.voice_db[i]);
    assert!((power_sum(&p.voice_db) - expected).abs() < 0.3, "puissance totale {}", power_sum(&p.voice_db));
    for (k, v) in p.voice_db.iter().enumerate() {
        if k + 1 < i || k > i + 1 {
            assert!(*v < expected - 20.0, "bande {k} : {v}");
        }
    }
    assert!((p.levels.speech_p50_db - expected).abs() < 0.2, "{:?}", p.levels);
    assert!((p.levels.peak_db - expected).abs() < 0.1);
    assert!(
        p.warnings.contains(&"too_short".to_string()) && p.warnings.contains(&"no_loud".to_string()),
        "{:?}",
        p.warnings
    );
}

#[test]
fn fondamentale_retrouvee() {
    for f0 in [120.0, 220.0] {
        let mut s = Sig::new(16000.0, 4.5);
        s.noise(0.0, 4.5, -80.0).vowel(1.4, 4.5, f0, 10, -20.0);
        let segs = [seg(Phase::Silence, 0.0, 1.5, 40), seg(Phase::Reading, 1.5, 4.5, 40)];
        let p = analyze(&s.samples(), 16000, &segs).unwrap();
        for v in [p.f0_hz, p.f0_low_hz, p.f0_high_hz] {
            let v = v.expect("fondamentale");
            assert!((v / f0 - 1.0).abs() < 0.03, "{f0} Hz : {v}");
        }
    }
}

#[test]
fn bruit_blanc_du_silence() {
    let fs = 16000.0;
    let mut s = Sig::new(fs, 5.0);
    s.noise(0.0, 3.0, -50.0).sine(2.9, 5.0, 1000.0, 0.25);
    let segs = [seg(Phase::Silence, 0.0, 3.0, 40), seg(Phase::Reading, 3.0, 5.0, 40)];
    let p = analyze(&s.samples(), 16000, &segs).unwrap();
    assert!((p.levels.noise_rms_db + 50.0).abs() < 0.5, "{:?}", p.levels);
    assert!(p.levels.noise_peak_db > p.levels.noise_rms_db && p.levels.noise_peak_db < -49.0, "{:?}", p.levels);
    // densité plate : chaque bande vaut la variance × sa largeur / (fs/2)
    let var = rms(-50.0).powi(2);
    let mut flat = vec![];
    for (c, v) in p.bands.iter().zip(&p.noise_db) {
        if *c < 100.0 || *v <= FLOOR_DB {
            continue;
        }
        let bw = c * (2f64.powf(1.0 / 12.0) - 2f64.powf(-1.0 / 12.0));
        let expected = 10.0 * (2.0 * var * bw / (fs / 2.0)).log10();
        assert!((v - expected).abs() < 1.5, "{c} Hz : {v} contre {expected}");
        flat.push(v - 10.0 * bw.log10());
    }
    let (lo, hi) = flat.iter().fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
    assert!(hi - lo < 3.0, "densité {lo}..{hi}");
    // au-delà de 0,95 × 8 kHz : pas de donnée
    assert_eq!(*p.noise_db.last().unwrap(), FLOOR_DB);
    assert!(p.hum.is_none());
}

#[test]
fn sifflantes_reperees() {
    let fs = 32000.0;
    let build = |with_s: bool| {
        let mut s = Sig::new(fs, 7.5);
        s.noise(0.0, 7.5, -80.0);
        let mut t = 1.6;
        while t + 0.45 < 7.5 {
            s.vowel(t, t + 0.2, 150.0, 8, -20.0);
            if with_s {
                s.hiss(t + 0.25, t + 0.37, 7000.0, -30.0);
            }
            t += 0.45;
        }
        let segs = [seg(Phase::Silence, 0.0, 1.5, 40), seg(Phase::Reading, 1.5, 7.5, 40)];
        analyze(&s.samples(), 32000, &segs).unwrap()
    };
    let (with, without) = (build(true), build(false));
    assert!(with.sibilance.ratio > 0.1, "{:?}", with.sibilance);
    let peak = with.sibilance.peak_hz.unwrap();
    assert!((5000.0..=9000.0).contains(&peak), "{peak}");
    assert!(with.sibilant_db.is_some());
    assert!(with.sibilance.excess_db > 10.0, "{:?}", with.sibilance);
    assert_eq!(without.sibilance.ratio, 0.0);
    assert!(without.sibilant_db.is_none());
    assert!(
        with.sibilance.excess_db - without.sibilance.excess_db > 15.0,
        "{:?} / {:?}",
        with.sibilance,
        without.sibilance
    );
}

#[test]
fn plosives_comptees() {
    let build = |thumps: &[(f64, f64)]| {
        let mut s = Sig::new(16000.0, 7.5);
        s.noise(0.0, 7.5, -80.0);
        let mut t = 1.6;
        while t + 0.35 < 7.5 {
            s.vowel(t, t + 0.25, 130.0, 10, -20.0);
            t += 0.35;
        }
        for (at, dur) in thumps {
            s.thump(*at, 45.0, 0.5, *dur);
        }
        let segs = [seg(Phase::Silence, 0.0, 1.5, 40), seg(Phase::Reading, 1.5, 7.5, 40)];
        analyze(&s.samples(), 16000, &segs).unwrap()
    };
    let p = build(&[(2.5, 0.035), (4.0, 0.035), (5.5, 0.035)]);
    assert_eq!(p.plosives.count, 3, "{:?}", p.plosives);
    assert!(p.plosives.worst_db >= 15.0, "{:?}", p.plosives);
    assert_eq!(p.plosives.per_minute, 30.0);
    assert_eq!(build(&[]).plosives.count, 0);
    // un grondement d'une demi-seconde n'est pas une plosive
    assert_eq!(build(&[(3.0, 0.5)]).plosives.count, 0);
}

#[test]
fn ronflement_du_secteur() {
    let build = |base: f64, db: f64| {
        let mut s = Sig::new(16000.0, 5.0);
        s.noise(0.0, 5.0, -75.0).sine(2.9, 5.0, 1000.0, 0.25);
        if db > -200.0 {
            s.sine(0.0, 5.0, base, rms(db) * 2f64.sqrt()).sine(0.0, 5.0, 2.0 * base, rms(db - 7.0) * 2f64.sqrt());
        }
        let segs = [seg(Phase::Silence, 0.0, 3.0, 40), seg(Phase::Reading, 3.0, 5.0, 40)];
        analyze(&s.samples(), 16000, &segs).unwrap()
    };
    let h = build(50.0, -55.0).hum.expect("ronflement");
    assert_eq!(h.hz, 50.0);
    assert!(h.level_db >= 10.0, "{h:?}");
    assert_eq!(build(60.0, -55.0).hum.expect("ronflement").hz, 60.0);
    assert!(build(50.0, -300.0).hum.is_none());
    // inaudible : ignoré
    assert!(build(50.0, -95.0).hum.is_none());
}

#[test]
fn gains_ramenes_a_la_lecture() {
    let build = |g_silence: i32, g_loud: i32| {
        let mut s = Sig::new(16000.0, 6.0);
        s.noise(0.0, 2.0, -60.0).sine(1.9, 4.0, 1000.0, 0.1).sine(4.0, 6.0, 1000.0, 0.5);
        let segs = [
            seg(Phase::Silence, 0.0, 2.0, g_silence),
            seg(Phase::Reading, 2.0, 4.0, 40),
            seg(Phase::Loud, 4.0, 6.0, g_loud),
        ];
        analyze(&s.samples(), 16000, &segs).unwrap()
    };
    let same = build(40, 40);
    let shifted = build(30, 34);
    assert!((same.levels.noise_rms_db + 60.0).abs() < 0.3, "{:?}", same.levels);
    assert!((shifted.levels.noise_rms_db + 50.0).abs() < 0.3, "{:?}", shifted.levels);
    assert!((shifted.levels.noise_peak_db - same.levels.noise_peak_db - 10.0).abs() < 0.15);
    for (a, b) in same.noise_db.iter().zip(&shifted.noise_db) {
        if *a > FLOOR_DB {
            assert!((b - a - 10.0).abs() < 0.15, "{a} / {b}");
        }
    }
    let loud = 20.0 * 0.5f64.log10();
    assert!((same.levels.loud_peak_db.unwrap() - loud).abs() < 0.1, "{:?}", same.levels);
    assert!((shifted.levels.loud_peak_db.unwrap() - (loud + 6.0)).abs() < 0.1, "{:?}", shifted.levels);
    assert_eq!(shifted.gain_db, 40);
    assert!(!shifted.warnings.contains(&"no_loud".to_string()));
}

#[test]
fn ecretage_signale() {
    let mut s = Sig::new(16000.0, 4.0);
    s.noise(0.0, 4.0, -80.0).sine(1.4, 4.0, 300.0, 1.3);
    s.x.iter_mut().for_each(|v| *v = v.clamp(-1.0, 1.0));
    let segs = [seg(Phase::Silence, 0.0, 1.5, 40), seg(Phase::Reading, 1.5, 4.0, 40)];
    let p = analyze(&s.samples(), 16000, &segs).unwrap();
    assert!(p.clipped);
    assert!(p.warnings.contains(&"clipped".to_string()));
    assert_eq!(p.levels.peak_db, 0.0);
}

fn take(p: &VoicePrint, w: &str) -> bool {
    p.warnings.iter().any(|x| x == w)
}

fn synth_segments(g_silence: i32) -> Vec<Segment> {
    vec![seg(Phase::Silence, 0.0, 3.0, g_silence), seg(Phase::Reading, 3.0, 11.0, 40), seg(Phase::Loud, 11.0, 14.0, 40)]
}

#[test]
fn voix_de_synthese_analysable() {
    let segs = synth_segments(40);
    for (f0, seed) in [(120.0, 7), (220.0, 3)] {
        let x = synth_speech(32000, &segs, f0, seed);
        assert_eq!(x.len(), 14 * 32000);
        assert_eq!(x, synth_speech(32000, &segs, f0, seed), "déterministe");
        let p = analyze(&x, 32000, &segs).unwrap();
        let f = p.f0_hz.expect("fondamentale");
        assert!((f / f0 - 1.0).abs() < 0.05, "{f0} : {f}");
        assert!(p.speech_s >= 4.0, "{}", p.speech_s);
        assert!(p.sibilance.ratio > 0.03, "{:?}", p.sibilance);
        assert!((4000.0..=9000.0).contains(&p.sibilance.peak_hz.unwrap()), "{:?}", p.sibilance);
        assert!(p.plosives.count >= 1, "{:?}", p.plosives);
        assert!(p.hum.is_none() && !p.clipped, "{p:?}");
        assert!(p.warnings.is_empty(), "{:?}", p.warnings);
        assert!(p.levels.loud_peak_db.unwrap() > p.levels.peak_db, "{:?}", p.levels);
        assert!((p.levels.noise_rms_db + 70.0).abs() < 3.0, "{:?}", p.levels);
        assert!((-30.0..=-16.0).contains(&p.levels.speech_p50_db), "{:?}", p.levels);
        assert!(p.room.rt60_ms.is_none_or(|r| r < 300.0), "{:?}", p.room);
    }
}

#[test]
fn voix_de_synthese_a_un_autre_gain() {
    // silence enregistré 10 dB plus bas : le bruit retrouve le même niveau une fois ramené au gain de la lecture
    let (a, b) = (synth_segments(40), synth_segments(30));
    let pa = analyze(&synth_speech(32000, &a, 150.0, 11), 32000, &a).unwrap();
    let pb = analyze(&synth_speech(32000, &b, 150.0, 11), 32000, &b).unwrap();
    assert!((pa.levels.noise_rms_db - pb.levels.noise_rms_db).abs() < 0.3, "{:?} / {:?}", pa.levels, pb.levels);
}

#[test]
fn json_aller_retour() {
    let segs = synth_segments(40);
    let p = analyze(&synth_speech(32000, &segs, 180.0, 5), 32000, &segs).unwrap();
    let text = serde_json::to_string(&p).unwrap();
    assert!(!text.contains("null,\"f0_low_hz\""), "{text}");
    let back: VoicePrint = serde_json::from_str(&text).unwrap();
    assert_eq!(back, p);
    assert_eq!(back.bands.len(), 51);
    assert_eq!(back.voice_db.len(), 51);
}

#[test]
fn enregistrements_inutilisables() {
    let x = vec![0.0f32; 16000 * 4];
    let segs = [seg(Phase::Silence, 0.0, 1.5, 40), seg(Phase::Reading, 1.5, 4.0, 40)];
    assert!(analyze(&x, 8000, &segs).is_err(), "fréquence trop basse");
    assert!(analyze(&x, 16000, &segs[..1]).is_err(), "pas de lecture");
    assert!(analyze(&x, 16000, &segs).is_err(), "lecture muette");
    assert!(analyze(&[], 16000, &segs).is_err(), "vide");
    let mut nan = Sig::new(16000.0, 4.0);
    nan.sine(1.4, 4.0, 300.0, 0.2);
    let mut y = nan.samples();
    y[30000] = f32::NAN;
    y[30001] = f32::INFINITY;
    assert!(analyze(&y, 16000, &segs).is_ok());
}

#[test]
fn sans_silence_ni_phrase_forte() {
    let segs = [seg(Phase::Reading, 0.0, 11.0, 40)];
    let full = synth_segments(40);
    let x = synth_speech(32000, &full, 130.0, 2);
    let p = analyze(&x[..11 * 32000], 32000, &segs).unwrap();
    assert!(take(&p, "no_silence") && take(&p, "no_loud"), "{:?}", p.warnings);
    assert!(p.hum.is_none());
    assert!(p.f0_hz.is_some());
    assert!(p.levels.noise_rms_db < p.levels.speech_p50_db - 20.0, "{:?}", p.levels);
}

#[test]
fn silence_avec_clics_ou_paroles() {
    // talk : (début, fin) d'une voyelle dite pendant le silence
    let build = |clicks: bool, talk: Option<(f64, f64)>| {
        let mut s = Sig::new(16000.0, 9.0);
        s.noise(0.0, 9.0, -70.0);
        let mut t = 3.4;
        while t + 0.5 < 9.0 {
            s.vowel(t, t + 0.35, 140.0, 8, -20.0);
            t += 0.5;
        }
        if clicks {
            for t in [0.8, 1.3, 1.9, 2.4] {
                s.thump(t, 3000.0, 0.05, 0.004);
            }
        }
        if let Some((a, b)) = talk {
            s.vowel(a, b, 140.0, 8, -20.0);
        }
        let segs = [seg(Phase::Silence, 0.0, 3.0, 40), seg(Phase::Reading, 3.0, 9.0, 40)];
        analyze(&s.samples(), 16000, &segs).unwrap_or_else(|e| panic!("{clicks} {talk:?} : {e}"))
    };
    let clean = build(false, None);
    assert!(!take(&clean, "no_silence"), "{:?}", clean.warnings);
    assert!((clean.levels.noise_rms_db + 70.0).abs() < 0.5, "{:?}", clean.levels);
    // des clics de clavier, ou quelques mots, ne changent pas le bruit mesuré
    for p in [build(true, None), build(false, Some((0.6, 1.4)))] {
        assert!(!take(&p, "no_silence"), "{:?}", p.warnings);
        assert!((p.levels.noise_rms_db - clean.levels.noise_rms_db).abs() < 0.3, "{:?} / {:?}", p.levels, clean.levels);
        assert!((p.levels.noise_peak_db - clean.levels.noise_peak_db).abs() < 0.5, "{:?}", p.levels);
    }
    // on a parlé pendant tout le silence : le bruit vient des pauses de la lecture
    let talked = build(false, Some((0.0, 3.0)));
    assert!(take(&talked, "no_silence"), "{:?}", talked.warnings);
    assert!((talked.levels.noise_rms_db + 70.0).abs() < 3.0, "{:?}", talked.levels);
}
