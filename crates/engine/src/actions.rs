//! GoXLR Mini : actions de l'API appliquées à l'état sous le verrou, sans jamais toucher l'USB.
use crate::api::{need, need_str, ApiError, ApiResult};
use crate::controller::{ch, inp, AutoActive, Inner, Key};
use rustudio_core::consts::*;
use rustudio_core::dsp::{self, Fx};
use rustudio_core::state;
use rustudio_core::value::{clamp, clamp_f, get, has, num, parse_bool, parse_volume};
use rustudio_core::{hpeq, lighting, scenes};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

impl Inner {
    pub fn apply_scene(&mut self, name: &str) -> Result<(), ApiError> {
        if let Some(sc) = scenes::scene(name) {
            self.state.faders = sc.faders_map();
            let monitor = self.state.route("Microphone", "Headphones");
            self.state.routing = sc.routing();
            // ta voix et ton retour casque sont a toi : une scene integree ne change que faders et routage
            self.state.set_route("Microphone", "Headphones", monitor);
            if self.state.lighting.theme == "voies" {
                lighting::apply_theme(&mut self.state);
            }
        } else if let Some(sc) = self.state.user_scenes.get(name).cloned() {
            self.state = state::apply_user_scene(&self.state, &sc);
        } else {
            return Err(ApiError::bad(format!("scène inconnue: {name}")));
        }
        self.state.scene = Some(name.into());
        self.prev_pos = None;
        self.mark(&[Key::All], true);
        Ok(())
    }

    fn settings(&mut self, p: &Value) {
        let before_mb = self.state.mic_button.clone();
        let before_vod = self.state.vod.mode.clone();
        let mut sub = serde_json::Map::new();
        for k in ["mute_modes", "mic_button", "vod", "prefs"] {
            if let Some(v) = p.get(k) {
                sub.insert(k.into(), v.clone());
            }
        }
        state::normalize_settings(&mut self.state, &Value::Object(sub));
        let mb = self.state.mic_button.clone();
        if mb.mode == "ptt" && before_mb.mode != "ptt" {
            self.mute_to("Mic", &mb.target); // appuyer pour parler : micro coupe au repos
            self.mute_changed("Mic");
        } else if before_mb.mode == "ptt" && mb.mode != "ptt" && self.is_muted("Mic") {
            self.unmute("Mic");
            self.mute_changed("Mic");
        } else if mb.target != before_mb.target && self.state.partial_of("Mic").is_some() {
            self.mute_to("Mic", &mb.target);
            self.mute_changed("Mic");
        }
        if mb.blink != before_mb.blink {
            self.mark(&[Key::Buttons], true);
        }
        if self.state.vod.mode != before_vod {
            let keys: Vec<Key> = (0..INPUTS.len()).map(Key::Route).collect();
            self.mark(&keys, true);
        }
    }

    /// Actions de l'API (sous le verrou) : ne touchent jamais l'USB.
    pub fn act_locked(&mut self, action: &str, p: &Value) -> ApiResult {
        match action {
            "volume" => {
                let c = channel_named(&need_str(p, "channel")?).ok_or_else(|| ApiError::bad("canal inconnu"))?;
                let v = parse_volume(need(p, "value")?).map_err(ApiError::bad)?;
                self.state.volumes.insert(c.into(), v);
                self.mark(&[Key::Volume(ch(c))], true);
            }
            "mute" => {
                let c = channel_named(&need_str(p, "channel")?).ok_or_else(|| ApiError::bad("canal inconnu"))?;
                if parse_bool(need(p, "muted")?) {
                    let t = p.get("target").and_then(|t| t.as_str()).filter(|t| !t.is_empty()).unwrap_or("all").to_string();
                    self.mute_to(c, &t);
                } else {
                    self.unmute(c);
                }
                self.mute_changed(c);
            }
            "fader" => {
                let f = need_str(p, "fader")?.to_uppercase();
                let c = channel_named(&need_str(p, "channel")?).ok_or_else(|| ApiError::bad("canal inconnu"))?;
                if !is_fader(&f) {
                    return Err(ApiError::bad(format!("fader inconnu: {f}")));
                }
                let old = self.state.fader_channel(&f).to_string();
                let swapped: Vec<String> =
                    self.state.faders.iter().filter(|(k, v)| v.as_str() == c && **k != f).map(|(k, _)| k.clone()).collect();
                for k in swapped {
                    self.state.faders.insert(k, old.clone()); // une voie par fader : on echange
                }
                self.state.faders.insert(f, c.into());
                self.state.scene = None;
                self.prev_pos = None;
                if self.state.lighting.theme == "voies" {
                    lighting::apply_theme(&mut self.state);
                }
                self.mark(&[Key::Faders, Key::Colours, Key::Buttons], true);
            }
            "route" => {
                let (i, o) = (need_str(p, "input")?, need_str(p, "output")?);
                if !is_input(&i) || !is_output(&o) {
                    return Err(ApiError::bad("entrée ou sortie inconnue"));
                }
                self.state.set_route(&i, &o, parse_bool(need(p, "enabled")?));
                self.state.scene = None;
                self.mark(&[Key::Route(inp(&i))], true);
            }
            "colour" => {
                let raw = need_str(p, "target")?;
                let t = colour_alias(&raw).to_string();
                if t == "all" {
                    let c1 = p.get("colour_one").and_then(|v| v.as_str()).map(String::from)
                        .or_else(|| get(p, "colours").get(0).and_then(|v| v.as_str()).map(String::from))
                        .unwrap_or_default();
                    let c = state::hex(&c1).ok_or_else(|| ApiError::bad(format!("couleur invalide: {c1}")))?;
                    for (k, _, _) in COLOUR_TARGETS {
                        let v = if k.starts_with("FadeMeter") { vec![lighting::darken(&c, 0.16), c.clone()] } else { vec![c.clone(), c.clone()] };
                        self.state.colours.insert(k.into(), v);
                    }
                } else if colour_target(&t).is_some() {
                    let cols: Vec<Value> = match p.get("colours").and_then(|v| v.as_array()) {
                        Some(a) if !a.is_empty() => a.clone(),
                        _ => vec![get(p, "colour_one").clone(), get(p, "colour_two").clone()],
                    };
                    for (i, c) in cols.iter().take(2).enumerate() {
                        if let Some(cs) = c.as_str().filter(|s| !s.is_empty()) {
                            let h = state::hex(cs).ok_or_else(|| ApiError::bad(format!("couleur invalide: {cs}")))?;
                            let e = self.state.colours.entry(t.clone()).or_insert_with(|| vec!["000000".into(); 2]);
                            e[i] = h;
                        }
                    }
                } else {
                    return Err(ApiError::bad(format!("cible couleur inconnue: {t}")));
                }
                self.state.lighting.theme = "custom".into();
                self.mark(&[Key::Colours], true);
            }
            "theme" => {
                let name = need_str(p, "name")?;
                if theme(&name).is_none() {
                    return Err(ApiError::bad(format!("thème inconnu: {name}")));
                }
                self.state.lighting.theme = name;
                lighting::apply_theme(&mut self.state);
                self.mark(&[Key::Colours], true);
            }
            "lighting" => {
                let li = &mut self.state.lighting;
                if let Some(a) = p.get("animation").and_then(|v| v.as_str()).filter(|a| animation(a).is_some()) {
                    li.animation = a.into();
                }
                for k in ["mod1", "mod2"] {
                    if let Some(v) = p.get(k).and_then(|v| clamp(v, 0, 100)) {
                        if k == "mod1" { li.mod1 = v } else { li.mod2 = v }
                    }
                }
                if let Some(w) = p.get("waterfall").and_then(|v| v.as_str()).filter(|w| waterfall(w).is_some()) {
                    li.waterfall = w.into();
                }
                match p.get("fader_style") {
                    Some(Value::String(s)) if fader_style(s).is_some() => {
                        for f in FADERS {
                            li.fader_style.insert(f.into(), s.clone());
                        }
                    }
                    Some(Value::Object(m)) => {
                        for (f, st) in m {
                            if let Some(s) = st.as_str() {
                                if is_fader(f) && fader_style(s).is_some() {
                                    li.fader_style.insert(f.clone(), s.into());
                                }
                            }
                        }
                    }
                    _ => {}
                }
                self.mark(&[Key::Lighting], true);
            }
            "mic" => {
                let mtype = p.get("type").and_then(|v| v.as_str()).map(String::from).unwrap_or_else(|| self.state.mic.kind.clone());
                if !is_mic_type(&mtype) {
                    return Err(ApiError::bad(format!("type micro inconnu: {mtype}")));
                }
                let mut gain = p.get("gain_db").filter(|v| !v.is_null()).and_then(num);
                if gain.is_none() && mtype != self.state.mic.kind && parse_bool(get(p, "suggest_gain")) {
                    gain = Some(recommended_gain(&mtype) as f64);
                }
                let before = self.state.mic.clone();
                self.state.mic = state::Mic { kind: mtype, gain_db: clamp_f(gain.unwrap_or(self.state.mic.gain_db as f64), 0, 72) };
                self.mark(&[Key::Mic], true);
                if self.state.mic != before && dsp::is_preset(&self.state.mic_preset) && self.cal_for().is_some() {
                    self.state.dsp = self.refit(); // seuils toujours places par rapport a ta voix
                    self.mark(&[Key::Dsp], true);
                }
            }
            "dsp" => {
                self.state.dsp = dsp::normalize_dsp(p, Some(&self.state.dsp));
                if has(get(p, "gate"), "enabled") {
                    self.state.fx.gate = rustudio_core::value::truthy(get(get(p, "gate"), "enabled"));
                }
                if has(p, "bleep_db") {
                    if let Some(b) = clamp(get(p, "bleep_db"), -36, 0) {
                        self.state.bleep_db = b;
                    }
                }
                self.state.mic_preset = "custom".into();
                self.mark(&[Key::Dsp], true);
            }
            "mic-preset" => {
                let name = need_str(p, "name")?;
                if !dsp::is_preset(&name) {
                    return Err(ApiError::bad(format!("préréglage micro inconnu: {name}")));
                }
                // « Naturel » coupe tout mais garde des reglages doux prets, pour allumer ensuite un seul traitement
                self.state.dsp = self.preset_dsp(if name == "brut" { "naturel" } else { &name });
                self.state.fx = if name == "brut" { Fx::default() } else { dsp::fx_from_dsp(&self.state.dsp) };
                self.state.mic_preset = name;
                self.mark(&[Key::Dsp], true);
            }
            "fx" => self.set_fx(p),
            "scene" => self.apply_scene(&need_str(p, "name")?)?,
            "scene-save" => {
                let name: String = need_str(p, "name")?.trim().chars().take(40).collect();
                if name.is_empty() {
                    return Err(ApiError::bad("nom de scène vide"));
                }
                let sc = state::user_scene_of(&self.state);
                self.state.user_scenes.insert(name.clone(), sc);
                self.state.scene = Some(name);
            }
            "scene-delete" => {
                let name = need_str(p, "name")?;
                self.state.user_scenes.shift_remove(&name);
                if self.state.scene.as_deref() == Some(&name) {
                    self.state.scene = None;
                }
                let au = &mut self.state.auto_scenes;
                au.rules.retain(|r| r.scene != name);
                if au.fallback.as_deref() == Some(&name) {
                    au.fallback = None;
                }
            }
            "settings" => self.settings(p),
            "auto-scenes" => {
                state::normalize_settings(&mut self.state, &json!({ "auto_scenes": p }));
                self.auto_active = AutoActive::Unknown; // re-evaluation au prochain passage
                self.auto_seen = true;
            }
            "sound-test" => {
                let secs = num(get(p, "seconds")).unwrap_or(40.0).clamp(0.0, 150.0);
                self.sound_test_until = (secs > 0.0).then(|| Instant::now() + Duration::from_secs_f64(secs));
                self.mark(&[Key::Route(inp("Samples"))], false);
                return Ok(json!({ "ok": true, "seconds": secs }));
            }
            "mic-listen" => {
                let secs = num(get(p, "seconds")).unwrap_or(15.0).clamp(0.0, 30.0);
                self.listen_level = p.get("level").and_then(|v| clamp(v, 0, 255)).unwrap_or(191);
                self.listen_until = (secs > 0.0).then(|| Instant::now() + Duration::from_secs_f64(secs));
                self.mark(&[Key::Volume(ch("MicMonitor")), Key::Route(inp("Microphone"))], false);
                return Ok(json!({ "ok": true, "seconds": secs }));
            }
            "hp-eq" => {
                let eq = self.state.hp_eq.clone();
                let mut upd = p.as_object().cloned().unwrap_or_default();
                let style = p.get("style").and_then(|v| v.as_str()).map(String::from);
                if let Some(st) = style.as_deref().and_then(hpeq::style) {
                    upd.insert("bands".into(), json!(st.bands));
                } else if let Some(name) = style.as_deref().and_then(|s| s.strip_prefix("custom:")) {
                    if let Some(b) = eq.custom.get(name) {
                        upd.insert("bands".into(), json!(b));
                    }
                } else if has(p, "bands") && !has(p, "style") {
                    upd.insert("style".into(), Value::Null); // bande touchee a la main : reglage perso
                }
                self.state.hp_eq = hpeq::normalize_hp_eq(&Value::Object(upd), Some(&eq));
            }
            "hp-eq-save" => {
                let name: String = p.get("name").and_then(|v| v.as_str()).unwrap_or("").trim().chars().take(40).collect();
                if name.is_empty() {
                    return Err(ApiError::bad("nom de profil vide"));
                }
                let eq = &mut self.state.hp_eq;
                eq.custom.insert(name.clone(), eq.bands.clone());
                eq.style = Some(format!("custom:{name}"));
            }
            "hp-eq-delete" => {
                let name = p.get("name").map(rustudio_core::value::py_str).unwrap_or_default();
                let eq = &mut self.state.hp_eq;
                eq.custom.shift_remove(&name);
                if eq.style.as_deref() == Some(format!("custom:{name}").as_str()) {
                    eq.style = None;
                }
            }
            "job-cancel" => {
                if let Some(j) = self.job.as_mut() {
                    j.insert("cancel".into(), json!(true));
                }
            }
            "profile" => {
                let keep = self.state.user_scenes.clone();
                let src = p.get("profile").filter(|v| v.is_object()).unwrap_or(p);
                self.state = state::normalize_state(src);
                if self.state.user_scenes.is_empty() {
                    self.state.user_scenes = keep;
                }
                if theme(&self.state.lighting.theme).is_some() {
                    lighting::apply_theme(&mut self.state);
                }
                self.blink.clear();
                self.prev_pos = None;
                self.mark(&[Key::All], true);
            }
            other => return Err(ApiError { status: 404, msg: format!("action inconnue: {other}") }),
        }
        Ok(json!({ "ok": true, "queued": self.connected }))
    }
}
