//! Outil local : profils sur mesure calculés depuis une empreinte (JSON de l'exemple voiceprint).
//!
//! cargo run --release -p rustudio-core --example profils -- empreinte.json [--model rode-nt1a] [--type condenser] [--gain 30]
use rustudio_core::state::{default_state, MicCal, VoiceRecord};
use rustudio_core::voice::VoicePrint;
use rustudio_core::{mics, voicefit};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage : profils empreinte.json [--model clé] [--type dynamic|condenser|jack] [--gain dB]");
        std::process::exit(2);
    };
    let opt = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let mut print: VoicePrint = serde_json::from_str(&std::fs::read_to_string(path).expect("lecture")).expect("JSON d'empreinte");
    // essais : --bright ajoute la courbe publiée du NT1-A à la voix, --room N un bruit de ventilation N dB sous la voix
    if args.iter().any(|a| a == "--bright") {
        let m = mics::model("rode-nt1a").unwrap();
        for (v, f) in print.voice_db.iter_mut().zip(&print.bands) {
            *v += mics::response_db(m, *f);
        }
    }
    if let Some(n) = opt("--room").and_then(|n| n.parse::<f64>().ok()) {
        let mid: f64 = print.bands.iter().zip(&print.voice_db).filter(|(f, _)| (800.0..=2500.0).contains(*f)).map(|(_, v)| *v).sum::<f64>() / 7.0;
        for (x, f) in print.noise_db.iter_mut().zip(&print.bands) {
            let lf = 10.0 * (-0.5 * ((f / 120.0f64).log2() / 0.8).powi(2)).exp();
            *x = mid - n + lf - 3.0 * (f / 1000.0f64).log2().max(0.0);
        }
    }
    // --roomfile mesure.json : bruit réel d'une pièce (spectre 1/6 d'octave « room.bands_1_6_oct »), recalé comme si
    // la voix (centile 50 de la parole) arrivait à -20 dBFS au gain de la mesure
    if let Some(path) = opt("--roomfile") {
        let m: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).expect("lecture")).expect("JSON");
        let offset = print.levels.speech_p50_db - -20.0;
        if let Some(bands) = m["room"]["bands_1_6_oct"].as_array() {
            for b in bands {
                let (Some(k), Some(db)) = (b["k"].as_i64(), b["mean_db"].as_f64()) else { continue };
                let i = (k - rustudio_core::voice::BAND_K_MIN as i64) as usize;
                if i < print.noise_db.len() {
                    print.noise_db[i] = db + offset;
                }
            }
        }
    }
    let model = opt("--model");
    let kind = opt("--type").or_else(|| model.as_deref().and_then(mics::model).map(|m| m.kind.to_string())).unwrap_or_else(|| "condenser".into());
    let gain: i32 = opt("--gain").and_then(|g| g.parse().ok()).unwrap_or(print.gain_db);
    let mut st = default_state();
    st.mic.kind = kind.clone();
    st.mic.gain_db = gain;
    st.mic.model = model.clone();
    let l = &print.levels;
    st.mic_cal = Some(MicCal {
        kind: kind.clone(), gain_db: print.gain_db, t: 0.0, noise_db: l.noise_rms_db - 3.0, noise_peak_db: l.noise_peak_db - 3.0,
        voice_low_db: l.speech_p10_db - 3.0, voice_db: l.speech_p50_db - 3.0, voice_peak_db: l.speech_p95_db - 3.0,
        loud_peak_db: l.loud_peak_db,
    });
    st.voice_print = Some(VoiceRecord { print, t: 0.0, mic_type: kind, model });
    let v = voicefit::voice_view(&st);
    println!("résumé : {}", v["summary"]);
    for n in v["notes"].as_array().unwrap() {
        println!("  conseil [{}] {}", n["lvl"].as_str().unwrap(), n["t"].as_str().unwrap());
    }
    for (k, p) in v["profiles"].as_object().unwrap() {
        let d = &p["dsp"];
        let eq: Vec<String> = ["90", "250", "500", "1000", "3000", "8000"]
            .iter()
            .map(|b| format!("{}@{}", d["eq"][b], d["eq_freq"][b]))
            .collect();
        println!("\n== {} ({k}) fx {}", p["label"].as_str().unwrap(), p["fx"]);
        println!("   EQ {}", eq.join("  "));
        println!("   gate {}  comp {}  deesser {}", d["gate"], d["comp"], d["deesser"]);
        for n in p["notes"].as_array().unwrap() {
            println!("   - {}", n["t"].as_str().unwrap());
        }
    }
}
