//! Traitements du micro : preregles, calage sur la voix mesuree, interrupteurs, ce qui part vraiment.
use crate::consts::{EQ_BANDS, GATE_ATTENUATION, RATIOS};
use crate::state::MicCal;
use crate::value::{clamp, clamp_f, get, has, truthy};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gate {
    pub enabled: bool,
    pub threshold: i32,
    pub attenuation: i32,
    pub attack: i32,
    pub release: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Comp {
    pub threshold: i32,
    pub ratio: i32,
    pub attack: i32,
    pub release: i32,
    pub makeup: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dsp {
    pub eq: IndexMap<String, i32>,
    pub gate: Gate,
    pub comp: Comp,
    pub deesser: i32,
}

/// Interrupteurs des traitements. Tous coupes par defaut : voix naturelle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Fx {
    pub eq: bool,
    pub gate: bool,
    pub comp: bool,
    pub deesser: bool,
}

impl Fx {
    pub fn any(&self) -> bool {
        self.eq || self.gate || self.comp || self.deesser
    }
    pub fn get(&self, k: &str) -> Option<bool> {
        match k {
            "eq" => Some(self.eq),
            "gate" => Some(self.gate),
            "comp" => Some(self.comp),
            "deesser" => Some(self.deesser),
            _ => None,
        }
    }
    pub fn set(&mut self, k: &str, on: bool) {
        match k {
            "eq" => self.eq = on,
            "gate" => self.gate = on,
            "comp" => self.comp = on,
            "deesser" => self.deesser = on,
            _ => {}
        }
    }
}

pub const FX_KEYS: [&str; 4] = ["eq", "gate", "comp", "deesser"];
/// attenuation max du noise gate des preregles (sauf Anti-bruit) : jamais de coupure seche
pub const GATE_SOFT_DB: f64 = 24.0;
/// bruit de la piece une fois le noise gate ferme (apres compensation) : inaudible
pub const RESIDUAL_NOISE_DB: f64 = -75.0;

pub fn make_dsp(eq: [i32; 6], gate: (bool, i32, i32, i32, i32), comp: [i32; 5], deesser: i32) -> Dsp {
    let mut m = IndexMap::new();
    for (i, (b, _, _)) in EQ_BANDS.iter().enumerate() {
        m.insert(b.to_string(), eq[i]);
    }
    Dsp {
        eq: m,
        gate: Gate { enabled: gate.0, threshold: gate.1, attenuation: gate.2, attack: gate.3, release: gate.4 },
        comp: Comp { threshold: comp[0], ratio: comp[1], attack: comp[2], release: comp[3], makeup: comp[4] },
        deesser,
    }
}

fn default_gate() -> (bool, i32, i32, i32, i32) {
    (true, -55, 40, 0, 19)
}
fn default_comp() -> [i32; 5] {
    [-12, 6, 1, 9, 2]
}

pub struct Preset {
    pub key: &'static str,
    pub label: &'static str,
    pub desc: &'static str,
    pub dsp: fn() -> Dsp,
}

pub const MIC_PRESETS: [Preset; 7] = [
    Preset { key: "brut", label: "Naturel", desc: "Aucun traitement : ta voix telle quelle. Active ensuite ce que tu veux.",
             dsp: || make_dsp([0; 6], (false, -59, 0, 0, 19), [0, 0, 1, 9, 0], 0) },
    Preset { key: "naturel", label: "Léger", desc: "Ta voix juste stabilisée, traitements doux.",
             dsp: || make_dsp([0; 6], default_gate(), default_comp(), 0) },
    Preset { key: "studio", label: "Studio", desc: "Voix de studio, nette et posée. Faite pour les micros statiques.",
             dsp: || make_dsp([-3, -2, -2, 0, 2, 1], (true, -45, 96, 0, 20), [-24, 8, 3, 11, 6], 35) },
    Preset { key: "radio", label: "Radio", desc: "Voix pleine et présente, comme à l’antenne.",
             dsp: || make_dsp([3, -2, -2, 0, 3, 2], (true, -48, 70, 0, 19), [-22, 9, 1, 9, 6], 30) },
    Preset { key: "stream", label: "Stream", desc: "Claire et régulière, idéale en live.",
             dsp: || make_dsp([1, -1, -1, 1, 2, 2], (true, -45, 80, 0, 19), [-18, 8, 1, 9, 4], 20) },
    Preset { key: "podcast", label: "Podcast", desc: "Chaleureuse et posée pour les longues discussions.",
             dsp: || make_dsp([2, 0, -1, 0, 1, 1], (true, -50, 50, 0, 19), [-20, 7, 1, 9, 4], 15) },
    Preset { key: "anti-bruit", label: "Anti-bruit", desc: "Coupe le clavier, le ventilo et le bruit de la pièce.",
             dsp: || make_dsp([-3, -1, 0, 1, 2, 1], (true, -35, 100, 0, 15), [-18, 8, 1, 9, 4], 25) },
];

pub fn preset(name: &str) -> Option<&'static Preset> {
    MIC_PRESETS.iter().find(|p| p.key == name)
}
pub fn preset_dsp(name: &str) -> Dsp {
    (preset(name).unwrap_or(&MIC_PRESETS[1]).dsp)()
}
pub fn is_preset(name: &str) -> bool {
    preset(name).is_some()
}

pub fn mic_type_preset(t: &str) -> &'static str {
    match t {
        "dynamic" => "radio",
        "condenser" => "studio",
        _ => "stream",
    }
}

/// Avec une voix mesuree, decalage du noise gate et du compresseur (dB), attenuation du gate (None : selon le
/// bruit mesure), part du gain perdu en compression rendue par la compensation.
pub struct Fit {
    pub gate: f64,
    pub att: Option<i32>,
    pub comp: f64,
    pub makeup: f64,
}
pub fn preset_fit(name: &str) -> Option<Fit> {
    Some(match name {
        "studio" => Fit { gate: 0.0, att: None, comp: 0.0, makeup: 0.7 },
        "radio" => Fit { gate: 0.0, att: None, comp: -3.0, makeup: 0.8 },
        "stream" => Fit { gate: 0.0, att: None, comp: -1.0, makeup: 0.7 },
        "podcast" => Fit { gate: -2.0, att: Some(80), comp: 1.0, makeup: 0.6 },
        "naturel" => Fit { gate: -4.0, att: Some(70), comp: 4.0, makeup: 0.5 },
        "anti-bruit" => Fit { gate: 3.0, att: Some(100), comp: 0.0, makeup: 0.7 },
        _ => return None,
    })
}

/// Attenuation du gate (dB, negative) pour un pourcentage GoXLR.
pub fn gate_attenuation(percent: i32) -> i32 {
    let p = percent.clamp(0, 100);
    if p > 99 {
        GATE_ATTENUATION[25]
    } else {
        GATE_ATTENUATION[(p as f64 * 0.24) as usize]
    }
}

/// Seuil du noise gate : au-dessus du bruit de la piece, bien sous les passages calmes de la voix.
pub fn gate_threshold(noise_peak: f64, voice_low: f64) -> f64 {
    let (lo, hi) = (noise_peak + 6.0, voice_low - 10.0);
    if lo > hi {
        return (noise_peak + voice_low) / 2.0;
    }
    (noise_peak + 6.0_f64.max(12.0_f64.min(0.4 * (voice_low - noise_peak)))).min(hi)
}

/// Plus douce attenuation (en % GoXLR) qui retire au moins need_db.
pub fn gate_att_percent(need_db: f64, least_db: f64) -> i32 {
    let need = least_db.max(need_db);
    for p in 0..100 {
        if -(gate_attenuation(p) as f64) >= need {
            return p;
        }
    }
    100
}

/// Reglages d'un preregle, cales sur la voix mesuree si elle existe.
pub fn fit_dsp(name: &str, cal: Option<&MicCal>, gain_db: Option<i32>) -> Dsp {
    let mut d = preset_dsp(name);
    let (Some(cal), Some(fit), Some(gain)) = (cal, preset_fit(name), gain_db) else {
        return d;
    };
    let shift = (gain - cal.gain_db) as f64;
    let (noise, low, med, peak) =
        (cal.noise_peak_db + shift, cal.voice_low_db + shift, cal.voice_db + shift, cal.voice_peak_db + shift);
    d.gate.threshold = clamp_f(gate_threshold(noise, low) + fit.gate, -59, -10);
    let thr = clamp_f(med - 4.0 + fit.comp, -40, -6);
    d.comp.threshold = thr;
    let ratio = RATIOS[d.comp.ratio.clamp(0, 14) as usize];
    d.comp.makeup = clamp_f((peak - thr as f64).max(0.0) * (1.0 - 1.0 / ratio) * fit.makeup, 0, 12);
    d.gate.attenuation = match fit.att {
        Some(a) => a,
        None => gate_att_percent(
            GATE_SOFT_DB.min(noise.max(-72.2) + d.comp.makeup as f64 - RESIDUAL_NOISE_DB),
            15.0,
        ),
    };
    d
}

pub fn normalize_fx(src: &Value) -> Fx {
    let mut fx = Fx::default();
    if src.is_object() {
        for k in FX_KEYS {
            fx.set(k, truthy(get(src, k)));
        }
    }
    fx
}

/// Traitements qu'utilise un preregle.
pub fn fx_from_dsp(d: &Dsp) -> Fx {
    Fx {
        eq: d.eq.values().any(|v| *v != 0),
        gate: d.gate.enabled,
        comp: d.comp.ratio > 0,
        deesser: d.deesser > 0,
    }
}

/// Ce qui part vraiment vers la GoXLR : un traitement coupe est neutre.
pub fn effective_dsp(d: &Dsp, fx: &Fx) -> Dsp {
    let mut e = d.clone();
    e.gate.enabled = fx.gate;
    if !fx.comp {
        e.comp.threshold = 0;
        e.comp.ratio = 0;
        e.comp.makeup = 0;
    }
    if !fx.eq {
        for v in e.eq.values_mut() {
            *v = 0;
        }
    }
    if !fx.deesser {
        e.deesser = 0;
    }
    e
}

pub fn normalize_dsp(src: &Value, base: Option<&Dsp>) -> Dsp {
    let mut d = base.cloned().unwrap_or_else(|| preset_dsp("naturel"));
    if let Some(eq) = get(src, "eq").as_object() {
        for (band, v) in eq {
            if crate::consts::is_eq_band(band) {
                if let Some(x) = clamp(v, -9, 9) {
                    d.eq.insert(band.clone(), x);
                }
            }
        }
    }
    let g = get(src, "gate");
    if has(g, "enabled") {
        d.gate.enabled = truthy(get(g, "enabled"));
    }
    for (k, lo, hi) in [("threshold", -59, 0), ("attenuation", 0, 100), ("attack", 0, 45), ("release", 0, 45)] {
        if let Some(x) = clamp(get(g, k), lo, hi) {
            match k {
                "threshold" => d.gate.threshold = x,
                "attenuation" => d.gate.attenuation = x,
                "attack" => d.gate.attack = x,
                _ => d.gate.release = x,
            }
        }
    }
    let c = get(src, "comp");
    for (k, lo, hi) in [("threshold", -40, 0), ("ratio", 0, 14), ("attack", 0, 19), ("release", 0, 19), ("makeup", -6, 24)] {
        if let Some(x) = clamp(get(c, k), lo, hi) {
            match k {
                "threshold" => d.comp.threshold = x,
                "ratio" => d.comp.ratio = x,
                "attack" => d.comp.attack = x,
                "release" => d.comp.release = x,
                _ => d.comp.makeup = x,
            }
        }
    }
    if let Some(x) = clamp(get(src, "deesser"), 0, 100) {
        d.deesser = x;
    }
    d
}

pub fn ratio_x(ratio: i32) -> f64 {
    RATIOS[ratio.clamp(0, 14) as usize]
}
