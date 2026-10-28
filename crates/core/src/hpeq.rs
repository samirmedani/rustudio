//! Egaliseur du casque (Equalizer APO) : styles, corrections AutoEq, filtres RBJ, preampli, fichier de config.
use crate::value::{clamp, first_chars, get, has, truthy};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const HP_BANDS: [i32; 10] = [31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];
pub const HP_Q: f64 = 1.41;
pub const HP_CHANNELS: [&str; 5] = ["Game", "Music", "System", "Chat", "Sample"];
pub const LIVE_APPS: [&str; 8] =
    ["obs64", "obs32", "obs", "streamlabs obs", "streamlabs desktop", "xsplit.core", "twitchstudio", "meld studio"];
pub const CONFIG_FILE: &str = "rustudio.txt";

pub struct Style {
    pub key: &'static str,
    pub label: &'static str,
    pub desc: &'static str,
    pub bands: [i32; 10],
}
pub const HP_STYLES: [Style; 7] = [
    Style { key: "neutre", label: "Neutre", desc: "Le son tel quel, ou juste corrigé pour ton casque.", bands: [0; 10] },
    Style { key: "basses", label: "Basses +", desc: "Des graves pleins et profonds, sans couvrir les voix.", bands: [4, 5, 3, 1, 0, 0, 0, 0, 0, 0] },
    Style { key: "jeu", label: "Jeu", desc: "Pas, rechargements et directions plus nets.", bands: [-2, -1, -1, -1, 0, 1, 3, 3, 2, 0] },
    Style { key: "voix", label: "Voix claire", desc: "Discord, podcasts et vidéos : les voix devant.", bands: [-4, -3, -1, 0, 1, 2, 3, 2, 0, -1] },
    Style { key: "film", label: "Film", desc: "Grand et immersif : impacts et ambiance.", bands: [4, 4, 2, 0, -1, -1, 0, 2, 3, 3] },
    Style { key: "musique", label: "Musique", desc: "Chaleureux et détaillé, pour l'écoute.", bands: [2, 3, 1, 0, -1, 0, 1, 1, 2, 2] },
    Style { key: "doux", label: "Doux", desc: "Longues sessions : graves et aigus calmés.", bands: [-3, -2, -1, 0, 0, 1, 1, 0, -2, -3] },
];
pub fn style(key: &str) -> Option<&'static Style> {
    HP_STYLES.iter().find(|s| s.key == key)
}

pub type Filter = (&'static str, f64, f64, f64);
/// Corrections AutoEq (cible Harman, mesures oratory1990, licence MIT) : (type, frequence, gain dB, Q)
pub const HEADPHONES: [(&str, [Filter; 10]); 21] = [
    ("Sennheiser HD 560S", [("LSC", 105.0, 9.0, 0.7), ("PK", 8983.0, 4.4, 2.63), ("PK", 54.0, -5.4, 0.48), ("PK", 1148.0, -1.5, 2.38), ("PK", 491.0, 0.6, 1.23), ("HSC", 10000.0, -3.9, 0.7), ("PK", 7451.0, 2.0, 3.91), ("PK", 4502.0, -1.7, 6.0), ("PK", 1937.0, 0.9, 3.93), ("PK", 5639.0, -0.9, 6.0)]),
    ("Sennheiser HD 599", [("LSC", 105.0, 6.1, 0.7), ("PK", 152.0, -4.3, 0.59), ("PK", 1751.0, 4.4, 1.45), ("PK", 4485.0, -1.9, 2.72), ("PK", 40.0, 1.0, 3.06), ("HSC", 10000.0, -0.7, 0.7), ("PK", 9629.0, 6.0, 1.71), ("PK", 5771.0, -2.8, 5.56), ("PK", 3175.0, -2.0, 4.7), ("PK", 3690.0, 0.7, 4.38)]),
    ("Sennheiser HD 600", [("LSC", 105.0, 6.5, 0.7), ("PK", 125.0, -2.7, 0.55), ("PK", 8445.0, 3.3, 1.61), ("PK", 522.0, 0.7, 1.02), ("PK", 1298.0, -1.2, 2.14), ("HSC", 10000.0, -3.1, 0.7), ("PK", 3158.0, -1.8, 3.67), ("PK", 2166.0, 0.9, 3.32), ("PK", 6639.0, 2.2, 5.82), ("PK", 5433.0, -1.2, 5.7)]),
    ("Sennheiser HD 650", [("LSC", 105.0, 6.4, 0.7), ("PK", 8800.0, 5.1, 1.42), ("PK", 118.0, -3.1, 0.5), ("PK", 37.0, 0.7, 3.96), ("PK", 3169.0, -1.7, 3.89), ("HSC", 10000.0, -2.1, 0.7), ("PK", 1227.0, -1.2, 2.53), ("PK", 2055.0, 1.2, 3.23), ("PK", 587.0, 0.4, 1.19), ("PK", 5332.0, -1.1, 5.75)]),
    ("Sennheiser HD 6XX", [("LSC", 105.0, 6.4, 0.7), ("PK", 8800.0, 5.1, 1.42), ("PK", 118.0, -3.1, 0.5), ("PK", 37.0, 0.7, 3.96), ("PK", 3169.0, -1.7, 3.89), ("HSC", 10000.0, -2.1, 0.7), ("PK", 1227.0, -1.2, 2.53), ("PK", 2055.0, 1.2, 3.23), ("PK", 587.0, 0.4, 1.19), ("PK", 5332.0, -1.1, 5.75)]),
    ("Sennheiser HD 660S2", [("LSC", 105.0, 7.6, 0.7), ("PK", 98.0, -3.5, 0.31), ("PK", 8089.0, 4.7, 0.53), ("PK", 5567.0, -6.6, 3.92), ("PK", 1206.0, -2.0, 2.46), ("HSC", 10000.0, -3.8, 0.7), ("PK", 2218.0, 1.1, 3.75), ("PK", 3206.0, -1.2, 3.51), ("PK", 4367.0, 1.3, 6.0), ("PK", 115.0, 0.1, 1.26)]),
    ("Sennheiser HD 800 S", [("LSC", 105.0, 6.6, 0.7), ("PK", 142.0, -2.3, 0.3), ("PK", 1959.0, 2.5, 0.88), ("PK", 5679.0, -4.4, 4.36), ("PK", 212.0, -0.5, 2.36), ("HSC", 10000.0, -4.3, 0.7), ("PK", 1043.0, -1.0, 2.89), ("PK", 1445.0, 1.0, 3.43), ("PK", 2640.0, -0.9, 4.01), ("PK", 3421.0, 1.3, 5.47)]),
    ("Beyerdynamic DT 770 Pro", [("LSC", 105.0, -3.0, 0.7), ("PK", 6430.0, -4.2, 0.97), ("PK", 3684.0, 6.8, 2.88), ("PK", 213.0, 3.5, 2.46), ("PK", 139.0, -3.9, 4.92), ("HSC", 10000.0, -5.4, 0.7), ("PK", 93.0, 3.5, 3.0), ("PK", 9561.0, 2.3, 1.86), ("PK", 45.0, -1.3, 1.74), ("PK", 119.0, -2.1, 5.83)]),
    ("Beyerdynamic DT 990 Pro", [("LSC", 105.0, 10.1, 0.7), ("PK", 541.0, 3.1, 0.52), ("PK", 64.0, -7.4, 0.38), ("PK", 7731.0, -4.6, 1.06), ("PK", 640.0, 0.6, 2.47), ("HSC", 10000.0, -9.8, 0.7), ("PK", 7136.0, 3.1, 2.08), ("PK", 5897.0, -4.8, 5.3), ("PK", 4193.0, 1.1, 1.51), ("PK", 9436.0, 2.7, 3.53)]),
    ("Beyerdynamic DT 700 Pro X", [("LSC", 105.0, 1.0, 0.7), ("PK", 131.0, -4.1, 1.24), ("PK", 2692.0, -2.6, 0.37), ("PK", 265.0, 4.2, 1.71), ("PK", 3557.0, 6.8, 2.7), ("HSC", 10000.0, -0.2, 0.7), ("PK", 5709.0, -2.0, 3.38), ("PK", 62.0, 1.5, 3.47), ("PK", 88.0, -0.8, 3.25), ("PK", 4028.0, 1.4, 5.91)]),
    ("Beyerdynamic DT 900 Pro X", [("LSC", 105.0, 5.7, 0.7), ("PK", 94.0, -2.3, 0.22), ("PK", 6103.0, -5.3, 1.81), ("PK", 4103.0, 3.8, 4.69), ("PK", 6339.0, 1.5, 0.18), ("HSC", 10000.0, -1.2, 0.7), ("PK", 2503.0, -0.8, 3.03), ("PK", 59.0, -0.3, 1.39), ("PK", 82.0, 0.7, 3.97), ("PK", 2966.0, 1.0, 6.0)]),
    ("AKG K371", [("LSC", 105.0, -2.7, 0.7), ("PK", 182.0, -2.3, 1.23), ("PK", 4038.0, 5.0, 3.6), ("PK", 67.0, 3.1, 1.41), ("PK", 1066.0, -0.8, 2.57), ("HSC", 10000.0, 2.4, 0.7), ("PK", 5564.0, -1.6, 3.89), ("PK", 4232.0, 0.9, 5.03), ("PK", 524.0, 0.3, 1.66), ("PK", 2048.0, 0.5, 4.67)]),
    ("AKG K702", [("LSC", 105.0, 7.1, 0.7), ("PK", 119.0, -2.7, 0.23), ("PK", 9459.0, 3.2, 3.19), ("PK", 730.0, 3.2, 1.22), ("PK", 2241.0, -3.5, 3.8), ("HSC", 10000.0, -1.7, 0.7), ("PK", 3721.0, 3.4, 2.02), ("PK", 5483.0, -4.7, 4.79), ("PK", 2633.0, -2.0, 5.29), ("PK", 56.0, -0.6, 2.62)]),
    ("AKG K712", [("LSC", 105.0, 6.9, 0.7), ("PK", 143.0, -3.6, 0.23), ("PK", 1217.0, 5.0, 1.43), ("PK", 3049.0, 4.8, 3.18), ("PK", 2178.0, -3.4, 3.3), ("HSC", 10000.0, -0.3, 0.7), ("PK", 5868.0, -2.6, 5.87), ("PK", 67.0, -0.6, 1.75), ("PK", 157.0, 0.9, 1.87), ("PK", 243.0, -0.5, 2.27)]),
    ("Audio-Technica ATH-M40x", [("LSC", 105.0, 17.6, 0.7), ("PK", 48.0, -16.9, 0.29), ("PK", 348.0, 6.0, 0.87), ("PK", 5150.0, 3.3, 4.1), ("PK", 2979.0, 1.8, 1.99), ("HSC", 10000.0, -6.9, 0.7), ("PK", 1378.0, -1.4, 2.13), ("PK", 796.0, 0.6, 1.67), ("PK", 6306.0, 1.6, 5.1), ("PK", 2225.0, 0.8, 3.7)]),
    ("Audio-Technica ATH-M50x", [("LSC", 105.0, 0.6, 0.7), ("PK", 156.0, -5.2, 0.73), ("PK", 326.0, 5.3, 1.59), ("PK", 7077.0, 2.8, 2.22), ("PK", 3483.0, 2.1, 5.82), ("HSC", 10000.0, -4.1, 0.7), ("PK", 45.0, -1.1, 1.9), ("PK", 66.0, 1.4, 3.59), ("PK", 787.0, -0.5, 1.79), ("PK", 1640.0, 0.9, 3.41)]),
    ("HyperX Cloud II", [("LSC", 105.0, 4.1, 0.7), ("PK", 170.0, -7.0, 1.06), ("PK", 271.0, 3.7, 0.58), ("PK", 3828.0, 5.4, 3.6), ("PK", 57.0, -4.8, 0.58), ("HSC", 10000.0, -7.0, 0.7), ("PK", 8085.0, -3.0, 3.98), ("PK", 2378.0, -1.9, 2.78), ("PK", 3700.0, 1.3, 2.71), ("PK", 1123.0, 1.1, 3.29)]),
    ("HyperX Cloud Alpha", [("LSC", 105.0, 7.1, 0.7), ("PK", 104.0, -5.9, 0.23), ("PK", 733.0, 4.5, 0.92), ("PK", 4324.0, 6.7, 2.87), ("PK", 6110.0, -3.0, 3.21), ("HSC", 10000.0, -5.6, 0.7), ("PK", 41.0, -0.4, 2.0), ("PK", 159.0, 0.5, 3.01), ("PK", 212.0, -0.5, 3.66), ("PK", 1964.0, 0.5, 4.33)]),
    ("HIFIMAN Sundara", [("LSC", 105.0, 8.9, 0.7), ("PK", 56.0, -4.5, 0.4), ("PK", 971.0, -1.8, 1.62), ("PK", 2115.0, 2.1, 2.21), ("PK", 8270.0, 1.4, 0.96), ("HSC", 10000.0, -3.1, 0.7), ("PK", 246.0, -0.6, 1.21), ("PK", 128.0, 0.7, 1.44), ("PK", 59.0, -0.6, 2.26), ("PK", 35.0, 0.2, 1.56)]),
    ("Philips SHP9500", [("LSC", 105.0, 6.5, 0.7), ("PK", 5734.0, -5.7, 2.84), ("PK", 110.0, -3.0, 0.81), ("PK", 2130.0, 3.7, 2.76), ("PK", 412.0, 1.0, 1.39), ("HSC", 10000.0, -2.4, 0.7), ("PK", 1113.0, -0.6, 2.07), ("PK", 7370.0, 2.1, 5.98), ("PK", 4477.0, -1.7, 6.0), ("PK", 3566.0, 1.3, 6.0)]),
    ("SteelSeries Arctis Nova Pro Wireless (filaire)", [("LSC", 105.0, -4.1, 0.7), ("PK", 134.0, -5.8, 0.9), ("PK", 56.0, 4.4, 1.5), ("PK", 3993.0, 5.8, 3.24), ("PK", 341.0, 2.6, 2.32), ("HSC", 10000.0, -1.9, 0.7), ("PK", 6469.0, 3.7, 5.11), ("PK", 9152.0, 2.7, 2.58), ("PK", 1532.0, 2.0, 2.79), ("PK", 824.0, -1.4, 1.88)]),
];
pub fn headphone(name: &str) -> Option<&'static [Filter; 10]> {
    HEADPHONES.iter().find(|(n, _)| *n == name).map(|(_, f)| f)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HpEq {
    pub enabled: bool,
    pub headphone: Option<String>,
    pub style: Option<String>,
    pub bands: Vec<i32>,
    pub custom: IndexMap<String, Vec<i32>>,
    pub channels: Vec<String>,
    pub pause_live: bool,
}

fn bands_of(v: &Value) -> Vec<i32> {
    let mut b: Vec<i32> =
        v.as_array().map(|a| a.iter().take(10).map(|x| clamp(x, -12, 12).unwrap_or(0)).collect()).unwrap_or_default();
    b.resize(10, 0);
    b
}

pub fn normalize_hp_eq(src: &Value, base: Option<&HpEq>) -> HpEq {
    let mut eq = base.cloned().unwrap_or_else(|| HpEq {
        enabled: false,
        headphone: None,
        style: Some("neutre".into()),
        bands: vec![0; 10],
        custom: IndexMap::new(),
        channels: vec!["Game".into(), "Music".into(), "System".into(), "Chat".into()],
        pause_live: true,
    });
    if !src.is_object() {
        return eq;
    }
    if has(src, "enabled") {
        eq.enabled = truthy(get(src, "enabled"));
    }
    if has(src, "headphone") {
        eq.headphone = get(src, "headphone").as_str().filter(|h| headphone(h).is_some()).map(String::from);
    }
    if let Some(c) = get(src, "custom").as_object() {
        eq.custom = c
            .iter()
            .filter(|(k, _)| !k.trim().is_empty())
            .map(|(k, b)| (first_chars(k, 40), bands_of(b)))
            .collect();
    }
    if get(src, "bands").is_array() {
        eq.bands = bands_of(get(src, "bands"));
    }
    if has(src, "style") {
        let st = get(src, "style").as_str();
        eq.style = st
            .filter(|s| style(s).is_some() || s.strip_prefix("custom:").map(|n| eq.custom.contains_key(n)).unwrap_or(false))
            .map(String::from);
    }
    if let Some(ch) = get(src, "channels").as_array() {
        let want: Vec<&str> = ch.iter().filter_map(|c| c.as_str()).collect();
        eq.channels = HP_CHANNELS.iter().filter(|c| want.contains(c)).map(|c| c.to_string()).collect();
    }
    if has(src, "pause_live") {
        eq.pause_live = truthy(get(src, "pause_live"));
    }
    eq
}

/// Coefficients (b0, b1, b2, a0, a1, a2) d'un filtre RBJ, comme Equalizer APO.
pub fn biquad(kind: &str, f0: f64, gain: f64, q: f64, fs: f64) -> [f64; 6] {
    let a = 10f64.powf(gain / 40.0);
    let w0 = 2.0 * std::f64::consts::PI * f0.min(fs * 0.49) / fs;
    let (cw, alpha) = (w0.cos(), w0.sin() / (2.0 * q));
    let s = 2.0 * a.sqrt() * alpha;
    match kind {
        "LSC" => [
            a * ((a + 1.0) - (a - 1.0) * cw + s), 2.0 * a * ((a - 1.0) - (a + 1.0) * cw), a * ((a + 1.0) - (a - 1.0) * cw - s),
            (a + 1.0) + (a - 1.0) * cw + s, -2.0 * ((a - 1.0) + (a + 1.0) * cw), (a + 1.0) + (a - 1.0) * cw - s,
        ],
        "HSC" => [
            a * ((a + 1.0) + (a - 1.0) * cw + s), -2.0 * a * ((a - 1.0) + (a + 1.0) * cw), a * ((a + 1.0) + (a - 1.0) * cw - s),
            (a + 1.0) - (a - 1.0) * cw + s, 2.0 * ((a - 1.0) - (a + 1.0) * cw), (a + 1.0) - (a - 1.0) * cw - s,
        ],
        _ => [1.0 + alpha * a, -2.0 * cw, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cw, 1.0 - alpha / a],
    }
}

/// Reponse totale (dB) d'une chaine de filtres aux frequences donnees.
pub fn response_db(filters: &[(String, f64, f64, f64)], freqs: &[f64], fs: f64) -> Vec<f64> {
    let coefs: Vec<[f64; 6]> = filters.iter().map(|(k, f, g, q)| biquad(k, *f, *g, *q, fs)).collect();
    freqs
        .iter()
        .map(|f| {
            let w = -2.0 * std::f64::consts::PI * f / fs;
            let (z1r, z1i) = (w.cos(), w.sin());
            let (z2r, z2i) = ((2.0 * w).cos(), (2.0 * w).sin());
            coefs
                .iter()
                .map(|c| {
                    let (nr, ni) = (c[0] + c[1] * z1r + c[2] * z2r, c[1] * z1i + c[2] * z2i);
                    let (dr, di) = (c[3] + c[4] * z1r + c[5] * z2r, c[4] * z1i + c[5] * z2i);
                    let mag = ((nr * nr + ni * ni) / (dr * dr + di * di)).sqrt();
                    20.0 * mag.max(1e-12).log10()
                })
                .sum()
        })
        .collect()
}

pub fn hp_grid() -> Vec<f64> {
    (0..200).map(|i| 20.0 * 1000f64.powf(i as f64 / 199.0)).collect()
}

/// Filtres a appliquer (correction du casque puis style) et preampli qui evite toute saturation.
pub fn hp_filters(eq: &HpEq) -> (Vec<(String, f64, f64, f64)>, f64) {
    let mut flt: Vec<(String, f64, f64, f64)> = eq
        .headphone
        .as_deref()
        .and_then(headphone)
        .map(|fs| fs.iter().map(|(k, f, g, q)| (k.to_string(), *f, *g, *q)).collect())
        .unwrap_or_default();
    for (f, g) in HP_BANDS.iter().zip(eq.bands.iter()) {
        if *g != 0 {
            flt.push(("PK".into(), *f as f64, *g as f64, HP_Q));
        }
    }
    let peak = if flt.is_empty() {
        0.0
    } else {
        response_db(&flt, &hp_grid(), 48000.0).into_iter().fold(f64::MIN, f64::max)
    };
    let pre = -peak.max(0.0) - if peak > 0.0 { 0.3 } else { 0.0 };
    (flt, (pre * 10.0).round() / 10.0)
}

fn style_label(eq: &HpEq) -> String {
    match eq.style.as_deref() {
        Some(s) if style(s).is_some() => style(s).unwrap().label.to_string(),
        Some(s) if s.starts_with("custom:") => format!("profil {}", &s[7..]),
        _ => "reglage perso".into(),
    }
}

/// Contenu de rustudio.txt (ASCII seulement : Equalizer APO lit le fichier tel quel).
pub fn config_text(eq: &HpEq, active: bool, why: &str) -> String {
    let mut lines = vec!["# Rustudio : egaliseur du casque. Fichier genere : regle-le dans Rustudio (page Casque).".to_string()];
    if !active {
        lines.push(format!("# Egaliseur coupe{}.", if why.is_empty() { String::new() } else { format!(" ({why})") }));
        return lines.join("\n") + "\n";
    }
    let (flt, pre) = hp_filters(eq);
    lines.push(format!(
        "# Casque : {} / style : {}",
        eq.headphone.as_deref().unwrap_or("sans correction"),
        style_label(eq)
    ));
    lines.push(format!(
        "Device: {}",
        eq.channels.iter().map(|c| format!("GoXLR {c}")).collect::<Vec<_>>().join("; ")
    ));
    lines.push(format!("Preamp: {pre:.1} dB"));
    for (i, (kind, f0, g, q)) in flt.iter().enumerate() {
        lines.push(format!("Filter {}: ON {} Fc {} Hz Gain {:.1} dB Q {:.2}", i + 1, kind, f0, g, q));
    }
    lines.push("Device: all".into());
    let text = lines.join("\n") + "\n";
    text.chars().map(|c| if c.is_ascii() { c } else { '?' }).collect()
}
