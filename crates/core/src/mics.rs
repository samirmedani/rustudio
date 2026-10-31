//! Catalogue des micros connus : type, alimentation, sensibilité, conseils. Chiffres des fiches officielles ;
//! la courbe « response » (écart à une réponse plate) n'est donnée que lorsqu'elle est publiée par la marque.
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct MicModel {
    pub key: &'static str,
    pub brand: &'static str,
    pub name: &'static str,
    /// type pour la GoXLR : "dynamic", "condenser" (48 V) ou "jack"
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub phantom: bool,
    /// sensibilité à 1 kHz (mV/Pa)
    pub sensitivity_mv_pa: Option<f64>,
    /// bruit propre (dB, pondéré A)
    pub self_noise_dba: Option<f64>,
    pub max_spl_db: Option<f64>,
    pub pattern: &'static str,
    /// distance de parole conseillée (cm)
    pub distance_cm: [u8; 2],
    /// aigus relevés : correction des aigus et de-esser plus marqués
    pub bright: bool,
    /// écart à une réponse plate (Hz, dB), lu sur la courbe publiée : sert là où la mesure de ta voix manque
    pub response: &'static [(f64, f64)],
    pub tips: &'static [&'static str],
    pub generic: bool,
    /// fiche officielle d'où viennent les chiffres
    pub source: Option<&'static str>,
}

const NT1A_RESPONSE: &[(f64, f64)] = &[
    (20.0, -2.0), (50.0, -0.5), (100.0, 0.0), (1000.0, 0.0), (2000.0, 0.5), (3000.0, 1.0), (5000.0, 2.0),
    (8000.0, 3.0), (10000.0, 4.0), (12000.0, 4.0), (15000.0, 2.5), (18000.0, 0.0), (20000.0, -2.0),
];

pub const MODELS: &[MicModel] = &[
    MicModel {
        key: "rode-nt1a", brand: "RØDE", name: "NT1-A", kind: "condenser", phantom: true, sensitivity_mv_pa: Some(25.0),
        self_noise_dba: Some(5.0), max_spl_db: Some(137.0), pattern: "cardioïde", distance_cm: [10, 20], bright: true,
        response: NT1A_RESPONSE,
        tips: &[
            "Parle face au point doré, à 10–20 cm, le filtre anti-pop entre ta bouche et la grille.",
            "Il entend toute la pièce : tourne son dos vers le PC et le clavier, et garde toujours la même distance.",
            "Le 48 V est obligatoire : Rustudio l’allume pour toi.",
        ],
        generic: false, source: Some("https://rode.com/en-us/products/nt1a"),
    },
    MicModel {
        key: "rode-nt1-5", brand: "RØDE", name: "NT1 (5ᵉ génération)", kind: "condenser", phantom: true,
        sensitivity_mv_pa: Some(25.0), self_noise_dba: Some(4.0), max_spl_db: None, pattern: "cardioïde",
        distance_cm: [10, 20], bright: false, response: &[],
        tips: &[
            "Branche-le en XLR sur la GoXLR : en USB-C, il ne passe pas par elle.",
            "Parle à 10–20 cm avec le filtre anti-pop, son dos tourné vers le PC et le clavier.",
            "Le 48 V est obligatoire : Rustudio l’allume pour toi.",
        ],
        generic: false, source: Some("https://edge.rode.com/pdf/products/1184/NT1%205th%20GEN_Datasheet.pdf"),
    },
    MicModel {
        key: "at2020", brand: "Audio-Technica", name: "AT2020", kind: "condenser", phantom: true,
        sensitivity_mv_pa: Some(14.1), self_noise_dba: Some(20.0), max_spl_db: Some(144.0), pattern: "cardioïde",
        distance_cm: [10, 20], bright: false, response: &[],
        tips: &[
            "Parle à 10–20 cm, face à la grille, avec un filtre anti-pop.",
            "Il entend la pièce : éloigne-le du PC et du clavier.",
            "Le 48 V est obligatoire : Rustudio l’allume pour toi.",
        ],
        generic: false, source: Some("https://docs.audio-technica.com/us/at2020_submit.pdf"),
    },
    MicModel {
        key: "rode-podmic", brand: "RØDE", name: "PodMic", kind: "dynamic", phantom: false, sensitivity_mv_pa: Some(1.6),
        self_noise_dba: None, max_spl_db: None, pattern: "cardioïde", distance_cm: [5, 10], bright: false, response: &[],
        tips: &[
            "Parle tout près, à 5–10 cm, face à la grille : il ignore le bruit de la pièce.",
            "Il demande beaucoup de gain : c’est normal pour un micro dynamique.",
        ],
        generic: false, source: Some("https://edge.rode.com/pdf/products/108/PodMic_Datasheet_FA.pdf"),
    },
    MicModel {
        key: "shure-sm7b", brand: "Shure", name: "SM7B", kind: "dynamic", phantom: false, sensitivity_mv_pa: Some(1.12),
        self_noise_dba: None, max_spl_db: None, pattern: "cardioïde", distance_cm: [3, 10], bright: false, response: &[],
        tips: &[
            "Parle tout près, à 3–10 cm, avec la bonnette fournie.",
            "Laisse ses interrupteurs arrière à plat : Rustudio corrige à l’égaliseur.",
            "Il demande beaucoup de gain (55 à 60 dB) : c’est normal.",
        ],
        generic: false, source: Some("https://pubs.shure.com/view/guide/SM7B/en-US.pdf"),
    },
    MicModel {
        key: "shure-sm58", brand: "Shure", name: "SM58", kind: "dynamic", phantom: false, sensitivity_mv_pa: Some(1.85),
        self_noise_dba: None, max_spl_db: None, pattern: "cardioïde", distance_cm: [5, 10], bright: false, response: &[],
        tips: &["Parle à 5–10 cm, dans l’axe de la grille.", "Il demande beaucoup de gain : c’est normal pour un micro dynamique."],
        generic: false, source: Some("https://pubs.shure.com/file/260009"),
    },
    MicModel {
        key: "generic-dynamic", brand: "", name: "Autre micro dynamique", kind: "dynamic", phantom: false,
        sensitivity_mv_pa: None, self_noise_dba: None, max_spl_db: None, pattern: "", distance_cm: [5, 15], bright: false,
        response: &[], tips: &["Parle à 5–15 cm, face à la grille."], generic: true, source: None,
    },
    MicModel {
        key: "generic-condenser", brand: "", name: "Autre micro statique (48 V)", kind: "condenser", phantom: true,
        sensitivity_mv_pa: None, self_noise_dba: None, max_spl_db: None, pattern: "", distance_cm: [10, 20], bright: false,
        response: &[], tips: &["Parle à 10–20 cm avec un filtre anti-pop, son dos tourné vers le PC et le clavier."],
        generic: true, source: None,
    },
    MicModel {
        key: "generic-jack", brand: "", name: "Micro de casque (jack 3,5 mm)", kind: "jack", phantom: false,
        sensitivity_mv_pa: None, self_noise_dba: None, max_spl_db: None, pattern: "", distance_cm: [2, 5], bright: false,
        response: &[], tips: &["Place la tige à 2–5 cm du coin de ta bouche, pas devant : moins de souffle."],
        generic: true, source: None,
    },
];

pub fn model(key: &str) -> Option<&'static MicModel> {
    MODELS.iter().find(|m| m.key == key)
}

/// Gain de départ sur la Mini : 57 dB pour un SM7B (1,12 mV/Pa), moins quand le micro est plus sensible.
pub fn start_gain_db(m: &MicModel) -> i32 {
    match m.sensitivity_mv_pa {
        Some(s) if s > 0.0 => crate::value::clamp_f(57.0 - 20.0 * (s / 1.12).log10(), 0, 72),
        _ => crate::consts::recommended_gain(m.kind),
    }
}

/// Écart du micro à une réponse plate (dB) à une fréquence, interpolé sur une échelle logarithmique ; 0 sans courbe.
pub fn response_db(m: &MicModel, f: f64) -> f64 {
    let r = m.response;
    if r.is_empty() || !(f > 0.0) {
        return 0.0;
    }
    if f <= r[0].0 {
        return r[0].1;
    }
    for w in r.windows(2) {
        let ((f0, g0), (f1, g1)) = (w[0], w[1]);
        if f <= f1 {
            let t = (f / f0).ln() / (f1 / f0).ln();
            return g0 + t * (g1 - g0);
        }
    }
    r[r.len() - 1].1
}

/// Ce que lit l'interface (S.meta.mic_models).
pub fn catalog_json() -> Value {
    let mut out = serde_json::Map::new();
    for m in MODELS {
        let mut v = serde_json::to_value(m).unwrap_or(Value::Null);
        if let Some(o) = v.as_object_mut() {
            o.insert("start_gain_db".into(), json!(start_gain_db(m)));
            o.remove("key");
            o.remove("response");
        }
        out.insert(m.key.into(), v);
    }
    Value::Object(out)
}

/// Modèle générique d'un type GoXLR.
pub fn generic_for(kind: &str) -> &'static str {
    match kind {
        "condenser" => "generic-condenser",
        "jack" => "generic-jack",
        _ => "generic-dynamic",
    }
}
