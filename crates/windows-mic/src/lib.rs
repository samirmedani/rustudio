//! Module « Micro » : n'importe quel micro de Windows (USB, jack, micro-casque, webcam), réglé depuis Rustudio.
//!
//! Les réglages et préréglages sont associés à l'identifiant Windows de chaque entrée. Le moteur publie
//! les commandes de l'effet et reçoit ses vumètres par mémoire partagée. Sous Windows, ce module gère
//! aussi le branchement, la restauration et leur vérification sur le périphérique de capture.
//! Les configurations créées avec Equalizer APO sont conservées jusqu'à la migration demandée.
//!
//! La voix reste naturelle par défaut : aucun traitement tant que l'utilisateur ne l'a pas activé pour ce micro.
mod dynamics;
pub use dynamics::Dynamics;
pub mod control;
use rustudio_core::micns::is_guid;
use rustudio_core::value::{clamp, get, has, truthy};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const ID: &str = "windows-mic";
/// Fichier d'Equalizer APO réservé à la voix des micros (inclus par config.txt après celui de RNNoise : le bruit
/// est retiré avant l'égaliseur, comme dans OBS).
pub const CONFIG_FILE: &str = "rustudio-micro-voix.txt";
/// Réglages retenus au plus : un par micro réglé un jour.
pub const MAX_MICS: usize = 16;
/// Gain ajouté par Rustudio, après le volume de Windows (dB).
pub const GAIN_RANGE: (i32, i32) = (-12, 12);
/// Bornes de chaque bande de l'égaliseur (dB).
pub const EQ_RANGE: (i32, i32) = (-9, 9);
/// Fréquence du coupe-bas (Hz).
pub const HPF_RANGE: (i32, i32) = (40, 200);
pub const HPF_DEFAULT: i32 = 80;
/// Pente du coupe-bas : Butterworth, 12 dB par octave.
const HPF_Q: f64 = 0.71;

/// Bande de l'égaliseur : filtre d'Equalizer APO (étagère ou cloche), fréquence et largeur fixes.
pub struct Band {
    pub key: &'static str,
    pub kind: &'static str,
    pub hz: f64,
    pub q: f64,
    pub label: &'static str,
    pub hint: &'static str,
}

/// Cinq bandes choisies pour la voix parlée, chacune avec un effet qu'on entend et qu'on peut nommer.
pub const BANDS: [Band; 5] = [
    Band { key: "low", kind: "LSC", hz: 120.0, q: 0.71, label: "Graves", hint: "Chaleur, voix posée" },
    Band { key: "mud", kind: "PK", hz: 320.0, q: 1.0, label: "Bas-médiums", hint: "Effet carton, voix étouffée" },
    Band { key: "mid", kind: "PK", hz: 1000.0, q: 1.1, label: "Médiums", hint: "Voix nasale ou en avant" },
    Band { key: "presence", kind: "PK", hz: 3500.0, q: 1.0, label: "Présence", hint: "Clarté, voix qui passe" },
    Band { key: "air", kind: "HSC", hz: 10000.0, q: 0.71, label: "Air", hint: "Brillance, souffle" },
];

/// Préréglage de la voix : coupe-bas (0 : coupé) et égaliseur. Le gain reste celui choisi par l'utilisateur, qui
/// dépend de son micro et pas du son voulu.
pub struct Preset {
    pub id: &'static str,
    pub label: &'static str,
    pub desc: &'static str,
    pub hpf_hz: i32,
    pub eq: [i32; 5],
}

pub const PRESETS: [Preset; 6] = [
    Preset { id: "natural", label: "Naturel", desc: "Ta voix telle quelle, sans les grondements du bureau.", hpf_hz: 80, eq: [0, 0, 0, 0, 0] },
    Preset { id: "streamer", label: "Streamer", desc: "Atténue les pauses et garde la voix régulière pendant les appels.", hpf_hz: 80, eq: [0, -2, 0, 2, 0] },
    Preset { id: "warm", label: "Voix chaude", desc: "Plus de corps et de rondeur, idéal pour un micro fin.", hpf_hz: 80, eq: [3, -1, 0, 1, 0] },
    Preset { id: "radio", label: "Radio", desc: "Graves pleins et voix en avant, comme un animateur radio.", hpf_hz: 100, eq: [4, -3, -1, 3, 2] },
    Preset { id: "clear", label: "Clair", desc: "Voix nette pour Discord et les visios, sans effet carton.", hpf_hz: 100, eq: [-1, -3, 0, 3, 1] },
    Preset { id: "soft", label: "Adoucir", desc: "Calme un micro brillant et les « s » qui sifflent.", hpf_hz: 80, eq: [1, 0, 0, -2, -4] },
];

pub fn preset(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.id == id)
}

/// Réglage de la voix d'un micro.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Voice {
    /// Traitement de voix actif sur ce micro, coupé par défaut.
    pub enabled: bool,
    pub dynamics: Dynamics,
    pub noise: bool,
    pub strength: i32,
    pub grace_ms: i32,
    /// L'utilisateur avait demandé le branchement. Permet de signaler sa perte après une mise à jour.
    pub native_requested: bool,
    pub safe_mode: bool,
    pub gain_db: i32,
    pub hpf: bool,
    pub hpf_hz: i32,
    /// Gains des cinq bandes, dans l'ordre de `BANDS`.
    pub eq: [i32; 5],
    /// Dernier préréglage appliqué tel quel ; aucun dès qu'un réglage s'en écarte.
    pub preset: Option<String>,
}

impl Default for Voice {
    fn default() -> Self {
        Voice { enabled: false, dynamics: Dynamics::default(), noise: false, strength: 90, grace_ms: 200, native_requested: false, safe_mode: false, gain_db: 0, hpf: true, hpf_hz: HPF_DEFAULT, eq: [0; 5], preset: Some("natural".into()) }
    }
}

impl Voice {
    /// Applique le coupe-bas, l'égaliseur et la dynamique du préréglage en conservant le gain et l'activation.
    pub fn apply_preset(&mut self, id: &str) -> Result<(), String> {
        let p = preset(id).ok_or_else(|| format!("Préréglage inconnu : {id}"))?;
        self.hpf = p.hpf_hz > 0;
        if p.hpf_hz > 0 {
            self.hpf_hz = p.hpf_hz;
        }
        self.eq = p.eq;
        self.dynamics = Dynamics::preset(id);
        self.preset = Some(p.id.into());
        Ok(())
    }

    /// Filtres d'Equalizer APO dans l'ordre : coupe-bas, puis les bandes non nulles.
    pub fn filters(&self) -> Vec<(&'static str, f64, f64, f64)> {
        let mut out = vec![];
        if self.hpf {
            out.push(("HPQ", f64::from(self.hpf_hz.clamp(HPF_RANGE.0, HPF_RANGE.1)), 0.0, HPF_Q));
        }
        for (band, gain) in BANDS.iter().zip(self.eq) {
            let gain = gain.clamp(EQ_RANGE.0, EQ_RANGE.1);
            if gain != 0 {
                out.push((band.kind, band.hz, f64::from(gain), band.q));
            }
        }
        out
    }

    /// Courbe du réglage (dB) aux fréquences données, gain compris : ce que l'égaliseur fait à ta voix.
    pub fn response_db(&self, freqs: &[f64]) -> Vec<f64> {
        const FS: f64 = 48000.0;
        let bands: Vec<(String, f64, f64, f64)> =
            self.filters().into_iter().filter(|f| f.0 != "HPQ").map(|(k, f, g, q)| (k.to_string(), f, g, q)).collect();
        let eq = rustudio_core::hpeq::response_db(&bands, freqs, FS);
        let gain = f64::from(self.gain_db.clamp(GAIN_RANGE.0, GAIN_RANGE.1));
        freqs.iter().zip(eq).map(|(f, db)| db + gain + if self.hpf { high_pass_db(f64::from(self.hpf_hz), *f) } else { 0.0 }).collect()
    }
}

/// Atténuation (dB) d'un passe-haut Butterworth du second ordre à la fréquence `f`.
fn high_pass_db(fc: f64, f: f64) -> f64 {
    let r = (f / fc.max(1.0)).powi(4);
    10.0 * (r / (1.0 + r)).max(1e-12).log10()
}

/// Réglages du module : le micro affiché, et la voix de chaque micro (par identifiant Windows).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub selected: Option<String>,
    pub mics: BTreeMap<String, Voice>,
}

impl Settings {
    pub fn voice(&self, guid: &str) -> Voice {
        self.mics.get(&guid.to_lowercase()).cloned().unwrap_or_default()
    }

    pub fn store_voice(&mut self, guid: String, voice: Voice) -> Result<(), String> {
        if !is_guid(&guid) { return Err("Micro inconnu.".into()); }
        if self.mics.len() >= MAX_MICS && !self.mics.contains_key(&guid) {
            let old = self.mics.iter().find(|(g,v)| !v.enabled && !v.native_requested && !v.safe_mode && self.selected.as_ref() != Some(*g))
                .map(|(g,_)|g.clone()).ok_or("Tous les emplacements micro sont utilisés.")?;
            self.mics.remove(&old);
        }
        self.mics.insert(guid,voice);
        Ok(())
    }

    /// Relit des réglages enregistrés : identifiants invalides écartés, valeurs bornées.
    pub fn sanitized(mut self) -> Settings {
        self.selected = self.selected.filter(|g| is_guid(g)).map(|g| g.to_lowercase());
        let mics = std::mem::take(&mut self.mics);
        for (guid, mut v) in mics.into_iter().filter(|(g, _)| is_guid(g)).take(MAX_MICS) {
            v.gain_db = v.gain_db.clamp(GAIN_RANGE.0, GAIN_RANGE.1);
            v.hpf_hz = v.hpf_hz.clamp(HPF_RANGE.0, HPF_RANGE.1);
            v.eq = v.eq.map(|g| g.clamp(EQ_RANGE.0, EQ_RANGE.1));
            v.dynamics.sanitize();
            v.strength = v.strength.clamp(0,99);
            v.grace_ms = v.grace_ms.clamp(0,500);
            v.preset = v.preset.filter(|p| preset(p).is_some());
            self.mics.insert(guid.to_lowercase(), v);
        }
        self
    }
}

fn guid_of(v: &Value) -> Result<String, String> {
    v.as_str().map(str::trim).filter(|g| is_guid(g)).map(str::to_lowercase).ok_or_else(|| "Micro inconnu.".to_string())
}

/// Applique une modification partielle venue de l'appli. `selected` choisit le micro affiché ; les champs de la
/// voix s'appliquent à `device`, sinon au micro affiché.
pub fn update(src: &Value, base: &Settings) -> Result<Settings, String> {
    if !src.is_object() {
        return Err("Réglage illisible.".into());
    }
    let mut s = base.clone();
    if has(src, "selected") {
        s.selected = Some(guid_of(get(src, "selected"))?);
    }
    let voice_keys = ["enabled", "gain_db", "hpf", "hpf_hz", "eq", "preset", "dynamics"];
    if !voice_keys.iter().any(|k| has(src, k)) {
        return Ok(s);
    }
    let guid = if has(src, "device") { guid_of(get(src, "device"))? } else { s.selected.clone().ok_or("Choisis d’abord un micro.")? };
    let mut v = s.voice(&guid);
    if has(src, "preset") {
        let id = get(src, "preset").as_str().ok_or("Préréglage inconnu.")?;
        v.apply_preset(id)?;
    }
    let mut custom = false;
    if has(src, "enabled") {
        v.enabled = truthy(get(src, "enabled"));
    }
    if let Some(g) = clamp(get(src, "gain_db"), GAIN_RANGE.0, GAIN_RANGE.1) {
        v.gain_db = g;
    }
    if has(src, "hpf") {
        let on = truthy(get(src, "hpf"));
        custom |= on != v.hpf;
        v.hpf = on;
    }
    if let Some(hz) = clamp(get(src, "hpf_hz"), HPF_RANGE.0, HPF_RANGE.1) {
        custom |= hz != v.hpf_hz;
        v.hpf_hz = hz;
    }
    let eq = get(src, "eq");
    for (i, band) in BANDS.iter().enumerate() {
        let value = if eq.is_array() { eq.get(i) } else { eq.get(band.key) };
        if let Some(g) = value.and_then(|x| clamp(x, EQ_RANGE.0, EQ_RANGE.1)) {
            custom |= g != v.eq[i];
            v.eq[i] = g;
        }
    }
    if let Some(patch) = src.get("dynamics") {
        let next = v.dynamics.updated(patch)?;
        custom |= next != v.dynamics;
        v.dynamics = next;
    }
    if custom {
        v.preset = None;
    }
    s.store_voice(guid,v)?;
    Ok(s)
}

/// Entrée Windows présente : identifiant et Equalizer APO branché dessus ou non.
pub struct Target<'a> {
    pub guid: &'a str,
    pub installed: bool,
}

/// Contenu de rustudio-micro-voix.txt : un bloc par micro actif, présent et branché, visé par son identifiant ;
/// sinon aucun filtre. ASCII seulement : Equalizer APO lit le fichier tel quel.
pub fn config_text(s: &Settings, targets: &[Target]) -> String {
    let mut lines = vec!["# Rustudio : voix du micro (module Micro). Fichier genere : regle-la dans Rustudio (page Micro).".to_string()];
    let mut blocks = 0;
    for (guid, v) in &s.mics {
        if !v.enabled || !targets.iter().any(|t| t.installed && t.guid.eq_ignore_ascii_case(guid)) {
            continue;
        }
        lines.push(format!("Device: {}", guid.to_uppercase()));
        lines.push(format!("Preamp: {:.1} dB", f64::from(v.gain_db.clamp(GAIN_RANGE.0, GAIN_RANGE.1))));
        for (i, (kind, hz, gain, q)) in v.filters().into_iter().enumerate() {
            if kind == "HPQ" {
                lines.push(format!("Filter {}: ON HPQ Fc {hz} Hz Q {q:.2}", i + 1));
            } else {
                lines.push(format!("Filter {}: ON {kind} Fc {hz} Hz Gain {gain:.1} dB Q {q:.2}", i + 1));
            }
        }
        blocks += 1;
    }
    if blocks > 0 {
        lines.push("Device: all".into());
    } else {
        lines.push("# Aucun traitement actif.".into());
    }
    let text = lines.join("\n") + "\n";
    text.chars().map(|c| if c.is_ascii() { c } else { '?' }).collect()
}

/// Ce que l'appli affiche : bandes, préréglages et bornes.
pub fn meta() -> Value {
    json!({
        "bands": BANDS.iter().map(|b| json!({"key": b.key, "hz": b.hz, "label": b.label, "hint": b.hint})).collect::<Vec<_>>(),
        "presets": PRESETS.iter().map(|p| json!({"id": p.id, "label": p.label, "desc": p.desc})).collect::<Vec<_>>(),
        "gain": GAIN_RANGE, "eq": EQ_RANGE, "hpf": HPF_RANGE,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIC: &str = "{cc214e79-d5e6-45c0-b617-dbb4992e5a7e}";
    const OTHER: &str = "{0a1b2c3d-0000-4000-8000-00000000beef}";

    #[test]
    fn voix_naturelle_par_defaut_et_aucun_filtre() {
        let s = update(&json!({"selected": MIC}), &Settings::default()).unwrap();
        assert_eq!(s.selected.as_deref(), Some(MIC));
        assert!(!s.voice(MIC).enabled, "aucun traitement sans choix explicite");
        let text = config_text(&s, &[Target { guid: MIC, installed: true }]);
        assert!(!text.contains("Device:") && !text.contains("Filter"), "{text}");
    }

    #[test]
    fn reglages_bornes_et_prereglage_perso_des_qu_on_touche() {
        let s = update(&json!({"selected": MIC.to_uppercase()}), &Settings::default()).unwrap();
        let s = update(&json!({"enabled": true, "preset": "radio"}), &s).unwrap();
        let v = s.voice(MIC);
        assert!(v.enabled);
        assert_eq!((v.hpf, v.hpf_hz, v.eq, v.preset.as_deref()), (true, 100, [4, -3, -1, 3, 2], Some("radio")));
        let s = update(&json!({"gain_db": 40, "eq": {"presence": -30}}), &s).unwrap();
        let v = s.voice(MIC);
        assert_eq!((v.gain_db, v.eq[3], v.preset.clone()), (12, -9, None));
        // même valeur : le préréglage reste reconnu
        let s = update(&json!({"preset": "clear"}), &s).unwrap();
        let s = update(&json!({"eq": [-1, -3, 0, 3, 1]}), &s).unwrap();
        assert_eq!(s.voice(MIC).preset.as_deref(), Some("clear"));
        assert!(update(&json!({"preset": "inconnu"}), &s).is_err());
        assert!(update(&json!({"selected": "Micro\nDevice: all"}), &s).is_err(), "rien d'autre qu'un identifiant");
        assert!(update(&json!({"gain_db": 3}), &Settings::default()).is_err(), "un micro doit être choisi");
    }

    #[test]
    fn fichier_vise_les_seuls_micros_actifs_et_branches() {
        let mut s = update(&json!({"selected": MIC}), &Settings::default()).unwrap();
        s = update(&json!({"enabled": true, "gain_db": 3, "preset": "warm"}), &s).unwrap();
        s = update(&json!({"device": OTHER, "enabled": true, "hpf": false, "eq": {"air": -4}}), &s).unwrap();
        let both = [Target { guid: MIC, installed: true }, Target { guid: OTHER, installed: false }];
        let t = config_text(&s, &both);
        assert!(t.contains("Device: {CC214E79-D5E6-45C0-B617-DBB4992E5A7E}\nPreamp: 3.0 dB\n"), "{t}");
        assert!(t.contains("Filter 1: ON HPQ Fc 80 Hz Q 0.71\n"), "{t}");
        assert!(t.contains("Filter 2: ON LSC Fc 120 Hz Gain 3.0 dB Q 0.71\n"), "{t}");
        assert!(t.contains("ON PK Fc 320 Hz Gain -1.0 dB Q 1.00"), "{t}");
        assert!(!t.contains("BEEF"), "micro non branché : rien n'est écrit pour lui");
        assert!(t.trim_end().ends_with("Device: all"));
        assert_eq!(t.lines().filter(|l| l.starts_with("Device:")).count(), 2);
        let t = config_text(&s, &[Target { guid: MIC, installed: true }, Target { guid: OTHER, installed: true }]);
        assert!(t.contains("Device: {0A1B2C3D-0000-4000-8000-00000000BEEF}\nPreamp: 0.0 dB\nFilter 1: ON HSC Fc 10000 Hz Gain -4.0 dB"), "{t}");
        assert!(t.is_ascii());
    }

    #[test]
    fn courbe_coupe_bas_et_bandes() {
        let mut v = Voice::default();
        v.apply_preset("radio").unwrap();
        let r = v.response_db(&[30.0, 120.0, 3500.0]);
        assert!(r[0] < -15.0, "le coupe-bas retire les grondements : {r:?}");
        assert!(r[2] > 2.5, "la présence ressort : {r:?}");
        v.hpf = false;
        v.eq = [0; 5];
        assert!(v.response_db(&[50.0, 1000.0]).iter().all(|d| d.abs() < 0.01));
    }

    #[test]
    fn reglages_relus_assainis_et_nombre_de_micros_borne() {
        let raw = json!({"selected": "pas un micro", "mics": {MIC: {"enabled": true, "gain_db": 99, "hpf_hz": 5, "preset": "zzz"}}});
        let s: Settings = serde_json::from_value::<Settings>(raw).unwrap().sanitized();
        assert_eq!(s.selected, None);
        let v = s.voice(MIC);
        assert_eq!((v.gain_db, v.hpf_hz, v.preset), (12, 40, None));
        let mut s = Settings::default();
        for i in 0..(MAX_MICS + 4) {
            let g = format!("{{00000000-0000-4000-8000-{i:012x}}}");
            s = update(&json!({"device": g, "gain_db": 1}), &s).unwrap();
        }
        assert!(s.mics.len() <= MAX_MICS);
    }
}

#[cfg(windows)]
pub mod mapping;
#[cfg(windows)]
pub mod registry;
#[cfg(windows)]
pub mod capture;
