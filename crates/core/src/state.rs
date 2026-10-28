//! Etat complet (ce qui est sauve dans state.json et envoye a l'interface) et sa normalisation tolerante.
use crate::consts::*;
use crate::dsp::{self, Dsp, Fx};
use crate::hpeq::{self, HpEq};
use crate::scenes::{self, Routing};
use crate::value::{clamp, first_chars, get, has, num, truthy};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mic {
    #[serde(rename = "type")]
    pub kind: String,
    pub gain_db: i32,
}

/// Voix mesuree par l'assistant « Régler ma voix » (niveaux d'entree en dB au gain indique).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MicCal {
    #[serde(rename = "type")]
    pub kind: String,
    pub gain_db: i32,
    pub t: f64,
    pub noise_db: f64,
    pub noise_peak_db: f64,
    pub voice_low_db: f64,
    pub voice_db: f64,
    pub voice_peak_db: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Lighting {
    pub theme: String,
    pub animation: String,
    pub mod1: i32,
    pub mod2: i32,
    pub waterfall: String,
    pub fader_style: IndexMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MicButton {
    pub mode: String,
    pub target: String,
    pub blink: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vod {
    pub mode: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub app: String,
    pub scene: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutoScenes {
    pub enabled: bool,
    pub rules: Vec<Rule>,
    pub fallback: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Prefs {
    pub hold_ms: i32,
    pub osd: bool,
    pub update_reminded: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub faders: IndexMap<String, String>,
    pub volumes: IndexMap<String, i32>,
    pub mutes: IndexMap<String, bool>,
    pub partial: IndexMap<String, Option<String>>,
    pub routing: Routing,
    pub colours: IndexMap<String, Vec<String>>,
    pub mic: Mic,
    pub dsp: Dsp,
    pub fx: Fx,
    pub mic_preset: String,
    pub mic_cal: Option<MicCal>,
    pub bleep_db: i32,
    pub lighting: Lighting,
    pub mute_modes: IndexMap<String, String>,
    pub mic_button: MicButton,
    pub vod: Vod,
    pub auto_scenes: AutoScenes,
    pub prefs: Prefs,
    pub scene: Option<String>,
    pub user_scenes: IndexMap<String, Value>,
    pub hp_eq: HpEq,
}

impl State {
    pub fn volume(&self, ch: &str) -> i32 {
        self.volumes.get(ch).copied().unwrap_or(0)
    }
    pub fn muted(&self, ch: &str) -> bool {
        self.mutes.get(ch).copied().unwrap_or(false)
    }
    pub fn partial_of(&self, ch: &str) -> Option<&str> {
        self.partial.get(ch).and_then(|p| p.as_deref())
    }
    pub fn fader_channel(&self, f: &str) -> &str {
        self.faders.get(f).map(|s| s.as_str()).unwrap_or("Mic")
    }
    pub fn route(&self, inp: &str, out: &str) -> bool {
        self.routing.get(inp).and_then(|m| m.get(out)).copied().unwrap_or(false)
    }
    pub fn set_route(&mut self, inp: &str, out: &str, on: bool) {
        if let Some(m) = self.routing.get_mut(inp) {
            m.insert(out.to_string(), on);
        }
    }
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
    /// mesure de la voix pour un type de micro (le micro actuel par defaut)
    pub fn cal_for(&self, kind: Option<&str>) -> Option<&MicCal> {
        let k = kind.unwrap_or(&self.mic.kind);
        self.mic_cal.as_ref().filter(|c| c.kind == k)
    }
}

pub fn default_state() -> State {
    State {
        faders: [("A", "Mic"), ("B", "Music"), ("C", "Chat"), ("D", "System")]
            .iter().map(|(f, c)| (f.to_string(), c.to_string())).collect(),
        volumes: CHANNELS.iter().map(|c| (c.to_string(), 128)).collect(),
        mutes: CHANNELS.iter().map(|c| (c.to_string(), false)).collect(),
        partial: CHANNELS.iter().map(|c| (c.to_string(), None)).collect(),
        routing: scenes::routing(&[]),
        colours: COLOUR_TARGETS.iter().map(|(t, _, _)| (t.to_string(), vec!["00FFFF".into(), "FFFFFF".into()])).collect(),
        mic: Mic { kind: "dynamic".into(), gain_db: 30 },
        dsp: dsp::preset_dsp("naturel"),
        fx: Fx::default(),
        mic_preset: "brut".into(),
        mic_cal: None,
        bleep_db: -10,
        lighting: Lighting {
            theme: "voies".into(),
            animation: "none".into(),
            mod1: 39,
            mod2: 39,
            waterfall: "off".into(),
            fader_style: FADERS.iter().map(|f| (f.to_string(), "gradient".into())).collect(),
        },
        mute_modes: FADERS.iter().map(|f| (f.to_string(), "all".into())).collect(),
        mic_button: MicButton { mode: "toggle".into(), target: "all".into(), blink: false },
        vod: Vod { mode: "off".into() },
        auto_scenes: AutoScenes { enabled: false, rules: vec![], fallback: None },
        prefs: Prefs { hold_ms: 500, osd: true, update_reminded: None },
        scene: None,
        user_scenes: IndexMap::new(),
        hp_eq: hpeq::normalize_hp_eq(&Value::Null, None),
    }
}

/// Couleur RRGGBB valide (majuscules), sinon None.
pub fn hex(c: &str) -> Option<String> {
    let c = c.trim().trim_start_matches('#').to_uppercase();
    (c.len() == 6 && c.chars().all(|ch| ch.is_ascii_hexdigit())).then_some(c)
}

pub fn normalize_cal(src: &Value) -> Option<MicCal> {
    let kind = get(src, "type").as_str()?;
    if !is_mic_type(kind) {
        return None;
    }
    let gain_db = clamp(get(src, "gain_db"), 0, 72)?;
    let t = match get(src, "t") {
        Value::Null => 0.0,
        v => num(v).unwrap_or(0.0),
    };
    let lvl = |k: &str| -> Option<f64> {
        let f = num(get(src, k))?;
        Some((f.clamp(-80.0, 0.0) * 10.0).round() / 10.0)
    };
    Some(MicCal {
        kind: kind.into(),
        gain_db,
        t,
        noise_db: lvl("noise_db")?,
        noise_peak_db: lvl("noise_peak_db")?,
        voice_low_db: lvl("voice_low_db")?,
        voice_db: lvl("voice_db")?,
        voice_peak_db: lvl("voice_peak_db")?,
    })
}

pub fn norm_app(name: &str) -> String {
    let n = name.trim().to_lowercase();
    n.strip_suffix(".exe").map(|s| s.to_string()).unwrap_or(n)
}

pub fn normalize_rules(rules: &Value, scene_names: &HashSet<String>) -> Vec<Rule> {
    let mut out: Vec<Rule> = vec![];
    for r in rules.as_array().into_iter().flatten() {
        if !r.is_object() {
            continue;
        }
        let app = first_chars(get(r, "app").as_str().unwrap_or("").trim(), 64);
        let scene = get(r, "scene").as_str().unwrap_or("");
        if !app.is_empty() && scene_names.contains(scene) && !out.iter().any(|x| norm_app(&x.app) == norm_app(&app)) {
            out.push(Rule { app, scene: scene.into() });
        }
    }
    out.truncate(20);
    out
}

pub fn scene_names(st: &State) -> HashSet<String> {
    scenes::SCENES.iter().map(|s| s.key.to_string()).chain(st.user_scenes.keys().cloned()).collect()
}

/// Reglages des boutons, de la VOD, des scenes automatiques et des preferences. Modifie st sur place.
pub fn normalize_settings(st: &mut State, src: &Value) {
    if let Some(mm) = get(src, "mute_modes").as_object() {
        for (f, mode) in mm {
            let f = f.to_uppercase();
            if let Some(m) = mode.as_str() {
                if is_fader(&f) && mute_target(m).is_some() {
                    st.mute_modes.insert(f, m.into());
                }
            }
        }
    }
    let mb = get(src, "mic_button");
    if let Some(m) = get(mb, "mode").as_str() {
        if MIC_BUTTON_MODES.contains(&m) {
            st.mic_button.mode = m.into();
        }
    }
    if let Some(t) = get(mb, "target").as_str() {
        if mute_target(t).is_some() {
            st.mic_button.target = t.into();
        }
    }
    if has(mb, "blink") {
        st.mic_button.blink = truthy(get(mb, "blink"));
    }
    if let Some(m) = get(get(src, "vod"), "mode").as_str() {
        if VOD_MODES.contains(&m) {
            st.vod.mode = m.into();
        }
    }
    let pr = get(src, "prefs");
    if has(pr, "hold_ms") {
        if let Some(v) = clamp(get(pr, "hold_ms"), 250, 2000) {
            st.prefs.hold_ms = v;
        }
    }
    if has(pr, "osd") {
        st.prefs.osd = truthy(get(pr, "osd"));
    }
    if has(pr, "update_reminded") {
        let v = get(pr, "update_reminded");
        st.prefs.update_reminded = v.as_str().and_then(crate::semver::parse).map(|_| v.clone());
    }
    let au = get(src, "auto_scenes");
    let names = scene_names(st);
    if has(au, "enabled") {
        st.auto_scenes.enabled = truthy(get(au, "enabled"));
    }
    if has(au, "rules") {
        st.auto_scenes.rules = normalize_rules(get(au, "rules"), &names);
    }
    if has(au, "fallback") {
        st.auto_scenes.fallback = get(au, "fallback").as_str().filter(|f| names.contains(*f)).map(String::from);
    }
}

/// Etat tolerant : tout ce qui est inconnu ou invalide garde la valeur par defaut.
pub fn normalize_state(src: &Value) -> State {
    let mut st = default_state();
    let empty = serde_json::Map::new();
    let obj = src.as_object().unwrap_or(&empty);
    let srcv = Value::Object(obj.clone());
    let src = &srcv;
    if let Some(m) = get(src, "faders").as_object() {
        for (f, ch) in m {
            let f = f.to_uppercase();
            if let Some(c) = ch.as_str() {
                if is_fader(&f) && channel_index(c).is_some() {
                    st.faders.insert(f, c.into());
                }
            }
        }
    }
    if let Some(m) = get(src, "volumes").as_object() {
        for (ch, v) in m {
            if channel_index(ch).is_some() {
                if let Some(x) = clamp(v, 0, 255) {
                    st.volumes.insert(ch.clone(), x);
                }
            }
        }
    }
    if let Some(m) = get(src, "mutes").as_object() {
        for (ch, v) in m {
            if channel_index(ch).is_some() {
                st.mutes.insert(ch.clone(), truthy(v));
            }
        }
    }
    if let Some(m) = get(src, "partial").as_object() {
        for (ch, t) in m {
            if let Some(t) = t.as_str() {
                if channel_input(ch).is_some() && mute_target(t).is_some() && t != "all" && !st.muted(ch) {
                    st.partial.insert(ch.clone(), Some(t.into()));
                }
            }
        }
    }
    if let Some(m) = get(src, "routing").as_object() {
        for (inp, outs) in m {
            if !is_input(inp) {
                continue;
            }
            if let Some(o) = outs.as_object() {
                for (out, on) in o {
                    if is_output(out) {
                        st.set_route(inp, out, truthy(on));
                    }
                }
            }
        }
    }
    if let Some(m) = get(src, "colours").as_object() {
        for (t, cols) in m {
            let t = colour_alias(t);
            if colour_target(t).is_none() {
                continue;
            }
            let list: Vec<String> = match cols {
                Value::String(s) => vec![s.clone()],
                Value::Array(a) => a.iter().filter_map(|c| c.as_str().map(String::from)).collect(),
                _ => vec![],
            };
            let list: Vec<String> = list.iter().filter(|c| !c.is_empty()).filter_map(|c| hex(c)).collect();
            if !list.is_empty() {
                let mut two = list.clone();
                two.extend(list);
                two.truncate(2);
                st.colours.insert(t.to_string(), two);
            }
        }
    }
    let mic = get(src, "mic");
    if let Some(t) = get(mic, "type").as_str() {
        if is_mic_type(t) {
            st.mic.kind = t.into();
        }
    }
    if !get(mic, "gain_db").is_null() {
        if let Some(g) = clamp(get(mic, "gain_db"), 0, 72) {
            st.mic.gain_db = g;
        }
    }
    if truthy(get(src, "dsp")) {
        st.dsp = dsp::normalize_dsp(get(src, "dsp"), None);
    }
    if let Some(p) = get(src, "mic_preset").as_str() {
        if dsp::is_preset(p) || p == "custom" {
            st.mic_preset = p.into();
        }
    }
    st.mic_cal = normalize_cal(get(src, "mic_cal"));
    if has(src, "fx") {
        st.fx = dsp::normalize_fx(get(src, "fx"));
    } else if !obj.is_empty() {
        // etat d'avant 4.8 : traitements et retour micro s'activaient tout seuls. On repart de la voix naturelle,
        // sans retour casque ; les reglages sont recales en douceur pour le jour ou tu les actives.
        st.fx = Fx::default();
        st.set_route("Microphone", "Headphones", false);
        st.mic_preset = "brut".into();
        let cal = st.mic_cal.clone().filter(|c| c.kind == st.mic.kind);
        st.dsp = dsp::fit_dsp("naturel", cal.as_ref(), Some(st.mic.gain_db));
    }
    st.hp_eq = hpeq::normalize_hp_eq(get(src, "hp_eq"), None);
    if has(src, "bleep_db") {
        if let Some(b) = clamp(get(src, "bleep_db"), -36, 0) {
            st.bleep_db = b;
        }
    }
    let li = get(src, "lighting");
    if let Some(t) = get(li, "theme").as_str() {
        if theme(t).is_some() || t == "custom" {
            st.lighting.theme = t.into();
        }
    }
    if let Some(a) = get(li, "animation").as_str() {
        if animation(a).is_some() {
            st.lighting.animation = a.into();
        }
    }
    for k in ["mod1", "mod2"] {
        if has(li, k) {
            if let Some(v) = clamp(get(li, k), 0, 100) {
                if k == "mod1" { st.lighting.mod1 = v } else { st.lighting.mod2 = v }
            }
        }
    }
    if let Some(w) = get(li, "waterfall").as_str() {
        if waterfall(w).is_some() {
            st.lighting.waterfall = w.into();
        }
    }
    if let Some(m) = get(li, "fader_style").as_object() {
        for (f, style) in m {
            if let Some(s) = style.as_str() {
                if is_fader(f) && fader_style(s).is_some() {
                    st.lighting.fader_style.insert(f.clone(), s.into());
                }
            }
        }
    }
    if let Some(m) = get(src, "user_scenes").as_object() {
        for (name, sc) in m {
            let n = name.trim();
            if sc.is_object() && !n.is_empty() {
                st.user_scenes.insert(first_chars(n, 40), sc.clone());
            }
        }
    }
    if let Some(sc) = get(src, "scene").as_str() {
        if scenes::scene(sc).is_some() || st.user_scenes.contains_key(sc) {
            st.scene = Some(sc.into());
        }
    }
    normalize_settings(&mut st, src);
    st
}

/// Reglages enregistres dans une scene perso.
pub const USER_SCENE_KEYS: [&str; 11] =
    ["faders", "routing", "volumes", "mic", "dsp", "fx", "mic_preset", "lighting", "colours", "mute_modes", "vod"];

pub fn user_scene_of(st: &State) -> Value {
    let all = st.to_value();
    let mut out = serde_json::Map::new();
    for k in USER_SCENE_KEYS {
        out.insert(k.into(), all.get(k).cloned().unwrap_or(Value::Null));
    }
    Value::Object(out)
}

/// Etat apres application d'une scene perso (ce qui n'est pas dans la scene reste).
pub fn apply_user_scene(st: &State, scene: &Value) -> State {
    let mut base = st.to_value();
    if let (Some(b), Some(s)) = (base.as_object_mut(), scene.as_object()) {
        for (k, v) in s {
            b.insert(k.clone(), v.clone());
        }
        if !s.contains_key("fx") {
            b.insert("fx".into(), json!(st.fx));
        }
    }
    let mut merged = normalize_state(&base);
    merged.user_scenes = st.user_scenes.clone();
    merged.mutes = st.mutes.clone();
    merged.partial = st.partial.clone();
    merged.auto_scenes = st.auto_scenes.clone();
    merged.prefs = st.prefs.clone();
    merged.mic_button = st.mic_button.clone();
    merged
}
