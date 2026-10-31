//! Profils sur mesure : modèle mesuré de l'égaliseur de la Mini, ajustement des six bandes, réglages calculés
//! depuis une empreinte vocale, catalogue des micros et relecture tolérante de l'état.
use rustudio_core::consts::{eq_range, EQ_BANDS};
use rustudio_core::mics::{self, MODELS};
use rustudio_core::state::{default_state, normalize_cal, normalize_state, VoiceRecord};
use rustudio_core::voice::{analyze, band_centers, synth_speech, Phase, Segment, VoicePrint};
use rustudio_core::voicefit::{self, eq_curve, fit_bands, MINI_EQ};
use serde_json::json;

fn segs(gain: i32) -> Vec<Segment> {
    [(Phase::Silence, 0.0, 4.0), (Phase::Reading, 4.0, 18.0), (Phase::Loud, 18.0, 22.0)]
        .iter()
        .map(|(phase, a, b)| Segment { phase: *phase, start_s: *a, end_s: *b, gain_db: gain })
        .collect()
}

fn synth_print(f0: f64) -> VoicePrint {
    let s = segs(30);
    analyze(&synth_speech(48_000, &s, f0, 7), 48_000, &s).unwrap()
}

fn st_with(print: VoicePrint, model: Option<&str>) -> rustudio_core::State {
    let mut st = default_state();
    st.mic.kind = "condenser".into();
    st.mic.gain_db = 30;
    st.mic.model = model.map(String::from);
    let l = print.levels.clone();
    st.mic_cal = normalize_cal(&json!({"type": "condenser", "gain_db": 30, "t": 1.0, "noise_db": l.noise_rms_db,
        "noise_peak_db": l.noise_peak_db, "voice_low_db": l.speech_p10_db, "voice_db": l.speech_p50_db,
        "voice_peak_db": l.speech_p95_db, "loud_peak_db": l.loud_peak_db}));
    st.voice_print = Some(VoiceRecord { print, t: 1.0, mic_type: "condenser".into(), model: model.map(String::from) });
    st
}

fn band(b: &str) -> &'static voicefit::BandModel {
    MINI_EQ.iter().find(|m| m.band == b).unwrap()
}

#[test]
fn modele_mesure_de_la_mini() {
    // plateau grave du second ordre : -9 dB réglé à 90 Hz, comparé à la coupe mesurée sur la Mini (x/f, dB ;
    // barre d'erreur de la mesure : 1,6 dB vers 100 Hz, 2,4 dB vers 20 Hz)
    let low = band("90");
    let measured = [
        (0.25, -10.1), (0.35, -11.1), (0.5, -9.9), (0.6, -9.2), (0.7, -8.3), (0.8, -7.0), (0.9, -6.3), (1.0, -4.7),
        (1.1, -4.5), (1.25, -1.5), (1.4, -1.1), (1.6, 0.1), (2.0, 0.2), (2.5, 0.0), (3.0, 0.2), (4.0, 0.0), (8.0, -0.2),
    ];
    let gaps: Vec<f64> = measured.iter().map(|(x, db)| (low.db(90.0, -9.0, 90.0 * x) - db).abs()).collect();
    assert!(gaps.iter().all(|g| *g < 1.8), "{gaps:?}");
    assert!(gaps.iter().sum::<f64>() / (gaps.len() as f64) < 0.6, "{gaps:?}");
    assert!((low.db(90.0, -9.0, 90.0) + 4.5).abs() < 0.05, "mi-gain au coude");
    assert!(low.db(90.0, -9.0, 1000.0).abs() < 0.05);
    assert!((low.db(90.0, 9.0, 20.0) - 9.2).abs() < 0.2, "montée symétrique de la coupe");
    // cloche demandée à 160 Hz : la Mini la place sur 160 Hz (mesuré de 100 à 300 Hz)
    let b250 = band("250");
    let peak_at = (60..400).map(|f| f as f64).max_by(|a, b| b250.db(160.0, 9.0, *a).partial_cmp(&b250.db(160.0, 9.0, *b)).unwrap()).unwrap();
    assert!((157.0..=163.0).contains(&peak_at), "{peak_at}");
    // plateau aigu juste : à mi-pente à la fréquence réglée
    let hi = band("8000");
    assert!((hi.db(10_000.0, 9.0, 10_000.0) - 4.5).abs() < 0.6);
    assert!(hi.db(10_000.0, 9.0, 1000.0).abs() < 0.2);
    // préréglage neutre : courbe plate
    let d = rustudio_core::dsp::preset_dsp("brut");
    assert!(eq_curve(&d, &band_centers()).iter().all(|x| x.abs() < 1e-9));
    // le modèle décrit à l'interface
    let m = voicefit::eq_model_json();
    assert_eq!(m["90"], json!({"kind": "lowshelf", "q": 0.94}));
    assert_eq!(m["250"], json!({"kind": "peak", "q": 1.38}));
    assert_eq!(m["8000"]["kind"], "highshelf");
}

#[test]
fn ajustement_retrouve_un_reglage_connu() {
    let g = band_centers();
    let mut d = rustudio_core::dsp::preset_dsp("brut");
    for (b, gain, f) in [("250", -4, 220.0), ("1000", 3, 1500.0), ("3000", 2, 4200.0), ("8000", -3, 11_000.0)] {
        d.eq.insert(b.into(), gain);
        d.eq_freq.insert(b.into(), f);
    }
    let target = eq_curve(&d, &g);
    let w = vec![1.0; g.len()];
    let fit = fit_bands(&target, &w, 60.0);
    let mut got = rustudio_core::dsp::preset_dsp("brut");
    for ((b, _, _), (f, gn)) in EQ_BANDS.iter().zip(&fit) {
        let (lo, hi) = eq_range(b).unwrap();
        assert!((lo..=hi).contains(f), "{b} : {f}");
        got.eq.insert(b.to_string(), *gn);
        got.eq_freq.insert(b.to_string(), *f);
    }
    let curve = eq_curve(&got, &g);
    let rms = (target.iter().zip(&curve).map(|(a, b)| (a - b) * (a - b)).sum::<f64>() / g.len() as f64).sqrt();
    assert!(rms < 0.6, "écart {rms:.2} dB : {fit:?}");
    assert_eq!(fit, fit_bands(&target, &w, 60.0), "résultat stable");
}

#[test]
fn profils_sur_mesure_dans_les_limites() {
    let g = band_centers();
    for f0 in [105.0, 210.0] {
        let st = st_with(synth_print(f0), Some("rode-nt1a"));
        let v = voicefit::voice_view(&st);
        assert_eq!(v["valid"], json!(true));
        for (k, p) in v["profiles"].as_object().unwrap() {
            let d: rustudio_core::dsp::Dsp = serde_json::from_value(p["dsp"].clone()).unwrap();
            for (b, _, _) in EQ_BANDS {
                let (lo, hi) = eq_range(b).unwrap();
                assert!((-9..=9).contains(&d.eq[b]) && (lo..=hi).contains(&d.eq_freq[b]), "{k} {b} : {d:?}");
            }
            let curve = eq_curve(&d, &g);
            let at = |f: f64| curve[g.iter().position(|x| *x >= f).unwrap()];
            assert!(at(45.0) <= 0.5, "{k} : jamais de remontée sous la voix ({:.1} dB)", at(45.0));
            assert!(curve.iter().all(|x| *x < 9.5), "{k} : {curve:?}");
            assert!((-59..=-10).contains(&d.gate.threshold) && (-40..=-6).contains(&d.comp.threshold), "{k} : {d:?}");
            assert!((3..=9).contains(&d.comp.ratio) && (0..=12).contains(&d.comp.makeup) && (0..=70).contains(&d.deesser), "{k} : {d:?}");
            assert!(p["fitted"] == json!(true) && !p["notes"].as_array().unwrap().is_empty() && p["chart"].is_object(), "{k}");
        }
        let anti = &v["profiles"]["anti-bruit"]["dsp"];
        assert_eq!(anti["gate"]["attenuation"], 100, "Anti-clavier : silence complet entre les phrases");
    }
}

#[test]
fn brillance_du_nt1a_calmee() {
    // même voix, entendue par un micro neutre puis par le NT1-A (sa courbe publiée ajoutée) : avec le NT1-A, les
    // aigus de 9 à 13 kHz sont plus baissés, et le de-esser plus fort
    let g = band_centers();
    let plain = synth_print(120.0);
    let mut bright = plain.clone();
    let nt1a = mics::model("rode-nt1a").unwrap();
    for (v, f) in bright.voice_db.iter_mut().zip(&bright.bands) {
        *v += mics::response_db(nt1a, *f);
    }
    let a = voicefit::voice_view(&st_with(plain, None));
    let b = voicefit::voice_view(&st_with(bright, Some("rode-nt1a")));
    for k in ["studio", "radio", "stream", "podcast"] {
        let curve = |v: &serde_json::Value| {
            let d: rustudio_core::dsp::Dsp = serde_json::from_value(v["profiles"][k]["dsp"].clone()).unwrap();
            let c = eq_curve(&d, &g);
            let sel: Vec<f64> = g.iter().zip(&c).filter(|(f, _)| (9000.0..=13000.0).contains(*f)).map(|(_, x)| *x).collect();
            (sel.iter().sum::<f64>() / sel.len() as f64, d.deesser)
        };
        let ((ha, da), (hb, db)) = (curve(&a), curve(&b));
        assert!(hb < ha - 1.0, "{k} : aigus {ha:.1} → {hb:.1} dB");
        assert!(db > da, "{k} : de-esser {da} → {db}");
    }
}

#[test]
fn catalogue_des_micros() {
    let gain = |k: &str| mics::start_gain_db(mics::model(k).unwrap());
    assert_eq!(gain("rode-nt1a"), 30);
    assert_eq!(gain("shure-sm7b"), 57);
    assert_eq!(gain("rode-podmic"), 54);
    assert_eq!(gain("at2020"), 35);
    for m in MODELS {
        assert!(rustudio_core::consts::is_mic_type(m.kind), "{}", m.key);
        assert_eq!(m.phantom, m.kind == "condenser", "{}", m.key);
        assert!(m.generic || m.source.is_some_and(|s| s.starts_with("https://")), "{} : fiche officielle", m.key);
        assert!(!m.tips.is_empty() && m.distance_cm[0] < m.distance_cm[1], "{}", m.key);
    }
    let nt1a = mics::model("rode-nt1a").unwrap();
    assert!(nt1a.bright && mics::response_db(nt1a, 11_000.0) > 3.0 && mics::response_db(nt1a, 1000.0) == 0.0);
    assert_eq!(mics::catalog_json()["rode-nt1a"]["start_gain_db"], 30);
}

#[test]
fn etat_relu_avec_empreinte_et_frequences() {
    let st = st_with(synth_print(130.0), Some("rode-nt1a"));
    let mut v = st.to_value();
    v["dsp"]["eq_freq"]["8000"] = json!(25000);
    v["dsp"]["eq_freq"]["250"] = json!("abc");
    let back = normalize_state(&v);
    assert_eq!(back.voice_print, st.voice_print, "empreinte relue telle quelle");
    assert_eq!(back.mic.model.as_deref(), Some("rode-nt1a"));
    assert_eq!(back.dsp.eq_freq["8000"], 18_000.0, "fréquence bornée à la plage de la bande");
    assert_eq!(back.dsp.eq_freq["250"], 250.0, "valeur illisible : fréquence d'origine");
    let mut bad = v.clone();
    bad["voice_print"]["voice_db"] = json!([1, 2, 3]);
    bad["mic"]["model"] = json!("micro-inconnu");
    let back = normalize_state(&bad);
    assert!(back.voice_print.is_none() && back.mic.model.is_none());
    // état d'avant 5.6.14 : fréquences d'origine
    let mut old = st.to_value();
    old["dsp"].as_object_mut().unwrap().remove("eq_freq");
    old.as_object_mut().unwrap().remove("voice_print");
    let back = normalize_state(&old);
    assert_eq!(back.dsp.eq_freq["90"], 90.0);
    assert!(back.voice_print.is_none());
    // empreinte d'un autre micro : non valable pour le micro branché
    let mut other = st.clone();
    other.mic.kind = "dynamic".into();
    other.mic.model = None;
    assert!(other.voice_for_mic().is_none());
}
