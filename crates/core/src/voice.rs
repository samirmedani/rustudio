//! Empreinte vocale : analyse d'un enregistrement de l'entrée « Chat Mic », traitements neutres, en trois temps
//! (silence, lecture d'un texte, phrase plus forte). Fonctions pures, testées sans matériel.
//!
//! Conventions : niveaux en dBFS, une sinusoïde pleine échelle vaut 0 dB ; spectres intégrés par bandes de
//! 1/6 d'octave (centres 1000 × 2^(k/6), de 49,6 Hz à 16 kHz) ; tout est ramené au gain de la lecture.
//!
//! Précisions :
//! - un niveau efficace (RMS) suit la convention AES17 : carré moyen × 2, en dB (une sinusoïde pleine échelle vaut
//!   0 dB, un bruit blanc pleine échelle aussi s'il a la même puissance) ; une crête vaut 20·log10(|x| max) ;
//! - un spectre donne la puissance de chaque bande (densité × largeur) : un bruit blanc monte de 3 dB par octave,
//!   un bruit rose est plat ; une bande sans donnée (au-delà de 95 % de la moitié de la fréquence
//!   d'échantillonnage) vaut FLOOR_DB ;
//! - la parole est repérée trame par trame (40 ms environ, Hann, recouvrement 50 %) : au moins 10 dB au-dessus du
//!   bruit mesuré pendant le silence et au-dessus de -60 dBFS, mesuré entre 80 Hz et 12 kHz.
//!
//! Valeurs relevées sur huit voix de synthèse françaises de macOS lisant le texte de l'assistant (crêtes à
//! -10 dBFS, bruit rose à -72 dBFS), pour caler les profils :
//! - fondamentale : voix d'homme 97 à 135 Hz, voix de femme 193 à 307 Hz ;
//! - parole : centile 50 de -24 à -21 dBFS, centile 95 de -19 à -16 ; crest_db 10 à 13 dB (+5 à 6 dB avec des
//!   plosives : la crête les inclut) ; range_db 19 à 32 dB ;
//! - sibilance.excess_db : voix naturelles 13 à 16 dB, voix par formants (sifflantes ternes) 2 à 8 dB, voix de
//!   synth_speech 19 à 20 dB ; ratio 0,03 à 0,17 ;
//! - plosives : aucune sans coup de souffle ; des coups de 35 ms à 50 Hz (crête -8 dBFS) donnent 41 à 51 dB ;
//! - room : 56 à 135 ms à sec, voir analyze pour les pièces ;
//! - hum : 50 Hz à -60 dBFS ressort de 35 dB ; à -90 dBFS il est ignoré.
//!
//! Durée : 80 ms environ pour 26 s à 48 kHz, 140 ms pour 40 s (compilation optimisée).
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

/// Première et dernière bande : 1000 × 2^(k/6) pour k de -26 (49,6 Hz) à 24 (16 kHz), 51 bandes.
pub const BAND_K_MIN: i32 = -26;
pub const BAND_K_MAX: i32 = 24;

/// Plancher des niveaux (dB) : silence numérique, ou bande sans donnée (trop haute pour la fréquence
/// d'échantillonnage).
pub const FLOOR_DB: f64 = -120.0;

/// Centres des bandes d'analyse (Hz).
pub fn band_centers() -> Vec<f64> {
    (BAND_K_MIN..=BAND_K_MAX).map(|k| 1000.0 * 2f64.powf(k as f64 / 6.0)).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Silence,
    Reading,
    Loud,
}

/// Une étape de l'enregistrement, en secondes depuis son premier échantillon, avec le gain du micro à ce moment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub phase: Phase,
    pub start_s: f64,
    pub end_s: f64,
    pub gain_db: i32,
}

/// Niveaux en dBFS, au gain de la lecture.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Levels {
    /// trames RMS de 50 ms pendant la parole : centiles 10, 50, 90 et 95
    pub speech_p10_db: f64,
    pub speech_p50_db: f64,
    pub speech_p90_db: f64,
    pub speech_p95_db: f64,
    /// crête d'échantillon pendant la lecture
    pub peak_db: f64,
    /// crête d'échantillon pendant la phrase forte (ramenée au gain de la lecture)
    pub loud_peak_db: Option<f64>,
    /// crête moins RMS moyen de la parole
    pub crest_db: f64,
    /// écart entre les centiles 90 et 10 de la parole : la dynamique de la voix
    pub range_db: f64,
    /// silence : RMS global et centile 95 des trames de 50 ms
    pub noise_rms_db: f64,
    pub noise_peak_db: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Sibilance {
    /// part des trames de parole qui sont des sifflantes (s, ch, z, j)
    pub ratio: f64,
    /// combien les sifflantes ressortent (dB), voir la documentation de analyze
    pub excess_db: f64,
    /// fréquence où les sifflantes sont les plus fortes
    pub peak_hz: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Plosives {
    /// coups de souffle dans les graves (p, b) pendant la lecture
    pub count: u32,
    pub per_minute: f64,
    /// le plus fort, en dB au-dessus des graves habituels de la voix
    pub worst_db: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hum {
    /// 50 ou 60 Hz (secteur)
    pub hz: f64,
    /// dépassement du ronflement au-dessus du bruit voisin (dB)
    pub level_db: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Room {
    /// réverbération estimée sur les fins de mots (ms), si la mesure est assez sûre
    pub rt60_ms: Option<f64>,
    /// 0 à 1
    pub confidence: f64,
}

/// Résultat de l'analyse : ce que le moteur garde et ce que l'interface affiche.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VoicePrint {
    pub version: u32,
    pub rate: u32,
    /// gain du micro pendant la lecture : niveaux et spectres sont exprimés à ce gain
    pub gain_db: i32,
    /// centres des bandes (Hz, arrondis au centième), voir band_centers
    pub bands: Vec<f64>,
    /// spectre moyen de la parole (dBFS par bande)
    pub voice_db: Vec<f64>,
    /// spectre du silence (dBFS par bande), ramené au gain de la lecture
    pub noise_db: Vec<f64>,
    /// spectre moyen des sifflantes, s'il y en a
    pub sibilant_db: Option<Vec<f64>>,
    /// fondamentale de la voix (Hz) : médiane, centiles 10 et 90
    pub f0_hz: Option<f64>,
    pub f0_low_hz: Option<f64>,
    pub f0_high_hz: Option<f64>,
    /// secondes de parole détectées pendant la lecture
    pub speech_s: f64,
    pub levels: Levels,
    pub sibilance: Sibilance,
    pub plosives: Plosives,
    pub hum: Option<Hum>,
    pub room: Room,
    /// un échantillon a touché la pleine échelle pendant la lecture ou la phrase forte
    pub clipped: bool,
    /// codes : "too_quiet", "too_short", "noisy", "clipped", "no_silence", "no_loud"
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------------------------------------------
// Réglages de l'analyse
// ---------------------------------------------------------------------------------------------------------------

const VERSION: u32 = 1;
const MIN_RATE: u32 = 16_000;
const MAX_RATE: u32 = 96_000;
/// début du silence ignoré (bruit du démarrage de l'enregistrement), fin ignorée (on commence à lire un peu tôt)
const SILENCE_TRIM_START_S: f64 = 0.3;
const SILENCE_TRIM_END_S: f64 = 0.15;
const MIN_SILENCE_S: f64 = 1.0;
/// une trame du silence qui dépasse d'autant son centile 20 n'est pas du silence (parole, clavier)
const NOT_SILENCE_DB: f64 = 15.0;
/// il faut au moins une seconde de vrai silence
const MIN_QUIET_S: f64 = 1.0;
/// le silence ne peut pas dépasser d'autant les pauses les plus calmes de la lecture
const SILENCE_OVER_PAUSES_DB: f64 = 10.0;
const SPEECH_OVER_NOISE_DB: f64 = 10.0;
const SPEECH_MIN_DB: f64 = -60.0;
const MIN_SPEECH_FRAMES: usize = 10;
const DETECT_BAND: (f64, f64) = (80.0, 12_000.0);
const SIB_BAND: (f64, f64) = (4_000.0, 12_000.0);
const LOWMID_BAND: (f64, f64) = (80.0, 4_000.0);
const LF_BAND: (f64, f64) = (20.0, 100.0);
const MID_BAND: (f64, f64) = (100.0, 2_000.0);
/// sifflante : énergie 4–12 kHz dominante, pas plus de 25 dB sous la médiane de la parole
const SIB_WITHIN_DB: f64 = 25.0;
/// plosive : 20–100 Hz au moins 15 dB au-dessus de leur médiane pendant la parole, et plus forts que 100 Hz–2 kHz ;
/// au-delà de 100 Hz, la fondamentale d'une voix aiguë qui descend ressemblerait à un coup de souffle
const PLOSIVE_OVER_DB: f64 = 15.0;
const PLOSIVE_GAP_S: f64 = 0.15;
/// plus long, ce n'est pas une plosive (pied de micro heurté, camion qui passe)
const PLOSIVE_MAX_S: f64 = 0.2;
const LEVEL_FRAME_S: f64 = 0.05;
const CLIP_LEVEL: f32 = 0.999;
const TOO_QUIET_P95_DB: f64 = -40.0;
const MIN_SPEECH_S: f64 = 4.0;
const NOISY_SNR_DB: f64 = 15.0;
const HUM_OVER_DB: f64 = 10.0;
/// en dessous, un ronflement est inaudible : ignoré
const HUM_MIN_DB: f64 = -80.0;
const F0_MIN_HZ: f64 = 60.0;
const F0_MAX_HZ: f64 = 400.0;
const YIN_THRESHOLD: f64 = 0.15;

// ---------------------------------------------------------------------------------------------------------------
// Analyse
// ---------------------------------------------------------------------------------------------------------------

/// Analyse un enregistrement mono. Utilise la dernière étape « reading » pour la voix, le silence pour le bruit,
/// la phrase forte pour la crête. Erreur si l'enregistrement est inutilisable (aucune lecture, format invalide).
///
/// Ce que donnent les mesures :
/// - voice_db, noise_db, sibilant_db : puissance par bande de 1/6 d'octave (dBFS, convention du module) ;
/// - sibilance.excess_db : bande la plus forte des sifflantes entre 4 et 11 kHz, moins la moyenne de voice_db entre
///   1 et 3 kHz ; sans sifflante repérée, calculé sur voice_db (valeur basse) ;
/// - plosives : trames dont les graves (20–100 Hz) dépassent d'au moins 15 dB leur médiane pendant la parole et
///   l'emportent sur 100 Hz–2 kHz ; deux coups à moins de 150 ms n'en font qu'un, un coup de plus de 200 ms n'est
///   pas une plosive ; per_minute rapporté à la durée de la lecture ;
/// - hum : pic à 50/100/150 Hz ou 60/120/180 Hz, 10 dB au-dessus de son voisinage (6 à 20 Hz de part et
///   d'autre) et au-dessus de -80 dBFS ;
/// - room : temps de décroissance apparent des fins de mots entre 500 Hz et 4 kHz (médiane des descentes d'au moins
///   15 dB, ramenées à 60 dB) : ce que l'on entend sur l'enregistrement, la pièce et la distance au micro mêlées.
///   Sur des voix de synthèse : 60 à 140 ms à sec ; 190 à 260 ms pour une pièce de 0,25 à 0,4 s ; 400 à 520 ms
///   pour 0,5 à 0,6 s (micro à distance moyenne) ; un micro très proche raccourcit la mesure. rt60_ms absent avec
///   moins de 4 descentes ou une confiance sous 0,3 ;
/// - levels.noise_* et noise_db : trames du silence, hors celles qui dépassent de 15 dB son centile 20 (parole,
///   clic) ; il en faut au moins une seconde, sinon le bruit vient des passages les plus calmes de la lecture
///   (avertissement no_silence).
pub fn analyze(samples: &[f32], rate: u32, segments: &[Segment]) -> Result<VoicePrint, String> {
    if !(MIN_RATE..=MAX_RATE).contains(&rate) {
        return Err(format!("Fréquence d’échantillonnage non prise en charge : {rate} Hz (16 à 96 kHz)."));
    }
    if samples.is_empty() {
        return Err("Enregistrement vide.".into());
    }
    let fs = rate as f64;
    let raw: Vec<f32> = samples.iter().map(|s| if s.is_finite() { *s } else { 0.0 }).collect();
    let reading_seg = segments
        .iter()
        .rev()
        .find(|s| s.phase == Phase::Reading && span(s, fs, raw.len()).len() > 0)
        .ok_or_else(|| "Aucune lecture dans l’enregistrement.".to_string())?;
    let read = span(reading_seg, fs, raw.len());
    let gain = reading_seg.gain_db;
    let x = dc_block(&raw, fs);
    let xh = highpass(&x, fs, 80.0);
    let mut stft = Stft::new(fs);
    if read.len() < stft.n * 2 {
        return Err("Lecture trop courte pour être analysée.".into());
    }

    // 1. le silence : bruit de la pièce, au gain de la lecture
    let read_frames = stft.frames(&x, read.a, read.b);
    let read_levels = level_frames(&x, &xh, fs, read.a, read.b);
    // un « silence » plus fort que les pauses de la lecture n'en est pas un (on a parlé tout du long)
    let pauses_db = percentile(&sorted(&read_frames.iter().map(|f| ms_db(f.detect)).collect::<Vec<_>>()), 0.05);
    let silence = pick_silence(segments, fs, raw.len())
        .and_then(|s| noise_from_silence(&mut stft, &x, &xh, fs, &s, gain))
        .filter(|n| n.detect_db <= pauses_db + SILENCE_OVER_PAUSES_DB);
    let noise = match &silence {
        Some(n) => n.clone(),
        None => noise_from_reading(&read_frames, &read_levels, stft.bands.len()),
    };

    // 2. la parole
    let thr = (noise.detect_db + SPEECH_OVER_NOISE_DB).max(SPEECH_MIN_DB);
    let speech: Vec<bool> = read_frames.iter().map(|f| ms_db(f.detect) >= thr).collect();
    let n_speech = speech.iter().filter(|s| **s).count();
    if n_speech < MIN_SPEECH_FRAMES {
        return Err("Je n’ai pas entendu ta voix pendant la lecture : vérifie que ton micro n’est pas coupé et parle plus près."
            .into());
    }
    let speech_frames: Vec<&Frame> = read_frames.iter().zip(&speech).filter(|(_, s)| **s).map(|(f, _)| f).collect();
    let voice_ms = mean_bands(&speech_frames, stft.bands.len());
    let speech_med = median(&speech_frames.iter().map(|f| ms_db(f.detect)).collect::<Vec<_>>());

    // 3. les sifflantes
    let sib_frames: Vec<&Frame> = speech_frames
        .iter()
        .copied()
        .filter(|f| f.sib > f.lowmid && ms_db(f.detect) >= speech_med - SIB_WITHIN_DB)
        .collect();
    let sib_ms = (!sib_frames.is_empty()).then(|| mean_bands(&sib_frames, stft.bands.len()));
    let voice_db = stft.to_db(&voice_ms);
    let sib_db = sib_ms.as_ref().map(|m| stft.to_db(m));
    let sibilance = sibilance(&stft.centers, &voice_db, sib_db.as_deref(), sib_frames.len(), n_speech);

    // 4. les plosives
    let plosives = plosives(&read_frames, &speech, stft.hop as f64 / fs, read.len() as f64 / fs);

    // 5. les niveaux (trames de 50 ms)
    let thr50 = (noise.level_detect_db + SPEECH_OVER_NOISE_DB).max(SPEECH_MIN_DB);
    let mut sp50: Vec<f64> = read_levels.iter().filter(|l| l.detect_db >= thr50).map(|l| l.ms).collect();
    if sp50.is_empty() {
        let med = median(&read_levels.iter().map(|l| l.detect_db).collect::<Vec<_>>());
        sp50 = read_levels.iter().filter(|l| l.detect_db >= med).map(|l| l.ms).collect();
    }
    let sp50_db = sorted(&sp50.iter().map(|m| ms_db(*m)).collect::<Vec<_>>());
    let mean_db = ms_db(sp50.iter().sum::<f64>() / sp50.len().max(1) as f64);
    let peak_db = amp_db(peak(&raw, read.a, read.b));
    let loud =
        segments.iter().rev().find(|s| s.phase == Phase::Loud).map(|s| span(s, fs, raw.len())).filter(|s| s.len() > 0);
    let loud_peak_db = loud.as_ref().and_then(|l| {
        let corr = gain as f64 - l.gain as f64;
        let n = level_frames(&x, &xh, fs, l.a, l.b).iter().filter(|f| f.detect_db + corr >= thr50).count();
        (n >= 3).then(|| amp_db(peak(&raw, l.a, l.b)) + corr)
    });
    let clipped = clip(&raw, read.a, read.b) || loud.as_ref().is_some_and(|l| clip(&raw, l.a, l.b));
    let levels = Levels {
        speech_p10_db: r1(percentile(&sp50_db, 0.10)),
        speech_p50_db: r1(percentile(&sp50_db, 0.50)),
        speech_p90_db: r1(percentile(&sp50_db, 0.90)),
        speech_p95_db: r1(percentile(&sp50_db, 0.95)),
        peak_db: r1(peak_db),
        loud_peak_db: loud_peak_db.map(r1),
        crest_db: r1(peak_db - mean_db),
        range_db: r1(percentile(&sp50_db, 0.90) - percentile(&sp50_db, 0.10)),
        noise_rms_db: r1(noise.rms_db),
        noise_peak_db: r1(noise.peak_db),
    };

    // 6. la fondamentale, le ronflement, la pièce
    let f0 = fundamental(&x, fs, read.a, read.b, &speech, stft.n, stft.hop);
    let hum = silence.as_ref().and_then(|n| hum(&x, &xh, fs, n.a, n.b, n.level_detect_db - n.corr));
    let read_env = band_env(&x, fs, read.a, read.b);
    let silence_env = silence.as_ref().map(|n| (band_env(&x, fs, n.a, n.b), n.corr)).filter(|(e, _)| !e.is_empty());
    let floor = match silence_env {
        Some((env, corr)) => {
            let low = percentile(&sorted(&env), 0.2);
            median(&env.iter().copied().filter(|e| *e < low + NOT_SILENCE_DB).collect::<Vec<_>>()) + corr
        }
        None => percentile(&sorted(&read_env), 0.05),
    };
    let room = room(&read_env, floor);

    let speech_s = n_speech as f64 * stft.hop as f64 / fs;
    let mut warnings = vec![];
    if levels.speech_p95_db < TOO_QUIET_P95_DB {
        warnings.push("too_quiet");
    }
    if speech_s < MIN_SPEECH_S {
        warnings.push("too_short");
    }
    if levels.speech_p50_db < levels.noise_rms_db + NOISY_SNR_DB {
        warnings.push("noisy");
    }
    if clipped {
        warnings.push("clipped");
    }
    if silence.is_none() {
        warnings.push("no_silence");
    }
    if loud_peak_db.is_none() {
        warnings.push("no_loud");
    }
    Ok(VoicePrint {
        version: VERSION,
        rate,
        gain_db: gain,
        bands: stft.centers.iter().map(|c| (c * 100.0).round() / 100.0).collect(),
        voice_db: voice_db.iter().map(|v| r1(*v)).collect(),
        noise_db: stft.to_db(&noise.spectrum).iter().map(|v| r1(*v)).collect(),
        sibilant_db: sib_db.map(|v| v.iter().map(|x| r1(*x)).collect()),
        f0_hz: f0.map(|f| r1(f.0)),
        f0_low_hz: f0.map(|f| r1(f.1)),
        f0_high_hz: f0.map(|f| r1(f.2)),
        speech_s: r2(speech_s),
        levels,
        sibilance,
        plosives,
        hum,
        room,
        clipped,
        warnings: warnings.into_iter().map(String::from).collect(),
    })
}

// ---------------------------------------------------------------------------------------------------------------
// Étapes et bruit
// ---------------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct Span {
    a: usize,
    b: usize,
    gain: i32,
}
impl Span {
    fn len(&self) -> usize {
        self.b.saturating_sub(self.a)
    }
}

fn span(s: &Segment, fs: f64, len: usize) -> Span {
    let at = |t: f64| if t.is_finite() { ((t.max(0.0) * fs).round() as usize).min(len) } else { 0 };
    let (a, b) = (at(s.start_s), at(s.end_s));
    Span { a, b: b.max(a), gain: s.gain_db }
}

/// Le dernier silence assez long (après avoir retiré son début et sa fin), sinon le plus long ; noise_from_silence
/// décide s'il est utilisable.
fn pick_silence(segments: &[Segment], fs: f64, len: usize) -> Option<Span> {
    let trimmed: Vec<Span> = segments
        .iter()
        .filter(|s| s.phase == Phase::Silence)
        .map(|s| {
            let sp = span(s, fs, len);
            let a = sp.a + (SILENCE_TRIM_START_S * fs) as usize;
            let b = sp.b.saturating_sub((SILENCE_TRIM_END_S * fs) as usize);
            Span { a, b: b.max(a), gain: sp.gain }
        })
        .collect();
    let min = (MIN_SILENCE_S * fs) as usize;
    trimmed.iter().rev().find(|s| s.len() >= min).or_else(|| trimmed.iter().max_by_key(|s| s.len())).copied()
}

/// Bruit de la pièce, ramené au gain de la lecture (corr en dB).
#[derive(Clone, Debug)]
struct Noise {
    /// médiane des trames d'analyse (bande de détection), en dB
    detect_db: f64,
    /// carré moyen par bande
    spectrum: Vec<f64>,
    rms_db: f64,
    peak_db: f64,
    /// médiane des trames de 50 ms filtrées à 80 Hz
    level_detect_db: f64,
    a: usize,
    b: usize,
    corr: f64,
}

fn noise_from_silence(stft: &mut Stft, x: &[f32], xh: &[f32], fs: f64, s: &Span, gain: i32) -> Option<Noise> {
    let corr = gain as f64 - s.gain as f64;
    let lin = 10f64.powf(corr / 10.0);
    let frames = stft.frames(x, s.a, s.b);
    if frames.len() < 3 {
        return None;
    }
    let quiet_ref = percentile(&sorted(&frames.iter().map(|f| ms_db(f.detect)).collect::<Vec<_>>()), 0.2);
    let keep: Vec<&Frame> = frames.iter().filter(|f| ms_db(f.detect) < quiet_ref + NOT_SILENCE_DB).collect();
    if (keep.len() as f64) * (stft.hop as f64) < MIN_QUIET_S * fs {
        return None;
    }
    let detect_db = median(&keep.iter().map(|f| ms_db(f.detect)).collect::<Vec<_>>()) + corr;
    let spectrum = mean_bands(&keep, stft.bands.len()).iter().map(|m| m * lin).collect();
    let lv = level_frames(x, xh, fs, s.a, s.b);
    if lv.is_empty() {
        return None;
    }
    let ref_h = percentile(&sorted(&lv.iter().map(|l| l.detect_db).collect::<Vec<_>>()), 0.2);
    let quiet: Vec<&Level> = lv.iter().filter(|l| l.detect_db < ref_h + NOT_SILENCE_DB).collect();
    let full = sorted(&quiet.iter().map(|l| ms_db(l.ms)).collect::<Vec<_>>());
    Some(Noise {
        detect_db,
        spectrum,
        rms_db: ms_db(quiet.iter().map(|l| l.ms).sum::<f64>() / quiet.len().max(1) as f64) + corr,
        peak_db: percentile(&full, 0.95) + corr,
        level_detect_db: median(&quiet.iter().map(|l| l.detect_db).collect::<Vec<_>>()) + corr,
        a: s.a,
        b: s.b,
        corr,
    })
}

/// Sans silence utilisable : les passages les plus calmes de la lecture (estimation grossière, qui suppose quelques
/// pauses pendant la lecture).
fn noise_from_reading(frames: &[Frame], levels: &[Level], nb: usize) -> Noise {
    let dbs = sorted(&frames.iter().map(|f| ms_db(f.detect)).collect::<Vec<_>>());
    let detect_db = percentile(&dbs, 0.05);
    let low_db = percentile(&dbs, 0.10);
    let quiet: Vec<&Frame> = frames.iter().filter(|f| ms_db(f.detect) <= low_db).collect();
    let mut lv: Vec<&Level> = levels.iter().collect();
    lv.sort_by(|a, b| a.detect_db.total_cmp(&b.detect_db));
    let low = &lv[..(lv.len() / 20).max(1).min(lv.len())];
    let full = sorted(&low.iter().map(|l| ms_db(l.ms)).collect::<Vec<_>>());
    Noise {
        detect_db,
        spectrum: mean_bands(&quiet, nb),
        rms_db: ms_db(low.iter().map(|l| l.ms).sum::<f64>() / low.len().max(1) as f64),
        peak_db: full.last().copied().unwrap_or(FLOOR_DB),
        level_detect_db: low.iter().map(|l| l.detect_db).fold(FLOOR_DB, f64::max),
        a: 0,
        b: 0,
        corr: 0.0,
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Mesures
// ---------------------------------------------------------------------------------------------------------------

fn sibilance(centers: &[f64], voice_db: &[f64], sib_db: Option<&[f64]>, n_sib: usize, n_speech: usize) -> Sibilance {
    let presence: Vec<f64> = centers
        .iter()
        .zip(voice_db)
        .filter(|(c, v)| (1000.0..=3000.0).contains(*c) && **v > FLOOR_DB)
        .map(|(_, v)| *v)
        .collect();
    let presence = if presence.is_empty() { FLOOR_DB } else { presence.iter().sum::<f64>() / presence.len() as f64 };
    let spec = sib_db.unwrap_or(voice_db);
    let idx: Vec<usize> =
        (0..centers.len()).filter(|&i| (4000.0..=11_000.0).contains(&centers[i]) && spec[i] > FLOOR_DB).collect();
    let Some(&best) = idx.iter().max_by(|a, b| spec[**a].total_cmp(&spec[**b])) else {
        return Sibilance { ratio: 0.0, excess_db: 0.0, peak_hz: None };
    };
    // sommet affiné par une parabole sur les bandes voisines (en octaves)
    let mut peak_hz = centers[best];
    if best > 0 && best + 1 < centers.len() && spec[best - 1] > FLOOR_DB && spec[best + 1] > FLOOR_DB {
        let (a, b, c) = (spec[best - 1], spec[best], spec[best + 1]);
        let den = a - 2.0 * b + c;
        if den.abs() > 1e-9 {
            let shift = (0.5 * (a - c) / den).clamp(-0.5, 0.5);
            peak_hz *= 2f64.powf(shift / 6.0);
        }
    }
    Sibilance {
        ratio: r3(n_sib as f64 / n_speech.max(1) as f64),
        excess_db: r1(spec[best] - presence),
        peak_hz: Some(peak_hz.round()),
    }
}

fn plosives(frames: &[Frame], speech: &[bool], hop_s: f64, reading_s: f64) -> Plosives {
    let lf: Vec<f64> = frames.iter().map(|f| ms_db(f.lf)).collect();
    let ref_lf = median(&lf.iter().zip(speech).filter(|(_, s)| **s).map(|(l, _)| *l).collect::<Vec<_>>());
    // (première trame, dernière trame, dépassement le plus fort)
    let mut events: Vec<(usize, usize, f64)> = vec![];
    for (i, f) in frames.iter().enumerate() {
        let over = lf[i] - ref_lf;
        if over >= PLOSIVE_OVER_DB && f.lf > f.mid {
            match events.last_mut() {
                Some(e) if (i - e.1) as f64 * hop_s <= PLOSIVE_GAP_S => {
                    e.1 = i;
                    e.2 = e.2.max(over);
                }
                _ => events.push((i, i, over)),
            }
        }
    }
    events.retain(|e| (e.1 - e.0) as f64 * hop_s <= PLOSIVE_MAX_S);
    let worst = events.iter().map(|e| e.2).fold(0.0, f64::max);
    let count = events.len() as u32;
    Plosives {
        count,
        per_minute: r1(if reading_s > 0.0 { count as f64 * 60.0 / reading_s } else { 0.0 }),
        worst_db: r1(worst),
    }
}

/// Fondamentale (médiane, centiles 10 et 90) par YIN, sur le signal ramené vers 8 kHz et filtré sous 1 kHz,
/// seulement dans les trames de parole.
fn fundamental(
    x: &[f32],
    fs: f64,
    a: usize,
    b: usize,
    speech: &[bool],
    n: usize,
    hop: usize,
) -> Option<(f64, f64, f64)> {
    let (y, fd, d) = lowpass_decimate(x, fs, a, b);
    let w = (0.03 * fd).round() as usize;
    let tmin = (fd / F0_MAX_HZ).floor() as usize;
    let tmax = (fd / F0_MIN_HZ).ceil() as usize;
    let step = (0.01 * fd).round().max(1.0) as usize;
    let mut scratch = vec![0.0; tmax + 2];
    let mut f0s = vec![];
    let mut p = 0;
    while p + w + tmax + 1 < y.len() {
        let c = a + (p + w / 2) * d;
        let core = c.checked_sub(a + n / 4).map(|o| o / hop);
        if core.is_some_and(|i| speech.get(i).copied().unwrap_or(false)) {
            if let Some(f) = yin(&y[p..p + w + tmax + 1], fd, w, tmin, tmax, &mut scratch) {
                f0s.push(f);
            }
        }
        p += step;
    }
    if f0s.len() < 10 {
        return None;
    }
    // sauts d'octave isolés écartés
    let m = median(&f0s);
    let kept = sorted(&f0s.into_iter().filter(|f| *f > 0.6 * m && *f < 1.7 * m).collect::<Vec<_>>());
    if kept.len() < 10 {
        return None;
    }
    Some((percentile(&kept, 0.5), percentile(&kept, 0.1), percentile(&kept, 0.9)))
}

fn yin(seg: &[f64], fd: f64, w: usize, tmin: usize, tmax: usize, dn: &mut [f64]) -> Option<f64> {
    let energy: f64 = seg[..w].iter().map(|v| v * v).sum();
    if energy <= 1e-14 {
        return None;
    }
    let mut cum = 0.0;
    dn[0] = 1.0;
    for tau in 1..=tmax + 1 {
        let d: f64 = seg[..w].iter().zip(&seg[tau..tau + w]).map(|(p, q)| (p - q) * (p - q)).sum();
        cum += d;
        dn[tau] = if cum > 0.0 { d * tau as f64 / cum } else { 1.0 };
    }
    let mut tau = tmin.max(2);
    while tau <= tmax {
        if dn[tau] < YIN_THRESHOLD {
            while tau < tmax && dn[tau + 1] < dn[tau] {
                tau += 1;
            }
            let (p, q, r) = (dn[tau - 1], dn[tau], dn[tau + 1]);
            let den = p - 2.0 * q + r;
            let shift = if den.abs() > 1e-12 { (0.5 * (p - r) / den).clamp(-0.5, 0.5) } else { 0.0 };
            return Some(fd / (tau as f64 + shift));
        }
        tau += 1;
    }
    None
}

/// Filtre passe-bas (sinus cardinal fenêtré de Blackman, coupure 1 kHz) puis décimation vers 8 kHz environ.
fn lowpass_decimate(x: &[f32], fs: f64, a: usize, b: usize) -> (Vec<f64>, f64, usize) {
    let d = (fs / 8000.0).floor().max(1.0) as usize;
    let fd = fs / d as f64;
    let half = ((fs / 500.0).round() as usize).max(8);
    let fc = 1000.0 / fs;
    let len = 2 * half;
    let mut h: Vec<f64> = (0..=len)
        .map(|j| {
            let m = j as f64 - half as f64;
            let sinc = if m == 0.0 { 2.0 * fc } else { (2.0 * PI * fc * m).sin() / (PI * m) };
            let t = j as f64 / len as f64;
            sinc * (0.42 - 0.5 * (2.0 * PI * t).cos() + 0.08 * (4.0 * PI * t).cos())
        })
        .collect();
    let sum: f64 = h.iter().sum();
    h.iter_mut().for_each(|v| *v /= sum);
    let count = (b - a) / d;
    let y = (0..count)
        .map(|i| {
            let c = a + i * d;
            if c >= half && c + half < x.len() {
                x[c - half..=c + half].iter().zip(&h).map(|(s, k)| *s as f64 * k).sum()
            } else {
                h.iter()
                    .enumerate()
                    .filter_map(|(j, k)| (c + j).checked_sub(half).filter(|i| *i < x.len()).map(|i| x[i] as f64 * k))
                    .sum()
            }
        })
        .collect();
    (y, fd, d)
}

/// Ronflement du secteur, sur le silence (spectre fin moyenné, résolution 1 à 2 Hz). Seuls comptent les morceaux
/// vraiment calmes : au-dessus de 80 Hz (où le ronflement ne compte pas), pas plus de 3 dB au-dessus du bruit
/// habituel du silence (quiet_db, au gain du silence).
fn hum(x: &[f32], xh: &[f32], fs: f64, a: usize, b: usize, quiet_db: f64) -> Option<Hum> {
    let len = b.saturating_sub(a);
    let mut nh = 1usize << ((fs * 0.7).log2().round() as u32);
    while nh > len && nh > 1024 {
        nh >>= 1;
    }
    if nh > len || (nh as f64) < fs * 0.25 {
        return None;
    }
    let mut fft = RealFft::new(nh);
    let win = hann(nh);
    let s2: f64 = win.iter().map(|w| w * w).sum();
    let mut avg = vec![0.0; nh / 2 + 1];
    let mut frame = vec![0.0; nh];
    let mut pow = vec![0.0; nh / 2 + 1];
    let mut count = 0;
    let mut s = a;
    while s + nh <= b {
        let level = ms_db(xh[s..s + nh].iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / nh as f64);
        if level <= quiet_db + 3.0 {
            for (i, f) in frame.iter_mut().enumerate() {
                *f = x[s + i] as f64 * win[i];
            }
            fft.power(&frame, &mut pow);
            avg.iter_mut().zip(&pow).for_each(|(m, p)| *m += p);
            count += 1;
        }
        s += nh / 2;
    }
    if count == 0 {
        return None;
    }
    avg.iter_mut().for_each(|m| *m /= count as f64);
    let df = fs / nh as f64;
    let cal = 2.0 / (nh as f64 * s2);
    let mut best: Option<Hum> = None;
    for base in [50.0, 60.0] {
        let mut level: Option<f64> = None;
        for h in 1..=3 {
            let f = base * h as f64;
            let kc = f / df;
            let reach = 2.0f64.max(1.5 / df).ceil() as usize;
            let k0 = (kc.round() as usize).saturating_sub(reach).max(1);
            let k1 = (kc.round() as usize + reach).min(avg.len() - 1);
            let peak = avg[k0..=k1].iter().copied().fold(0.0, f64::max);
            let sine_ms: f64 = avg[k0..=k1].iter().sum::<f64>() * cal;
            let around: Vec<f64> = (1..avg.len())
                .filter(|k| {
                    let dist = (*k as f64 * df - f).abs();
                    (6.0..=20.0).contains(&dist)
                })
                .map(|k| avg[k])
                .collect();
            if around.is_empty() {
                continue;
            }
            let base_level = median(&around);
            if base_level <= 0.0 || peak <= 0.0 {
                continue;
            }
            let over = 10.0 * (peak / base_level).log10();
            if over >= HUM_OVER_DB && ms_db(sine_ms) >= HUM_MIN_DB {
                level = Some(level.map_or(over, |l: f64| l.max(over)));
            }
        }
        if let Some(l) = level {
            if best.as_ref().is_none_or(|b| l > b.level_db) {
                best = Some(Hum { hz: base, level_db: l });
            }
        }
    }
    best.map(|h| Hum { hz: h.hz, level_db: r1(h.level_db) })
}

/// Enveloppe 500 Hz–4 kHz : fenêtres de 20 ms tous les 5 ms, en dB.
fn band_env(x: &[f32], fs: f64, a: usize, b: usize) -> Vec<f64> {
    let pre = ((0.05 * fs) as usize).min(a);
    let mut hp = Biquad::highpass(fs, 500.0, std::f64::consts::FRAC_1_SQRT_2);
    let mut lp = Biquad::lowpass(fs, 4000.0, std::f64::consts::FRAC_1_SQRT_2);
    let sq: Vec<f64> = x[a - pre..b]
        .iter()
        .map(|v| {
            let y = lp.run(hp.run(*v as f64));
            y * y
        })
        .skip(pre)
        .collect();
    let win = ((0.020 * fs).round() as usize).max(1);
    let hop = ((0.005 * fs).round() as usize).max(1);
    let mut acc = vec![0.0; sq.len() + 1];
    for (i, v) in sq.iter().enumerate() {
        acc[i + 1] = acc[i] + v;
    }
    let mut env = vec![];
    let mut s = 0;
    while s + win <= sq.len() {
        env.push(ms_db((acc[s + win] - acc[s]) / win as f64));
        s += hop;
    }
    env
}

const ENV_DT: f64 = 0.005;

/// Descentes après les fins de mots : depuis un sommet (25 dB au moins au-dessus du bruit), la suite des fenêtres
/// qui ne remontent pas de plus de 2 dB ; pente mesurée de 5 dB sous le sommet jusqu'au point le plus bas, si la
/// descente fait au moins 15 dB et reste droite (r² ≥ 0,8). Temps ramenés à 60 dB, en ms.
fn decay_times(env: &[f64], floor: f64) -> Vec<f64> {
    let n = env.len();
    let mut out = vec![];
    let mut i = 5;
    while i + 5 < n {
        let e = env[i];
        if e < floor + 25.0 || env[i - 5..=i + 5].iter().any(|v| *v > e) {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let mut low = (e, i);
        while j < n && env[j] <= low.0 + 2.0 && env[j] > floor + 3.0 {
            if env[j] < low.0 {
                low = (env[j], j);
            }
            j += 1;
        }
        if let Some(k0) = (i + 1..=low.1).find(|&k| env[k] <= e - 5.0) {
            if low.1 > k0 + 2 && env[k0] - low.0 >= 15.0 {
                let pts: Vec<(f64, f64)> = (k0..=low.1).map(|k| (k as f64 * ENV_DT, env[k])).collect();
                if let Some((slope, r2)) = linfit(&pts) {
                    if slope < 0.0 && r2 >= 0.8 {
                        out.push(-60.0 / slope * 1000.0);
                    }
                }
            }
        }
        i = j.max(i + 1);
    }
    out
}

fn room(env: &[f64], floor: f64) -> Room {
    let d = sorted(&decay_times(env, floor));
    let count = (d.len() as f64 / 10.0).min(1.0);
    if d.len() < 4 {
        return Room { rt60_ms: None, confidence: r2(count * 0.5) };
    }
    let mid = percentile(&d, 0.5);
    let spread = (percentile(&d, 0.75) - percentile(&d, 0.25)) / mid;
    let confidence = count * (1.0 - 0.5 * (spread - 0.3).clamp(0.0, 1.0));
    Room { rt60_ms: (confidence >= 0.3).then(|| mid.clamp(30.0, 3000.0).round()), confidence: r2(confidence) }
}

fn linfit(pts: &[(f64, f64)]) -> Option<(f64, f64)> {
    let n = pts.len() as f64;
    let (sx, sy) = pts.iter().fold((0.0, 0.0), |(a, b), (x, y)| (a + x, b + y));
    let (mx, my) = (sx / n, sy / n);
    let (mut sxx, mut sxy, mut syy) = (0.0, 0.0, 0.0);
    for (x, y) in pts {
        sxx += (x - mx) * (x - mx);
        sxy += (x - mx) * (y - my);
        syy += (y - my) * (y - my);
    }
    if sxx <= 0.0 || syy <= 0.0 {
        return None;
    }
    let slope = sxy / sxx;
    Some((slope, sxy * sxy / (sxx * syy)))
}

// ---------------------------------------------------------------------------------------------------------------
// Trames de niveau (50 ms)
// ---------------------------------------------------------------------------------------------------------------

struct Level {
    /// carré moyen, signal complet
    ms: f64,
    /// niveau du signal filtré à 80 Hz (détection)
    detect_db: f64,
}

fn level_frames(x: &[f32], xh: &[f32], fs: f64, a: usize, b: usize) -> Vec<Level> {
    let len = ((LEVEL_FRAME_S * fs).round() as usize).max(1);
    let hop = (len / 2).max(1);
    let mut out = vec![];
    let mut s = a;
    while s + len <= b {
        let ms = x[s..s + len].iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / len as f64;
        let mh = xh[s..s + len].iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / len as f64;
        out.push(Level { ms, detect_db: ms_db(mh) });
        s += hop;
    }
    out
}

fn peak(x: &[f32], a: usize, b: usize) -> f64 {
    x[a..b].iter().fold(0.0f32, |m, v| m.max(v.abs())) as f64
}

fn clip(x: &[f32], a: usize, b: usize) -> bool {
    x[a..b].iter().any(|v| v.abs() >= CLIP_LEVEL)
}

// ---------------------------------------------------------------------------------------------------------------
// Trames d'analyse spectrale
// ---------------------------------------------------------------------------------------------------------------

struct Frame {
    detect: f64,
    sib: f64,
    lowmid: f64,
    lf: f64,
    mid: f64,
    /// carré moyen par bande de 1/6 d'octave
    bands: Vec<f64>,
}

type Weights = Vec<(usize, f64)>;

struct Stft {
    n: usize,
    hop: usize,
    window: Vec<f64>,
    fft: RealFft,
    /// puissance d'un bin (une face) -> carré moyen
    cal: f64,
    centers: Vec<f64>,
    bands: Vec<Weights>,
    band_ok: Vec<bool>,
    detect: Weights,
    sib: Weights,
    lowmid: Weights,
    lf: Weights,
    mid: Weights,
    frame: Vec<f64>,
    power: Vec<f64>,
}

impl Stft {
    fn new(fs: f64) -> Stft {
        let n = 1usize << ((0.04 * fs).log2().round() as u32);
        let window = hann(n);
        let s2: f64 = window.iter().map(|w| w * w).sum();
        let df = fs / n as f64;
        let top = 0.95 * fs / 2.0;
        let w = |lo: f64, hi: f64| weights(lo.min(top), hi.min(top), df, n / 2);
        let centers = band_centers();
        let bands = centers.iter().map(|c| w(c * 2f64.powf(-1.0 / 12.0), c * 2f64.powf(1.0 / 12.0))).collect();
        let band_ok = centers.iter().map(|c| c * 2f64.powf(1.0 / 12.0) <= top).collect();
        Stft {
            n,
            hop: n / 2,
            window,
            fft: RealFft::new(n),
            cal: 2.0 / (n as f64 * s2),
            centers,
            bands,
            band_ok,
            detect: w(DETECT_BAND.0, DETECT_BAND.1),
            sib: w(SIB_BAND.0, SIB_BAND.1),
            lowmid: w(LOWMID_BAND.0, LOWMID_BAND.1),
            lf: w(LF_BAND.0, LF_BAND.1),
            mid: w(MID_BAND.0, MID_BAND.1),
            frame: vec![0.0; n],
            power: vec![0.0; n / 2 + 1],
        }
    }

    fn frames(&mut self, x: &[f32], a: usize, b: usize) -> Vec<Frame> {
        let mut out = vec![];
        let mut s = a;
        while s + self.n <= b {
            for (i, f) in self.frame.iter_mut().enumerate() {
                *f = x[s + i] as f64 * self.window[i];
            }
            self.fft.power(&self.frame, &mut self.power);
            let (p, cal) = (&self.power, self.cal);
            let sum = |w: &Weights| w.iter().map(|(k, g)| g * p[*k]).sum::<f64>() * cal;
            out.push(Frame {
                detect: sum(&self.detect),
                sib: sum(&self.sib),
                lowmid: sum(&self.lowmid),
                lf: sum(&self.lf),
                mid: sum(&self.mid),
                bands: self.bands.iter().map(sum).collect(),
            });
            s += self.hop;
        }
        out
    }

    fn to_db(&self, ms: &[f64]) -> Vec<f64> {
        ms.iter().zip(&self.band_ok).map(|(m, ok)| if *ok { ms_db(*m) } else { FLOOR_DB }).collect()
    }
}

/// Part de chaque bin dans [lo, hi) (un bin couvre [k-1/2, k+1/2) × df), sans la composante continue ni le dernier bin.
fn weights(lo: f64, hi: f64, df: f64, half: usize) -> Weights {
    if hi <= lo {
        return vec![];
    }
    let k0 = ((lo / df - 0.5).floor().max(1.0)) as usize;
    let k1 = ((hi / df + 0.5).ceil() as usize).min(half - 1);
    (k0..=k1)
        .filter_map(|k| {
            let (bl, bh) = ((k as f64 - 0.5) * df, (k as f64 + 0.5) * df);
            let o = (hi.min(bh) - lo.max(bl)) / df;
            (o > 0.0).then_some((k, o.min(1.0)))
        })
        .collect()
}

fn mean_bands(frames: &[&Frame], nb: usize) -> Vec<f64> {
    let mut m = vec![0.0; nb];
    for f in frames {
        m.iter_mut().zip(&f.bands).for_each(|(a, b)| *a += b);
    }
    let n = frames.len().max(1) as f64;
    m.iter_mut().for_each(|v| *v /= n);
    m
}

fn hann(n: usize) -> Vec<f64> {
    (0..n).map(|i| 0.5 - 0.5 * (2.0 * PI * i as f64 / n as f64).cos()).collect()
}

/// FFT réelle de taille n (puissance de 2) par une FFT complexe de taille n/2.
struct RealFft {
    n: usize,
    rev: Vec<usize>,
    /// e^{-2πi j/m}, j < m/2 (m = n/2)
    tw: Vec<(f64, f64)>,
    /// e^{-2πi k/n}, k ≤ m
    post: Vec<(f64, f64)>,
    re: Vec<f64>,
    im: Vec<f64>,
}

impl RealFft {
    fn new(n: usize) -> RealFft {
        assert!(n >= 4 && n.is_power_of_two());
        let m = n / 2;
        let bits = m.trailing_zeros();
        let rev = (0..m).map(|i| if bits == 0 { 0 } else { i.reverse_bits() >> (usize::BITS - bits) }).collect();
        let tw = (0..m / 2).map(|j| {
            let a = 2.0 * PI * j as f64 / m as f64;
            (a.cos(), -a.sin())
        });
        let post = (0..=m).map(|k| {
            let a = 2.0 * PI * k as f64 / n as f64;
            (a.cos(), -a.sin())
        });
        RealFft { n, rev, tw: tw.collect(), post: post.collect(), re: vec![0.0; m], im: vec![0.0; m] }
    }

    /// |X[k]|² pour k de 0 à n/2.
    fn power(&mut self, x: &[f64], out: &mut [f64]) {
        let m = self.n / 2;
        for i in 0..m {
            let j = self.rev[i];
            self.re[j] = x[2 * i];
            self.im[j] = x[2 * i + 1];
        }
        let mut len = 2;
        while len <= m {
            let step = m / len;
            let h = len / 2;
            for start in (0..m).step_by(len) {
                for k in 0..h {
                    let (wr, wi) = self.tw[k * step];
                    let (i0, i1) = (start + k, start + k + h);
                    let (xr, xi) = (self.re[i1], self.im[i1]);
                    let (vr, vi) = (xr * wr - xi * wi, xr * wi + xi * wr);
                    let (ur, ui) = (self.re[i0], self.im[i0]);
                    self.re[i0] = ur + vr;
                    self.im[i0] = ui + vi;
                    self.re[i1] = ur - vr;
                    self.im[i1] = ui - vi;
                }
            }
            len <<= 1;
        }
        for (k, o) in out.iter_mut().enumerate().take(m + 1) {
            let (ar, ai) = (self.re[k % m], self.im[k % m]);
            let j = (m - k % m) % m;
            let (br, bi) = (self.re[j], -self.im[j]);
            let (er, ei) = ((ar + br) * 0.5, (ai + bi) * 0.5);
            let (or, oi) = ((ai - bi) * 0.5, -(ar - br) * 0.5);
            let (c, s) = self.post[k];
            let (xr, xi) = (er + c * or - s * oi, ei + c * oi + s * or);
            *o = xr * xr + xi * xi;
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Filtres et petits calculs
// ---------------------------------------------------------------------------------------------------------------

/// Biquad (forme directe II transposée), formules de R. Bristow-Johnson.
#[derive(Clone, Copy)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    fn new(b: [f64; 3], a: [f64; 3]) -> Biquad {
        Biquad { b0: b[0] / a[0], b1: b[1] / a[0], b2: b[2] / a[0], a1: a[1] / a[0], a2: a[2] / a[0], z1: 0.0, z2: 0.0 }
    }
    fn coefs(fs: f64, f: f64, q: f64) -> (f64, f64) {
        let w0 = 2.0 * PI * f.min(0.49 * fs) / fs;
        (w0.cos(), w0.sin() / (2.0 * q))
    }
    fn highpass(fs: f64, f: f64, q: f64) -> Biquad {
        let (c, al) = Biquad::coefs(fs, f, q);
        Biquad::new([(1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0], [1.0 + al, -2.0 * c, 1.0 - al])
    }
    fn lowpass(fs: f64, f: f64, q: f64) -> Biquad {
        let (c, al) = Biquad::coefs(fs, f, q);
        Biquad::new([(1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0], [1.0 + al, -2.0 * c, 1.0 - al])
    }
    /// passe-bande, 0 dB au sommet
    fn bandpass(fs: f64, f: f64, q: f64) -> Biquad {
        let (c, al) = Biquad::coefs(fs, f, q);
        Biquad::new([al, 0.0, -al], [1.0 + al, -2.0 * c, 1.0 - al])
    }
    fn run(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// Retire la composante continue (passe-haut du premier ordre à 10 Hz).
fn dc_block(x: &[f32], fs: f64) -> Vec<f32> {
    let r = 1.0 - 2.0 * PI * 10.0 / fs;
    let mut x1 = x.first().copied().unwrap_or(0.0) as f64;
    let mut y1 = 0.0f64;
    x.iter()
        .map(|v| {
            let v = *v as f64;
            let y = v - x1 + r * y1;
            x1 = v;
            y1 = y;
            y as f32
        })
        .collect()
}

fn highpass(x: &[f32], fs: f64, f: f64) -> Vec<f32> {
    let mut hp = Biquad::highpass(fs, f, std::f64::consts::FRAC_1_SQRT_2);
    x.iter().map(|v| hp.run(*v as f64) as f32).collect()
}

/// Niveau efficace en dB (convention AES17 : carré moyen × 2).
fn ms_db(ms: f64) -> f64 {
    if ms > 0.0 && ms.is_finite() {
        (10.0 * (2.0 * ms).log10()).max(FLOOR_DB)
    } else {
        FLOOR_DB
    }
}

fn amp_db(a: f64) -> f64 {
    if a > 0.0 && a.is_finite() {
        (20.0 * a.log10()).max(FLOOR_DB)
    } else {
        FLOOR_DB
    }
}

fn sorted(v: &[f64]) -> Vec<f64> {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    s
}

/// Centile par interpolation linéaire sur des valeurs triées.
fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return FLOOR_DB;
    }
    let pos = q.clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let i = pos.floor() as usize;
    let f = pos - i as f64;
    if i + 1 < sorted.len() {
        sorted[i] * (1.0 - f) + sorted[i + 1] * f
    } else {
        sorted[i]
    }
}

fn median(v: &[f64]) -> f64 {
    percentile(&sorted(v), 0.5)
}

fn r1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}
fn r2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}
fn r3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

// ---------------------------------------------------------------------------------------------------------------
// Voix synthétique
// ---------------------------------------------------------------------------------------------------------------

/// Générateur pseudo-aléatoire déterministe (splitmix64).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// dans [0, 1)
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
    /// bruit blanc de variance 1
    fn white(&mut self) -> f64 {
        (self.unit() * 2.0 - 1.0) * 3f64.sqrt()
    }
}

/// Voyelles : formants (Hz) et largeurs de bande (Hz).
const VOWELS: [[(f64, f64); 3]; 5] = [
    [(730.0, 90.0), (1250.0, 110.0), (2550.0, 160.0)],
    [(280.0, 60.0), (2250.0, 100.0), (2950.0, 170.0)],
    [(320.0, 70.0), (850.0, 90.0), (2300.0, 150.0)],
    [(420.0, 70.0), (2000.0, 100.0), (2650.0, 160.0)],
    [(480.0, 80.0), (900.0, 100.0), (2600.0, 160.0)],
];

/// Signal de test et de simulation : bruit léger pendant le silence, une voix synthétique (fondamentale f0_hz,
/// voyelles, sifflantes et plosives) pendant la lecture, plus forte pendant la phrase forte. Déterministe (seed).
pub fn synth_speech(rate: u32, segments: &[Segment], f0_hz: f64, seed: u64) -> Vec<f32> {
    let fs = rate as f64;
    let end = segments.iter().map(|s| s.end_s).filter(|e| e.is_finite()).fold(0.0, f64::max);
    let len = (end * fs).round() as usize;
    if rate < 8000 {
        return vec![0.0; len];
    }
    let mut out = vec![0.0f64; len];
    let mut rng = Rng(seed ^ 0x5EED_5EC4_A11C_0F00);
    let f0 = if f0_hz.is_finite() && f0_hz > 0.0 { f0_hz.clamp(50.0, 500.0) } else { 120.0 };
    let reading_gain = segments.iter().rev().find(|s| s.phase == Phase::Reading).map(|s| s.gain_db).unwrap_or(0);
    let offset = |i: usize| {
        let t = i as f64 / fs;
        segments
            .iter()
            .rev()
            .find(|s| t >= s.start_s && t < s.end_s)
            .map(|s| s.gain_db as f64 - reading_gain as f64)
            .unwrap_or(0.0)
    };
    // bruit de fond rose (filtre de Paul Kellet), -70 dBFS au gain de la lecture
    let mut bg = vec![0.0f64; len];
    let (mut p0, mut p1, mut p2) = (0.0, 0.0, 0.0);
    for v in bg.iter_mut() {
        let w = rng.white();
        p0 = 0.99765 * p0 + w * 0.099_046;
        p1 = 0.963 * p1 + w * 0.296_516_4;
        p2 = 0.57 * p2 + w * 1.052_691_3;
        *v = p0 + p1 + p2 + w * 0.1848;
    }
    let bg_ms = bg.iter().map(|v| v * v).sum::<f64>() / len.max(1) as f64;
    let bg_scale = if bg_ms > 0.0 { (db_ms(-70.0) / bg_ms).sqrt() } else { 0.0 };
    for (i, v) in bg.iter().enumerate() {
        out[i] = v * bg_scale * 10f64.powf(offset(i) / 20.0);
    }
    for s in segments {
        let (a, b) = (((s.start_s.max(0.0)) * fs) as usize, ((s.end_s.max(0.0) * fs) as usize).min(len));
        if b <= a {
            continue;
        }
        let off = s.gain_db as f64 - reading_gain as f64;
        match s.phase {
            Phase::Silence => {}
            Phase::Reading => speak(&mut out, fs, a, b, f0, -22.0 + off, &mut rng),
            Phase::Loud => speak(&mut out, fs, a, b, f0 * 1.08, -13.0 + off, &mut rng),
        }
    }
    out.into_iter().map(|v| v.clamp(-1.0, 1.0) as f32).collect()
}

/// carré moyen d'un niveau efficace en dB (convention AES17)
fn db_ms(db: f64) -> f64 {
    10f64.powf(db / 10.0) / 2.0
}

/// Des mots de 2 à 4 syllabes, des phrases de 5 mots ; consonnes : sifflante (2 syllabes sur 9), nasale (2 sur 9),
/// plosive (1 sur 18) ou rien.
fn speak(out: &mut [f64], fs: f64, a: usize, b: usize, f0: f64, level_db: f64, rng: &mut Rng) {
    let sec = |s: f64| (s * fs) as usize;
    let mut t = a + sec(0.15);
    let mut syl = 0usize;
    let mut left = 2 + (rng.unit() * 3.0) as usize;
    let mut in_sentence = 0usize;
    let mut phase = 0.0f64;
    while t + sec(0.35) < b {
        let pos = (t - a) as f64 / (b - a) as f64;
        // intonation : déclinaison dans la phrase et ondulation lente, centrées sur f0
        let decl = in_sentence as f64 / 4.0;
        let pitch = f0 * (1.0 + 0.06 * (1.0 - 2.0 * decl) + 0.04 * (2.0 * PI * pos * 3.0).sin());
        match syl % 18 {
            4 => {
                t += sec(0.03);
                plosive(out, fs, t, level_db, rng);
                t += sec(0.03);
            }
            1 | 6 | 10 | 15 => {
                let d = sec(rng.range(0.08, 0.12));
                sibilant(out, fs, t, d, level_db - 13.0 + rng.range(-2.0, 2.0), rng);
                t += d;
            }
            2 | 7 | 11 | 16 => {
                let d = sec(0.06);
                voiced(
                    out,
                    fs,
                    t,
                    d,
                    pitch,
                    &[(250.0, 60.0), (1100.0, 150.0), (2500.0, 200.0)],
                    level_db - 10.0,
                    &mut phase,
                    rng,
                );
                t += d;
            }
            _ => {}
        }
        let d = sec(rng.range(0.11, 0.19));
        let v = VOWELS[(syl * 3 + (rng.unit() * 2.0) as usize) % VOWELS.len()];
        voiced(out, fs, t, d, pitch, &v, level_db + rng.range(-2.5, 2.5), &mut phase, rng);
        t += d;
        syl += 1;
        left -= 1;
        if left == 0 {
            in_sentence += 1;
            t += sec(rng.range(0.08, 0.22));
            if in_sentence == 5 {
                in_sentence = 0;
                t += sec(rng.range(0.25, 0.4));
            }
            left = 2 + (rng.unit() * 3.0) as usize;
        }
    }
}

/// Son voisé : dent de scie à la fondamentale (avec un léger vibrato aléatoire), adoucie, puis formants en cascade.
#[allow(clippy::too_many_arguments)]
fn voiced(
    out: &mut [f64],
    fs: f64,
    start: usize,
    len: usize,
    f0: f64,
    formants: &[(f64, f64)],
    level_db: f64,
    phase: &mut f64,
    rng: &mut Rng,
) {
    if len == 0 || start >= out.len() {
        return;
    }
    let mut buf = vec![0.0; len];
    let jitter = rng.range(-0.01, 0.01);
    let mut lp = 0.0;
    let k_lp = 1.0 - (-2.0 * PI * 500.0 / fs).exp();
    for (i, v) in buf.iter_mut().enumerate() {
        let f = f0 * (1.0 + jitter + 0.004 * (2.0 * PI * 5.0 * i as f64 / fs).sin());
        let dt = f / fs;
        *phase += dt;
        if *phase >= 1.0 {
            *phase -= 1.0;
        }
        // dent de scie à bande limitée (polyBLEP)
        let mut saw = 2.0 * *phase - 1.0;
        let p = *phase;
        if p < dt {
            let x = p / dt;
            saw -= x + x - x * x - 1.0;
        } else if p > 1.0 - dt {
            let x = (p - 1.0) / dt;
            saw -= x * x + x + x + 1.0;
        }
        lp += k_lp * (saw - lp);
        *v = lp;
    }
    for (f, bw) in formants {
        let f = f.min(0.45 * fs);
        let c = -(-2.0 * PI * bw / fs).exp();
        let b = 2.0 * (-PI * bw / fs).exp() * (2.0 * PI * f / fs).cos();
        let a = 1.0 - b - c;
        let (mut y1, mut y2) = (0.0, 0.0);
        for v in buf.iter_mut() {
            let y = a * *v + b * y1 + c * y2;
            y2 = y1;
            y1 = y;
            *v = y;
        }
    }
    place(out, start, &mut buf, fs, level_db, 0.012, 0.03);
}

fn sibilant(out: &mut [f64], fs: f64, start: usize, len: usize, level_db: f64, rng: &mut Rng) {
    let mut bp = Biquad::bandpass(fs, 6500.0f64.min(0.4 * fs), 1.6);
    let mut hp = Biquad::highpass(fs, 3500.0f64.min(0.3 * fs), std::f64::consts::FRAC_1_SQRT_2);
    let mut buf: Vec<f64> = (0..len).map(|_| hp.run(bp.run(rng.white()))).collect();
    place(out, start, &mut buf, fs, level_db, 0.015, 0.025);
}

/// Coup de souffle grave (40 à 60 Hz, 35 ms, crête 13 dB au-dessus du niveau efficace de la voix, -7 dBFS au plus)
/// suivi d'un petit claquement.
fn plosive(out: &mut [f64], fs: f64, start: usize, level_db: f64, rng: &mut Rng) {
    let n = (0.035 * fs) as usize;
    let amp = 10f64.powf((level_db + 13.0) / 20.0).min(0.45);
    let f = rng.range(40.0, 60.0);
    for i in 0..n {
        if let Some(o) = out.get_mut(start + i) {
            let t = i as f64 / fs;
            *o += amp * (PI * i as f64 / n as f64).sin() * (2.0 * PI * f * t).cos();
        }
    }
    let click = (0.005 * fs) as usize;
    let mut buf: Vec<f64> = (0..click).map(|_| rng.white()).collect();
    place(out, start + n / 2, &mut buf, fs, level_db - 6.0, 0.0005, 0.003);
}

/// Ajoute buf au niveau efficace voulu, avec des rampes d'attaque et de relâchement (en secondes).
fn place(out: &mut [f64], start: usize, buf: &mut [f64], fs: f64, level_db: f64, attack: f64, release: f64) {
    let len = buf.len();
    if len == 0 {
        return;
    }
    let ms = buf.iter().map(|v| v * v).sum::<f64>() / len as f64;
    if ms <= 0.0 {
        return;
    }
    let scale = (db_ms(level_db) / ms).sqrt();
    let (na, nr) = (((attack * fs) as usize).clamp(1, len), ((release * fs) as usize).clamp(1, len));
    for (i, v) in buf.iter().enumerate() {
        let mut g = 1.0;
        if i < na {
            g *= 0.5 - 0.5 * (PI * i as f64 / na as f64).cos();
        }
        if i + nr > len {
            g *= 0.5 - 0.5 * (PI * (len - i) as f64 / nr as f64).cos();
        }
        if let Some(o) = out.get_mut(start + i) {
            *o += v * scale * g;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fft_reelle_egale_dft() {
        let n = 64;
        let x: Vec<f64> = (0..n).map(|i| ((i * 7919) % 97) as f64 / 97.0 - 0.5 + (i as f64 * 0.3).sin()).collect();
        let mut fft = RealFft::new(n);
        let mut p = vec![0.0; n / 2 + 1];
        fft.power(&x, &mut p);
        for (k, pk) in p.iter().enumerate() {
            let (mut re, mut im) = (0.0, 0.0);
            for (i, v) in x.iter().enumerate() {
                let a = -2.0 * PI * (k * i) as f64 / n as f64;
                re += v * a.cos();
                im += v * a.sin();
            }
            assert!((pk - (re * re + im * im)).abs() < 1e-9 * (1.0 + pk), "bin {k}");
        }
    }

    #[test]
    fn poids_des_bandes_couvrent_le_spectre() {
        let (df, half) = (48000.0 / 2048.0, 1024);
        let total: f64 = weights(100.0, 10_000.0, df, half).iter().map(|(_, w)| w).sum();
        assert!((total - (10_000.0 - 100.0) / df).abs() < 1e-9);
        let a: f64 = weights(100.0, 1000.0, df, half).iter().map(|(_, w)| w).sum();
        let b: f64 = weights(1000.0, 10_000.0, df, half).iter().map(|(_, w)| w).sum();
        assert!((a + b - total).abs() < 1e-9);
    }
}
