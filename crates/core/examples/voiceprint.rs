//! Outil local : empreinte vocale d'un fichier WAV (PCM 8/16/24/32 bits ou flottant 32/64 bits, mono ou stéréo).
//!
//! cargo run --release -p rustudio-core --example voiceprint -- voix.wav silence:0-5@40 reading:5-21@40 loud:21-26@40
//!
//! Chaque étape s'écrit phase:début-fin[@gain] (secondes, gain du micro en dB). Sans étape, tout le fichier est la
//! lecture. --gain G donne le gain des étapes qui n'en précisent pas (0 par défaut) ; --compact imprime le JSON
//! sur une ligne. Le temps d'analyse s'affiche sur la sortie d'erreur.
//!
//! À la place du fichier, synth:F0[:graine] analyse la voix de synthèse (voice::synth_speech) à --rate Hz
//! (48 000 par défaut), sur les étapes données ou silence:0-5, reading:5-21, loud:21-26.
use rustudio_core::voice::{analyze, synth_speech, Phase, Segment};
use std::process::exit;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        eprintln!("usage : voiceprint fichier.wav|synth:F0[:graine] [silence:0-5@40] [reading:5-21@40] [loud:21-26@40] [--gain G] [--rate R] [--compact]");
        exit(2);
    }
    let mut gain = 0;
    let mut rate = 48_000u32;
    let mut compact = false;
    let mut specs = vec![];
    let mut it = args[1..].iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--gain" => {
                gain = it.next().and_then(|g| g.parse().ok()).unwrap_or_else(|| fail("--gain attend un nombre"))
            }
            "--rate" => {
                rate = it.next().and_then(|g| g.parse().ok()).unwrap_or_else(|| fail("--rate attend un nombre"))
            }
            "--compact" => compact = true,
            s => specs.push(s.to_string()),
        }
    }
    let mut segments: Vec<Segment> =
        specs.iter().map(|s| parse_segment(s, gain).unwrap_or_else(|e| fail(&e))).collect();
    let (samples, rate) = match args[0].strip_prefix("synth:") {
        Some(spec) => {
            let mut parts = spec.split(':');
            let f0 = parts.next().and_then(|f| f.parse().ok()).unwrap_or_else(|| fail("synth:F0 attend une fréquence"));
            let seed = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
            if segments.is_empty() {
                for (phase, a, b) in
                    [(Phase::Silence, 0.0, 5.0), (Phase::Reading, 5.0, 21.0), (Phase::Loud, 21.0, 26.0)]
                {
                    segments.push(Segment { phase, start_s: a, end_s: b, gain_db: gain });
                }
            }
            let t0 = Instant::now();
            let x = synth_speech(rate, &segments, f0, seed);
            eprintln!("synthèse : {:.1} ms", t0.elapsed().as_secs_f64() * 1000.0);
            (x, rate)
        }
        None => read_wav(&args[0]).unwrap_or_else(|e| fail(&e)),
    };
    let dur = samples.len() as f64 / rate as f64;
    if segments.is_empty() {
        segments.push(Segment { phase: Phase::Reading, start_s: 0.0, end_s: dur, gain_db: gain });
    }
    let t0 = Instant::now();
    match analyze(&samples, rate, &segments) {
        Ok(p) => {
            eprintln!("analyse : {:.1} ms pour {dur:.1} s à {rate} Hz", t0.elapsed().as_secs_f64() * 1000.0);
            let json = if compact { serde_json::to_string(&p) } else { serde_json::to_string_pretty(&p) };
            println!("{}", json.unwrap_or_else(|e| fail(&e.to_string())));
        }
        Err(e) => fail(&e),
    }
}

fn fail(msg: &str) -> ! {
    eprintln!("erreur : {msg}");
    exit(1)
}

fn parse_segment(s: &str, default_gain: i32) -> Result<Segment, String> {
    let bad = || format!("étape illisible : {s} (attendu phase:début-fin[@gain])");
    let (phase, rest) = s.split_once(':').ok_or_else(bad)?;
    let phase = match phase {
        "silence" => Phase::Silence,
        "reading" | "lecture" => Phase::Reading,
        "loud" | "forte" => Phase::Loud,
        _ => return Err(bad()),
    };
    let (times, gain) = match rest.split_once('@') {
        Some((t, g)) => (t, g.parse::<i32>().map_err(|_| bad())?),
        None => (rest, default_gain),
    };
    let (a, b) = times.split_once('-').ok_or_else(bad)?;
    let start_s = a.parse::<f64>().map_err(|_| bad())?;
    let end_s = b.parse::<f64>().map_err(|_| bad())?;
    Ok(Segment { phase, start_s, end_s, gain_db: gain })
}

/// Lit un WAV et le ramène en mono (moyenne des voies).
fn read_wav(path: &str) -> Result<(Vec<f32>, u32), String> {
    let data = std::fs::read(path).map_err(|e| format!("{path} : {e}"))?;
    if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return Err(format!("{path} : pas un fichier WAV"));
    }
    let u16_at = |b: &[u8], i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let u32_at = |b: &[u8], i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let (mut fmt, mut pcm) = (None, None);
    let mut pos = 12;
    while pos + 8 <= data.len() {
        let size = u32_at(&data, pos + 4) as usize;
        let start = pos + 8;
        let body = &data[start..(start + size).min(data.len())];
        match &data[pos..pos + 4] {
            b"fmt " if body.len() >= 16 => {
                let mut format = u16_at(body, 0);
                if format == 0xFFFE && body.len() >= 26 {
                    format = u16_at(body, 24); // WAVE_FORMAT_EXTENSIBLE : sous-format
                }
                fmt = Some((format, u16_at(body, 2), u32_at(body, 4), u16_at(body, 14)));
            }
            b"data" => pcm = Some(body),
            _ => {}
        }
        pos = start + size + (size & 1);
    }
    let (format, channels, rate, bits) = fmt.ok_or("chunk fmt absent")?;
    let pcm = pcm.ok_or("chunk data absent")?;
    if !matches!((format, bits), (1, 8 | 16 | 24 | 32) | (3, 32 | 64)) {
        return Err(format!("format WAV non pris en charge (format {format}, {bits} bits)"));
    }
    let ch = channels.max(1) as usize;
    let bps = bits as usize / 8;
    let sample = |b: &[u8]| -> f32 {
        match (format, bits) {
            (1, 8) => (b[0] as f32 - 128.0) / 128.0,
            (1, 16) => i16::from_le_bytes([b[0], b[1]]) as f32 / 32_768.0,
            (1, 24) => (i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8) as f32 / 8_388_608.0,
            (1, _) => i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0,
            (_, 32) => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            _ => f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]) as f32,
        }
    };
    let out = pcm.chunks_exact(bps * ch).map(|f| f.chunks_exact(bps).map(sample).sum::<f32>() / ch as f32).collect();
    Ok((out, rate))
}
