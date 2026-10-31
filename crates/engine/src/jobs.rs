//! Analyses du micro : detection (48 V compris), gain automatique, assistant « Régler ma voix ».
use crate::config::now_epoch;
use crate::api::ApiError;
use crate::controller::{Ctl, Key};
use crate::logf;
use rustudio_core::consts::*;
use rustudio_core::dsp::{self, mic_type_preset};
use rustudio_core::state::{self, MicCal};
use rustudio_core::voice::{self, Phase, Segment};
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
        "voice-print" => voice_print(ctl, p),
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
    if std::mem::replace(&mut g.voice_capture, false) {
        // fin de l'empreinte (réussie, annulée ou en erreur) : traitements, routage et volume du micro reviennent
        let mut keys = vec![Key::Dsp, Key::Volume(crate::controller::ch("Mic"))];
        keys.extend((0..INPUTS.len()).map(Key::Route));
        g.mark(&keys, false);
    }
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
        let model = g.state.mic.model.clone().filter(|m| rustudio_core::mics::model(m).map(|x| x.kind == mtype).unwrap_or(false));
        g.state.mic = state::Mic { kind: mtype.clone(), gain_db: new_gain, model };
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


// ---------- empreinte vocale ----------

/// Ce que le moteur a mesuré pendant l'empreinte vocale, en attendant l'audio enregistré par l'appli.
#[derive(Clone, Debug)]
pub struct VoiceTake {
    pub done_at: Instant,
    /// étapes : phase, début et fin (epoch, s), gain du micro
    pub timeline: Vec<(Phase, f64, f64, i32)>,
    pub mic_type: String,
    pub model: Option<String>,
}

/// L'audio doit arriver dans ce délai après la fin de l'enregistrement.
const TAKE_TTL: Duration = Duration::from_secs(900);
/// Taille maximale de l'audio reçu (Int16 mono) : 8 Mo, soit plus de 80 s à 48 kHz.
pub const VOICE_AUDIO_MAX: usize = 8_000_000;

fn phase_name(p: Phase) -> &'static str {
    match p {
        Phase::Silence => "silence",
        Phase::Reading => "reading",
        Phase::Loud => "loud",
    }
}

fn set_phase(ctl: &Ctl, phase: &str, seconds: f64, step_txt: &str) {
    update(ctl, &[("phase", json!(phase)), ("phase_end", json!(now_epoch() + seconds)), ("step", json!(step_txt))]);
}

/// Simulateur : la fausse GoXLR parle pendant la lecture et la phrase forte, et se tait sinon.
struct SimVoice<'a>(&'a Ctl, Option<f64>);
impl<'a> SimVoice<'a> {
    fn new(ctl: &'a Ctl, mtype: &str) -> SimVoice<'a> {
        let base = ctl.sim.as_ref().map(|s| {
            let mut g = s.lock();
            g.dev.mic_input = Some(mtype.to_string());
            g.dev.voice_db
        });
        SimVoice(ctl, base)
    }
    fn talk(&self, on: bool, louder_db: f64) {
        if let (Some(s), Some(base)) = (self.0.sim.as_ref(), self.1) {
            let mut g = s.lock();
            g.dev.talking = on;
            g.dev.voice_db = base + louder_db;
        }
    }
}
impl Drop for SimVoice<'_> {
    fn drop(&mut self) {
        self.talk(false, 0.0);
    }
}

/// Empreinte vocale, côté moteur : silence, lecture, phrase forte, avec des traitements neutres et « Chat Mic » ne
/// portant que ta voix. Le moteur mesure les niveaux (gain, bruit, crêtes) ; l'appli enregistre « Chat Mic » et
/// envoie l'audio ensuite (voice_audio). Si ta voix sature pendant la lecture : 10 dB de gain en moins, on relit.
fn voice_print(ctl: &Ctl, p: &Value) -> JR<Value> {
    let silence_s = num(get(p, "silence_s")).unwrap_or(5.0).clamp(1.5, 10.0);
    let reading_s = num(get(p, "reading_s")).unwrap_or(16.0).clamp(3.0, 30.0);
    let loud_s = num(get(p, "loud_s")).unwrap_or(5.0).clamp(1.5, 10.0);
    let (mtype, model, old_gain) = {
        let g = ctl.lock();
        if g.state.muted("Mic") || g.state.partial_of("Mic") == Some("chat") {
            return Err("Ton micro est coupé : réactive-le, puis recommence.".to_string().into());
        }
        (g.state.mic.kind.clone(), g.state.mic.model.clone(), g.state.mic.gain_db)
    };
    let sim = SimVoice::new(ctl, &mtype);
    let mut gain = old_gain;
    let mut timeline: Vec<(Phase, f64, f64, i32)> = vec![];
    // 1. préparation : traitements neutres, ta voix seule vers Chat Mic, volume du micro au maximum
    set_phase(ctl, "prepare", 1.2, "Je prépare l’enregistrement…");
    let gen = {
        let mut g = ctl.lock();
        g.voice_capture = true;
        g.ab_until = None;
        g.gain_override = Some((mtype.clone(), gain));
        let mut keys = vec![Key::Dsp, Key::Mic, Key::Volume(crate::controller::ch("Mic"))];
        keys.extend((0..INPUTS.len()).map(Key::Route));
        g.mark(&keys, false)
    };
    wait_applied(ctl, gen, Duration::from_secs(4))?;
    sample(ctl, 0.9, 0.0, 0.04)?;
    let total = silence_s + reading_s + loud_s;
    let (a, b) = (0.04, 0.04 + 0.9 * silence_s / total);
    // 2. silence : le bruit de ta pièce
    set_phase(ctl, "silence", silence_s, "Silence…");
    let t0 = now_epoch();
    sample(ctl, 0.3_f64.min(silence_s / 4.0), a, a + 0.01)?;
    let mut quiet = sorted(sample(ctl, silence_s - 0.3_f64.min(silence_s / 4.0), a + 0.01, b)?);
    timeline.push((Phase::Silence, t0, now_epoch(), gain));
    if quiet.is_empty() {
        quiet.push(-72.2);
    }
    let (noise, noise_peak, noise_gain) = (pct(&quiet, 0.5), pct(&quiet, 0.95), gain);
    // 3. lecture
    let (c, d) = (b, b + 0.9 * reading_s / total);
    let mut vals = vec![];
    for attempt in 0..3 {
        update(ctl, &[("retry", json!(attempt))]);
        let why = if attempt == 0 {
            "Lis le texte à voix haute".to_string()
        } else {
            format!("Ta voix saturait : gain baissé à {gain} dB. Relis le texte, même voix.")
        };
        set_phase(ctl, "reading", reading_s, &why);
        sim.talk(true, 0.0);
        let t0 = now_epoch();
        let (v, clipped) = sample_voice(ctl, reading_s, c, d)?;
        timeline.push((Phase::Reading, t0, now_epoch(), gain));
        sim.talk(false, 0.0);
        vals = v;
        if !(clipped && attempt < 2 && gain > 0) {
            break;
        }
        gain = (gain - 10).max(0);
        set_gain_and_wait(ctl, &mtype, gain)?;
        step(ctl, &format!("Ta voix saturait : gain baissé à {gain} dB. Relis le texte, même voix."));
        sample(ctl, 0.8, c, c)?;
    }
    // 4. phrase forte : la marge avant saturation
    set_phase(ctl, "loud", loud_s, "Plus fort !");
    sim.talk(true, 8.0);
    let t0 = now_epoch();
    let loud = sorted(sample(ctl, loud_s, d, 0.97)?);
    timeline.push((Phase::Loud, t0, now_epoch(), gain));
    sim.talk(false, 0.0);
    drop(sim);
    // 5. niveaux : ta voix sans ses pauses, ses crêtes, et le gain qui les place vers -12 dB sans saturer en criant
    let pauses = if vals.is_empty() { -72.2 } else { pct(&sorted(vals.clone()), 0.1) };
    let heard = sorted(vals.iter().copied().filter(|v| *v > (pauses + 10.0).max(-66.0)).collect());
    if (heard.len() as f64) < 10f64.max(vals.len() as f64 * 0.25) {
        return Err("Je ne t’ai pas assez entendu. Parle plus près du micro (10 à 15 cm) et recommence.".to_string().into());
    }
    let peak = pct(&heard, 0.95);
    let loud_peak = loud.last().copied().filter(|l| *l > peak);
    let mut new_gain = clamp_f(gain as f64 + VOICE_TARGET_DB - peak, 0, 72);
    if let Some(lp) = loud_peak {
        new_gain = new_gain.min((gain as f64 + CLIP_DB - 3.0 - lp).floor().max(0.0) as i32);
    }
    let shift = (new_gain - gain) as f64;
    let noise_shift = (new_gain - noise_gain) as f64;
    let mut voiced = sorted(vals.iter().copied().filter(|v| *v > (noise_peak + (gain - noise_gain) as f64 + 6.0).max(-66.0)).collect());
    if voiced.is_empty() {
        voiced = heard.clone();
    }
    let (low, med) = (pct(&voiced, 0.15), pct(&voiced, 0.5));
    let cal = state::normalize_cal(&json!({"type": mtype, "gain_db": new_gain, "t": now_epoch(), "noise_db": noise + noise_shift,
        "noise_peak_db": noise_peak + noise_shift, "voice_low_db": low + shift, "voice_db": med + shift,
        "voice_peak_db": peak + shift, "loud_peak_db": loud_peak.map(|l| l + shift)}))
    .ok_or_else(|| JobErr::Msg("Mesure illisible".into()))?;
    update(ctl, &[("phase", json!("analyse")), ("step", json!("J’attends l’enregistrement…")), ("progress", json!(0.98))]);
    let gen = {
        let mut g = ctl.lock();
        g.state.mic = state::Mic { kind: mtype.clone(), gain_db: new_gain, model: model.clone() };
        g.state.mic_cal = Some(cal.clone());
        g.voice_take = Some(VoiceTake { done_at: Instant::now(), timeline: timeline.clone(), mic_type: mtype.clone(), model });
        g.mark(&[Key::Mic], true)
    };
    wait_applied(ctl, gen, Duration::from_secs(4))?;
    let mut notes = vec![];
    if gain != old_gain {
        notes.push(json!({"lvl": "warn", "t": format!("Ta voix saturait : gain baissé de {old_gain} à {gain} dB pendant l’enregistrement.")}));
    }
    if new_gain != gain {
        notes.push(json!({"lvl": "ok", "t": format!("Gain réglé à {new_gain} dB : tes crêtes arrivent vers \u{2212}12 dB, sans saturer quand tu cries.")}));
    }
    logf!(
        "Empreinte vocale : micro {mtype}, gain {old_gain} → {new_gain} dB, bruit {:.1} dB, voix {:.1} dB (crêtes {:.1}, fort {:?})",
        cal.noise_peak_db, cal.voice_db, cal.voice_peak_db, cal.loud_peak_db
    );
    Ok(json!({"gain_db": new_gain, "old_gain_db": old_gain, "cal": cal,
        "timeline": timeline.iter().map(|(ph, a, b, g)| json!({"phase": phase_name(*ph), "start": a, "end": b, "gain_db": g})).collect::<Vec<_>>(),
        "notes": notes, "audio": "wanted"}))
}

fn query_value<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query.split('&').filter_map(|kv| kv.split_once('=')).find(|(k, _)| *k == key).map(|(_, v)| v)
}

/// Audio de l'empreinte (POST /api/modules/goxlr-mini/voice-audio?rate=48000&t0=<epoch ms>&format=s16, Int16 mono) :
/// analyse, empreinte gardée avec le micro, préréglage actif recalculé sur mesure. En simulation, ?synthetic=1 sans
/// corps analyse une voix de synthèse.
pub fn voice_audio(ctl: &Ctl, query: &str, body: &[u8]) -> Result<Value, ApiError> {
    let take = ctl.lock().voice_take.clone().filter(|t| t.done_at.elapsed() < TAKE_TTL)
        .ok_or_else(|| ApiError::bad("Aucun enregistrement en attente : refais l’empreinte."))?;
    let first = take.timeline.first().map(|t| t.1).unwrap_or_else(now_epoch);
    let (samples, rate, t0) = if query_value(query, "synthetic") == Some("1") {
        if ctl.sim.is_none() {
            return Err(ApiError::bad("La voix de démonstration n’existe qu’en simulation."));
        }
        let t0 = first - 0.5;
        let segs = segments(&take, t0, f64::MAX);
        let seed = (now_epoch() * 1000.0) as u64;
        (voice::synth_speech(48_000, &segs, 118.0, seed), 48_000u32, t0)
    } else {
        if query_value(query, "format").unwrap_or("s16") != "s16" {
            return Err(ApiError::bad("format audio non pris en charge (s16 attendu)"));
        }
        let rate: u32 = query_value(query, "rate").and_then(|r| r.parse().ok()).filter(|r| (16_000..=96_000).contains(r))
            .ok_or_else(|| ApiError::bad("fréquence d’échantillonnage invalide"))?;
        let t0 = query_value(query, "t0").and_then(|t| t.parse::<f64>().ok()).filter(|t| t.is_finite() && *t > 0.0)
            .ok_or_else(|| ApiError::bad("début de l’enregistrement inconnu (t0)"))? / 1000.0;
        if body.len() < 2 || body.len() % 2 != 0 || body.len() > VOICE_AUDIO_MAX {
            return Err(ApiError::bad("enregistrement vide ou illisible"));
        }
        let x: Vec<f32> = body.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect();
        (x, rate, t0)
    };
    let dur = samples.len() as f64 / rate as f64;
    let segs = segments(&take, t0, dur);
    if !segs.iter().any(|s| s.phase == Phase::Reading) {
        return Err(ApiError::bad("L’enregistrement ne couvre pas la lecture : refais l’empreinte."));
    }
    let print = voice::analyze(&samples, rate, &segs).map_err(|e| ApiError::bad(format!("Analyse impossible : {e}")))?;
    if print.speech_s < 4.0 || print.warnings.iter().any(|w| w == "too_short" || w == "too_quiet") {
        return Err(ApiError::bad("Je ne t’ai pas assez entendu dans l’enregistrement. Parle plus près du micro et refais l’empreinte."));
    }
    let mut g = ctl.lock();
    logf!(
        "Empreinte analysée : {:.1} s de parole, fondamentale {:?} Hz, sifflantes {:.1} dB, {} plosive(s), pièce {:?} ms",
        print.speech_s, print.f0_hz.map(|f| f.round()), print.sibilance.excess_db, print.plosives.count, print.room.rt60_ms.map(|r| r.round())
    );
    g.state.voice_print = Some(state::VoiceRecord { print, t: now_epoch(), mic_type: take.mic_type, model: take.model });
    g.voice_take = None;
    if dsp::is_preset(&g.state.mic_preset) {
        g.state.dsp = g.refit(); // le préréglage actif devient sur mesure
        g.mark(&[Key::Dsp], true);
    }
    g.version += 1;
    g.save_dirty = true;
    let mut v = rustudio_core::voicefit::voice_view(&g.state);
    v["ok"] = json!(true);
    Ok(v)
}

/// Étapes de l'empreinte en secondes depuis le premier échantillon (t0), bornées à la durée enregistrée.
fn segments(take: &VoiceTake, t0: f64, dur: f64) -> Vec<Segment> {
    take.timeline
        .iter()
        .map(|(phase, a, b, g)| Segment { phase: *phase, start_s: (a - t0).max(0.0), end_s: (b - t0).min(dur), gain_db: *g })
        .filter(|s| s.end_s - s.start_s >= 0.5)
        .collect()
}
