//! Analyses du micro : detection (48 V compris), gain automatique, assistant « Régler ma voix ».
use crate::config::now_epoch;
use crate::api::ApiError;
use crate::controller::{Ctl, Key};
use crate::logf;
use rustudio_core::consts::*;
use rustudio_core::dsp::{self, mic_type_preset};
use rustudio_core::state::{self, MicCal};
use rustudio_core::value::{clamp_f, get, num, parse_bool, truthy};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub enum JobErr {
    Cancelled,
    Msg(String),
}
impl From<String> for JobErr {
    fn from(s: String) -> JobErr {
        JobErr::Msg(s)
    }
}
type JR<T> = Result<T, JobErr>;

fn pct(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return -72.2;
    }
    let i = ((sorted.len() as f64 * q) as usize).min(sorted.len() - 1);
    sorted[i]
}
fn sorted(mut v: Vec<f64>) -> Vec<f64> {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    v
}

pub fn start_job(ctl: &Arc<Ctl>, kind: &str, p: Value) -> Result<f64, ApiError> {
    let started = now_epoch();
    {
        let mut g = ctl.lock();
        if g.job.as_ref().map(|j| truthy(j.get("running").unwrap_or(&Value::Null))).unwrap_or(false) {
            return Err(ApiError::bad("Une analyse est déjà en cours."));
        }
        if !g.connected {
            return Err(ApiError::device("GoXLR non connectée"));
        }
        let j = json!({"kind": kind, "running": true, "progress": 0.0, "step": "", "result": null, "error": null, "started": started});
        g.job = j.as_object().cloned();
    }
    let (c, k) = (ctl.clone(), kind.to_string());
    std::thread::Builder::new().name("job".into()).spawn(move || run_job(&c, &k, &p)).ok();
    Ok(started)
}

fn update(ctl: &Ctl, kv: &[(&str, Value)]) {
    let mut g = ctl.lock();
    if let Some(j) = g.job.as_mut() {
        for (k, v) in kv {
            j.insert(k.to_string(), v.clone());
        }
    }
}
fn step(ctl: &Ctl, s: &str) {
    update(ctl, &[("step", json!(s))]);
}

fn cancelled(ctl: &Ctl) -> bool {
    ctl.lock().job.as_ref().map(|j| truthy(j.get("cancel").unwrap_or(&Value::Null))).unwrap_or(false)
}

fn wait_applied(ctl: &Ctl, gen: u64, timeout: Duration) -> JR<()> {
    let t0 = Instant::now();
    while t0.elapsed() < timeout {
        {
            let g = ctl.lock();
            if g.done >= gen {
                return Ok(());
            }
            if !g.connected {
                return Err("GoXLR déconnectée pendant l’analyse".to_string().into());
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Err("La GoXLR ne répond pas".to_string().into())
}

fn set_gain_and_wait(ctl: &Ctl, mtype: &str, gain: i32) -> JR<()> {
    let gen = {
        let mut g = ctl.lock();
        g.gain_override = Some((mtype.into(), gain));
        g.mark(&[Key::Mic], false)
    };
    wait_applied(ctl, gen, Duration::from_secs(4))
}

fn sample(ctl: &Ctl, seconds: f64, p0: f64, p1: f64) -> JR<Vec<f64>> {
    let mut vals = vec![];
    let t0 = Instant::now();
    while t0.elapsed().as_secs_f64() < seconds {
        if cancelled(ctl) {
            return Err(JobErr::Cancelled);
        }
        let v = {
            let g = ctl.lock();
            if !g.connected {
                return Err("GoXLR déconnectée pendant l’analyse".to_string().into());
            }
            g.live.mic_db
        };
        if let Some(v) = v {
            vals.push(v);
        }
        let prog = p0 + (p1 - p0) * t0.elapsed().as_secs_f64() / seconds;
        update(ctl, &[("progress", json!((prog * 1000.0).round() / 1000.0))]);
        std::thread::sleep(Duration::from_millis(40));
    }
    Ok(vals)
}

/// Comme sample, mais s'arrete tot si l'entree sature : le gain est alors trop fort.
fn sample_voice(ctl: &Ctl, seconds: f64, p0: f64, p1: f64) -> JR<(Vec<f64>, bool)> {
    let (mut vals, t0, mut clips, mut seen) = (vec![], Instant::now(), 0, None);
    while t0.elapsed().as_secs_f64() < seconds {
        if cancelled(ctl) {
            return Err(JobErr::Cancelled);
        }
        let (v, t) = {
            let g = ctl.lock();
            if !g.connected {
                return Err("GoXLR déconnectée pendant le réglage".to_string().into());
            }
            (g.live.mic_db, g.live.level_t)
        };
        if let Some(v) = v {
            vals.push(v);
            if t != seen && v >= CLIP_DB {
                clips += 1;
                if clips >= 3 && t0.elapsed().as_secs_f64() < 5.0 {
                    return Ok((vals, true));
                }
            }
            seen = t;
        }
        let prog = p0 + (p1 - p0) * t0.elapsed().as_secs_f64() / seconds;
        update(ctl, &[("progress", json!((prog * 1000.0).round() / 1000.0))]);
        std::thread::sleep(Duration::from_millis(40));
    }
    Ok((vals, false))
}

fn run_job(ctl: &Arc<Ctl>, kind: &str, p: &Value) {
    ctl.lock().fast_level = true;
    let r = match kind {
        "detect-mic" => detect_mic(ctl, parse_bool(get(p, "phantom"))),
        "voice-setup" => voice_setup(ctl, p),
        _ => auto_gain(ctl, num(get(p, "target_db")).unwrap_or(VOICE_TARGET_DB), num(get(p, "seconds")).unwrap_or(6.0)),
    };
    match r {
        Ok(res) => update(ctl, &[("running", json!(false)), ("progress", json!(1.0)), ("result", res), ("step", json!("Terminé"))]),
        Err(JobErr::Cancelled) => update(ctl, &[("running", json!(false)), ("error", json!("Analyse annulée."))]),
        Err(JobErr::Msg(m)) => {
            logf!("Analyse {kind}: {m}");
            update(ctl, &[("running", json!(false)), ("error", json!(m))]);
        }
    }
    let mut g = ctl.lock();
    g.fast_level = false;
    g.gain_override = None;
    g.mark(&[Key::Mic], true);
}

fn detect_mic(ctl: &Ctl, phantom: bool) -> JR<Value> {
    let mut tests = vec![("dynamic", 40, "Entrée XLR"), ("jack", 30, "Entrée jack 3,5 mm")];
    if phantom {
        tests.push(("condenser", 30, "Entrée XLR avec alimentation 48 V"));
    }
    let n = tests.len() as f64;
    let mut levels = serde_json::Map::new();
    let mut best: Option<(&str, f64)> = None;
    for (i, (mtype, gain, label)) in tests.iter().enumerate() {
        let i = i as f64;
        step(ctl, &format!("Écoute : {label}"));
        set_gain_and_wait(ctl, mtype, *gain)?;
        sample(ctl, 0.6, i / n, (i + 0.3) / n)?;
        let vals = sorted(sample(ctl, 1.6, (i + 0.3) / n, (i + 1.0) / n)?);
        let floor = if vals.is_empty() { -72.2 } else { vals[vals.len() / 2] };
        let peak = vals.last().copied().unwrap_or(-72.2);
        levels.insert(mtype.to_string(), json!({"floor_db": floor, "peak_db": peak, "gain_db": gain, "label": label}));
        if peak > -70.5 && best.map(|(_, f)| floor > f).unwrap_or(true) {
            best = Some((mtype, floor));
        }
    }
    let Some((best, _)) = best else {
        return Ok(json!({"detected": null, "levels": levels,
            "message": format!("Aucun micro détecté. Branche-le (XLR ou jack) puis relance l’analyse.{}",
                if phantom { "" } else { " Micro statique : relance avec l’alimentation 48 V." })}));
    };
    let msg = match best {
        "dynamic" => "Micro dynamique XLR détecté.",
        "jack" => "Micro jack (casque) détecté.",
        _ => "Micro statique détecté (alimentation 48 V).",
    };
    Ok(json!({"detected": best, "levels": levels, "recommended_preset": mic_type_preset(best),
              "recommended_gain": recommended_gain(best), "message": msg}))
}

fn auto_gain(ctl: &Ctl, target_db: f64, seconds: f64) -> JR<Value> {
    let old = ctl.lock().state.mic.gain_db;
    step(ctl, "Parle normalement, comme en live");
    let vals = sorted(sample(ctl, seconds, 0.0, 0.92)?.into_iter().filter(|v| *v > -60.0).collect());
    if vals.len() < 15 {
        return Err("Je n’ai pas entendu ta voix. Rapproche-toi du micro et recommence.".to_string().into());
    }
    let peak = vals[((vals.len() as f64 * 0.95) as usize).saturating_sub(1).min(vals.len() - 1)];
    let new = clamp_f(old as f64 + (target_db - peak), 0, 72);
    step(ctl, "Application du nouveau gain");
    let gen = {
        let mut g = ctl.lock();
        g.state.mic.gain_db = new;
        let mut gen = g.mark(&[Key::Mic], true);
        if dsp::is_preset(&g.state.mic_preset) && g.cal_for().is_some() {
            g.state.dsp = g.refit(); // les seuils suivent le gain
            gen = g.mark(&[Key::Dsp], true);
        }
        gen
    };
    wait_applied(ctl, gen, Duration::from_secs(4))?;
    Ok(json!({"old_gain_db": old, "new_gain_db": new, "voice_peak_db": peak, "target_db": target_db,
              "message": format!("Gain réglé de {old} à {new} dB.")}))
}

struct Levels {
    low: f64,
    floor: f64,
    peak: f64,
}

fn listen_input(ctl: &Ctl, mtype: &str, gain: i32, settle: f64, seconds: f64, p0: f64, p1: f64) -> JR<Levels> {
    set_gain_and_wait(ctl, mtype, gain)?;
    let mid = p0 + (p1 - p0) * settle / (settle + seconds);
    sample(ctl, settle, p0, mid)?;
    let mut vals = sorted(sample(ctl, seconds, mid, p1)?);
    if vals.is_empty() {
        vals.push(-72.2);
    }
    Ok(Levels { low: pct(&vals, 0.2), floor: pct(&vals, 0.5), peak: *vals.last().unwrap() })
}

/// Quel micro est branche ? XLR sans puis avec 48 V (un micro statique ne fonctionne qu'alimente), puis jack.
fn find_mic(ctl: &Ctl, p0: f64, p1: f64) -> JR<Option<&'static str>> {
    let span = (p1 - p0) / 3.0;
    let settle = if ctl.lock().state.mic.kind == "condenser" { 2.5 } else { 0.8 }; // le 48 V met un instant a retomber
    step(ctl, "Entrée XLR…");
    let dynl = listen_input(ctl, "dynamic", 40, settle, 1.4, p0, p0 + span)?;
    step(ctl, "Entrée XLR avec l’alimentation 48 V…");
    let cond = listen_input(ctl, "condenser", 40, 2.0, 1.4, p0 + span, p0 + 2.0 * span)?;
    if cond.peak > -70.5 && (cond.low >= dynl.low + 8.0 || cond.floor >= dynl.floor + 8.0) {
        return Ok(Some("condenser"));
    }
    if dynl.peak > -70.5 {
        return Ok(Some("dynamic"));
    }
    step(ctl, "Entrée jack 3,5 mm…");
    let jack = listen_input(ctl, "jack", 30, 0.6, 1.4, p0 + 2.0 * span, p1)?;
    Ok((jack.peak > -70.5).then_some("jack"))
}

/// Assistant « Régler ma voix » : quel micro, niveau de ta voix, bruit de ta piece, puis gain et reglages prets.
fn voice_setup(ctl: &Ctl, p: &Value) -> JR<Value> {
    let mtype: String = if p.get("detect").map(truthy).unwrap_or(true) {
        update(ctl, &[("phase", json!("detect")), ("step", json!("Je cherche ton micro…"))]);
        match find_mic(ctl, 0.0, 0.3)? {
            Some(t) => t.into(),
            None => {
                return Err("Aucun micro détecté. Vérifie son câble, dis quelques mots pendant la recherche, puis recommence."
                    .to_string()
                    .into())
            }
        }
    } else {
        ctl.lock().state.mic.kind.clone()
    };
    let (cur_kind, cur_gain) = {
        let g = ctl.lock();
        (g.state.mic.kind.clone(), g.state.mic.gain_db)
    };
    let mut gain = if mtype == cur_kind { cur_gain } else { recommended_gain(&mtype) };
    let voice_s = num(get(p, "voice_seconds")).unwrap_or(10.0).clamp(1.0, 20.0);
    let silence_s = num(get(p, "silence_seconds")).unwrap_or(4.0).clamp(1.0, 10.0);
    // 1. ta voix, au gain de depart ; si l'entree sature : 10 dB de moins et on recommence (deux fois au plus)
    update(ctl, &[("phase", json!("voice")), ("step", json!("J’écoute ta voix…"))]);
    let mut vals = vec![];
    for attempt in 0..3 {
        set_gain_and_wait(ctl, &mtype, gain)?;
        let (v, clipped) = sample_voice(ctl, voice_s, 0.32, 0.78)?;
        vals = v;
        if !(clipped && attempt < 2 && gain > 0) {
            break;
        }
        gain = (gain - 10).max(0);
        step(ctl, &format!("Ta voix sature : gain baissé à {gain} dB, continue de parler."));
    }
    let pauses = if vals.is_empty() { -72.2 } else { pct(&sorted(vals.clone()), 0.1) };
    let heard = sorted(vals.iter().copied().filter(|v| *v > (pauses + 10.0).max(-66.0)).collect());
    if (heard.len() as f64) < 10f64.max(vals.len() as f64 * 0.25) {
        return Err("Je ne t’ai pas assez entendu. Parle plus près du micro (10 à 15 cm) et recommence.".to_string().into());
    }
    let peak = pct(&heard, 0.95);
    let new_gain = clamp_f(gain as f64 + VOICE_TARGET_DB - peak, 0, 72);
    let shift = (new_gain - gain) as f64;
    // 2. le silence, au gain final : la piece, le PC, et ce que le micro entend du casque
    update(ctl, &[("phase", json!("silence")), ("step", json!("J’écoute ta pièce…"))]);
    let (mut noise, mut noise_peak) = (-72.2, -72.2);
    for attempt in 0..2 {
        set_gain_and_wait(ctl, &mtype, new_gain)?;
        sample(ctl, 0.7, 0.78, 0.8)?;
        let mut quiet = sorted(sample(ctl, silence_s, 0.8, 0.95)?);
        if quiet.is_empty() {
            quiet.push(-72.2);
        }
        noise = pct(&quiet, 0.5);
        noise_peak = pct(&quiet, 0.95);
        if noise_peak - noise < 18.0 || noise_peak < -55.0 || attempt > 0 {
            break;
        }
        step(ctl, "J’ai entendu un bruit : on recommence, silence total.");
    }
    // ta voix sans ses pauses : ce qui depasse nettement le bruit mesure (ramene au gain de l'ecoute de ta voix)
    let mut voiced = sorted(vals.iter().copied().filter(|v| *v > (noise_peak - shift + 6.0).max(-66.0)).collect());
    if voiced.is_empty() {
        voiced = heard.clone();
    }
    let (low, med) = (pct(&voiced, 0.15), pct(&voiced, 0.5));
    let cal = state::normalize_cal(&json!({"type": mtype, "gain_db": new_gain, "t": now_epoch(), "noise_db": noise,
        "noise_peak_db": noise_peak, "voice_low_db": low + shift, "voice_db": med + shift, "voice_peak_db": peak + shift}))
        .ok_or_else(|| JobErr::Msg("Mesure illisible".into()))?;
    let mut style = p.get("preset").and_then(|v| v.as_str()).filter(|s| dsp::preset_fit(s).is_some()).map(String::from)
        .unwrap_or_else(|| mic_type_preset(&mtype).to_string());
    update(ctl, &[("phase", json!("apply")), ("step", json!("J’applique tes réglages…")), ("progress", json!(0.96))]);
    let apply = p.get("apply").map(truthy).unwrap_or(false);
    let (gen, d, fx) = {
        let mut g = ctl.lock();
        g.state.mic = state::Mic { kind: mtype.clone(), gain_db: new_gain };
        g.state.mic_cal = Some(cal.clone());
        if !(apply || g.state.fx.any()) {
            style = "naturel".into(); // rien d'actif : des reglages doux, prets
        }
        g.state.dsp = dsp::fit_dsp(&style, Some(&cal), Some(new_gain)); // actives seulement si tu le choisis
        if apply {
            g.state.fx = dsp::fx_from_dsp(&g.state.dsp);
        }
        g.state.mic_preset = if g.state.fx.any() { style.clone() } else { "brut".into() };
        let gen = g.mark(&[Key::Mic, Key::Dsp], true);
        (gen, g.state.dsp.clone(), g.state.fx)
    };
    wait_applied(ctl, gen, Duration::from_secs(4))?;
    logf!(
        "Voix réglée : micro {mtype}, gain {new_gain} dB, bruit {} dB, voix {} dB (crêtes {}), noise gate {} dB, compresseur {} dB",
        cal.noise_peak_db, cal.voice_db, cal.voice_peak_db, d.gate.threshold, d.comp.threshold
    );
    let mut gate = serde_json::to_value(&d.gate).unwrap_or(json!({}));
    gate["attenuation_db"] = json!(dsp::gate_attenuation(d.gate.attenuation));
    let mut comp = serde_json::to_value(&d.comp).unwrap_or(json!({}));
    comp["ratio_x"] = json!(dsp::ratio_x(d.comp.ratio));
    Ok(json!({"type": mtype, "gain_db": new_gain, "preset": style, "cal": cal, "fx": fx,
        "snr_db": ((cal.voice_db - cal.noise_peak_db) * 10.0).round() / 10.0, "gate": gate, "comp": comp,
        "deesser": d.deesser, "notes": voice_notes(ctl, &mtype, new_gain, &cal), "message": "Ta voix est réglée."}))
}

/// Conseils apres le reglage : placement du micro, bruit, et sources d'echo.
fn voice_notes(ctl: &Ctl, mtype: &str, gain: i32, cal: &MicCal) -> Vec<Value> {
    let g = ctl.lock();
    let s = &g.state;
    let mut notes = vec![];
    let snr = cal.voice_db - cal.noise_peak_db;
    if gain >= 72 && cal.voice_peak_db < VOICE_TARGET_DB - 3.0 {
        notes.push(json!({"lvl": "warn", "t": "Ta voix arrive faiblement, même au gain maximum : rapproche-toi du micro (10 à 15 cm)."}));
    } else if mtype == "condenser" && gain >= 42 {
        notes.push(json!({"lvl": "tip", "t": "Tu sembles loin du micro : à 10–15 cm, ta voix sera plus présente et l’écho de la pièce bien plus faible."}));
    } else if gain <= 6 {
        notes.push(json!({"lvl": "tip", "t": "Ta voix arrive très fort : tu peux reculer un peu du micro (10 à 15 cm)."}));
    }
    if snr < 18.0 {
        notes.push(json!({"lvl": "warn", "t": "Ta pièce est bruyante pour ce micro : rapproche-toi du micro, éloigne-le du PC et du clavier, et tourne son dos vers le bruit."}));
    } else if snr < 26.0 {
        notes.push(json!({"lvl": "tip", "t": "Un peu de bruit de fond : allume le noise gate si tes amis l’entendent entre tes phrases."}));
    } else {
        notes.push(json!({"lvl": "ok", "t": "Bruit de fond très faible : ta voix ressort nettement."}));
    }
    if mtype == "condenser" {
        notes.push(json!({"lvl": "tip", "t": "Micro statique : parle à 10–15 cm, face au micro, avec un filtre anti-pop."}));
    }
    if s.route("Chat", "ChatMic") {
        notes.push(json!({"lvl": "warn", "t": "Discord s’entend lui-même : tes amis ont de l’écho.",
            "fix": {"label": "Corriger", "action": "route", "body": {"input": "Chat", "output": "ChatMic", "enabled": false}}}));
    }
    if mtype == "condenser" && s.route("Chat", "LineOut") {
        notes.push(json!({"lvl": "tip", "t": "Si des enceintes sont branchées sur la GoXLR, ton micro y entend Discord (écho chez tes amis) : garde le casque pendant les vocaux.",
            "fix": {"label": "Retirer Discord des enceintes", "action": "route", "body": {"input": "Chat", "output": "LineOut", "enabled": false}}}));
    }
    if s.route("Microphone", "Headphones") && s.volume("MicMonitor") > 64 {
        notes.push(json!({"lvl": "tip", "t": format!("Tu t’entends à {} % dans le casque : avec un casque ouvert, au-delà de 25 % ça peut siffler.",
            (s.volume("MicMonitor") as f64 * 100.0 / 255.0).round() as i32)}));
    }
    notes
}
