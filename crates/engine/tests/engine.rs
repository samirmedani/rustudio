//! Tests du moteur : boucle USB reelle branchee sur la fausse GoXLR (aucun materiel).
use rustudio_core::consts::{button_bit, BTN_FLASH, BTN_ON};
use rustudio_engine::config::Config;
use rustudio_engine::controller::Ctl;
use rustudio_goxlr::sim::Sim;
use rustudio_goxlr::Timing;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

static N: AtomicUsize = AtomicUsize::new(0);

pub fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rustudio-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub fn wait_for(timeout: f64, mut f: impl FnMut() -> bool) -> bool {
    let t0 = Instant::now();
    while t0.elapsed().as_secs_f64() < timeout {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

struct Bench {
    ctl: Arc<Ctl>,
    sim: Sim,
    stop: Arc<AtomicBool>,
    th: Option<JoinHandle<()>>,
    tmp: PathBuf,
}

impl Bench {
    fn new() -> Bench {
        Bench::with_fw([1, 2, 1, 49])
    }
    fn with_fw(fw: [u32; 4]) -> Bench {
        rustudio_engine::log::set_echo(false);
        let tmp = tmpdir("eng");
        let cfg = Config { simulate: true, port: 0, data_dir: tmp.clone(), base_dir: tmp.clone(), supervised: false, engine_restarts: 0 };
        let sim = Sim::new(fw);
        sim.lock().latency = Duration::ZERO;
        let s2 = sim.clone();
        let ctl = Ctl::new(cfg, Box::new(move || Ok(Box::new(s2.tusb()) as Box<dyn rustudio_goxlr::Tusb>)), Some(sim.clone()), true);
        ctl.with_dev(|d| d.timing = Timing { first_read_wait: Duration::ZERO, retry_wait: Duration::from_micros(500), response_timeout: Duration::from_millis(500) });
        *ctl.poll_interval.lock().unwrap() = Duration::from_millis(2);
        ctl.lock().state.prefs.hold_ms = 300;
        let stop = Arc::new(AtomicBool::new(false));
        let (c, s) = (ctl.clone(), stop.clone());
        let th = std::thread::spawn(move || c.device_loop(Some(s)));
        let b = Bench { ctl, sim, stop, th: Some(th), tmp };
        assert!(wait_for(5.0, || {
            let g = b.ctl.lock();
            g.connected && g.dirty.is_empty() && g.done >= g.want && g.prev_pos.is_some()
        }));
        b
    }
    fn settle(&self) {
        assert!(wait_for(5.0, || {
            let g = self.ctl.lock();
            g.dirty.is_empty() && g.done >= g.want && g.connected
        }));
        std::thread::sleep(Duration::from_millis(20));
    }
    fn act(&self, a: &str, p: Value) -> Value {
        self.ctl.act(a, &p).unwrap_or_else(|e| panic!("{a} : {}", e.msg))
    }
    fn stop_loop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.th.take() {
            t.join().unwrap();
        }
    }
    fn led(&self, button: &str) -> u8 {
        self.sim.lock().dev.button_states[button_bit(button).unwrap() as usize]
    }
    fn job(&self) -> Value {
        self.ctl.lock().job.clone().map(Value::Object).unwrap_or(Value::Null)
    }
    fn job_done(&self, timeout: f64) -> Value {
        assert!(wait_for(timeout, || self.job()["running"] == json!(false)), "analyse trop longue : {}", self.job());
        let j = self.job();
        assert!(j["error"].is_null(), "{j}");
        j["result"].clone()
    }
}
impl Drop for Bench {
    fn drop(&mut self) {
        self.stop_loop();
        let _ = std::fs::remove_dir_all(&self.tmp);
    }
}

// ---------- la panne du 20 octobre ----------
#[test]
fn index_16_bits_passe_sans_erreur() {
    let mut b = Bench::new();
    b.stop_loop();
    b.ctl.with_dev(|d| {
        d.open().unwrap();
        let start = d.count;
        for _ in 0..(70000 - start) {
            d.get_button_states().unwrap();
        }
        let st = d.info.lock().unwrap().stats.clone();
        assert_eq!(st.index_resets, 1);
        assert!(d.count < 6000);
        assert_eq!(st.resyncs, 0, "la Mini simulee repond avec son propre compteur : aucun decalage");
    });
    let s = b.sim.lock();
    assert!(s.dev.index_resets >= 1);
    assert!(s.dev.errors.is_empty(), "{:?}", s.dev.errors);
}

#[test]
fn saut_de_compteur_rattrape() {
    let mut b = Bench::new();
    b.stop_loop();
    b.ctl.with_dev(|d| {
        d.open().unwrap();
        d.count = 60000;
        d.get_button_states().unwrap();
        assert_eq!(d.info.lock().unwrap().stats.resyncs, 1);
        d.get_button_states().unwrap();
    });
}

#[test]
fn erreur_inattendue_reconnecte() {
    let b = Bench::new();
    b.sim.lock().fail_next = vec![3992977495];
    assert!(wait_for(6.0, || {
        let g = b.ctl.lock();
        g.reconnects == 1 && g.connected
    }));
    assert!(b.ctl.lock().errors.iter().any(|(_, m)| m.contains("3992977495")));
    assert_eq!(b.sim.lock().enumerate_violations, 0);
}

#[test]
fn debranche_rebranche_reapplique() {
    let b = Bench::new();
    b.sim.unplug();
    assert!(wait_for(3.0, || !b.ctl.lock().connected));
    b.act("volume", json!({"channel": "Game", "value": 211}));
    b.act("mute", json!({"channel": "Chat", "muted": true}));
    b.sim.plug();
    assert!(wait_for(6.0, || b.ctl.lock().connected));
    b.settle();
    let s = b.sim.lock();
    assert_eq!(s.dev.volume("Game"), 211);
    assert!(s.dev.is_muted("Chat"));
    assert_eq!(s.enumerate_violations, 0, "EnumerateDevices jamais appele handle ouvert");
}

#[test]
fn absente_dit_ce_que_voit_windows() {
    let b = Bench::new();
    *b.ctl.absent_retry.lock().unwrap() = Duration::from_millis(100);
    b.sim.unplug();
    assert!(wait_for(3.0, || {
        let g = b.ctl.lock();
        g.usb.as_deref() == Some("none") && b.ctl.health_status(&g) == "absent"
    }));
    assert_eq!(b.ctl.live_snapshot(false)["usb"], "none");
    b.sim.lock().unplugged_usb = "failed".into();
    assert!(wait_for(3.0, || b.ctl.lock().usb.as_deref() == Some("failed")));
    b.sim.plug();
    assert!(wait_for(8.0, || {
        let g = b.ctl.lock();
        g.connected && g.usb.as_deref() == Some("mini")
    }));
    assert!(rustudio_engine::log::recent(80).iter().any(|l| l.contains("ne fait peut-être que charger")));
}

#[test]
fn api_repond_pendant_un_blocage_usb() {
    let b = Bench::new();
    b.sim.hang(1.5);
    std::thread::sleep(Duration::from_millis(200));
    let t0 = Instant::now();
    let r = b.act("volume", json!({"channel": "Music", "value": 100}));
    let snap = b.ctl.live_snapshot(false);
    assert!(t0.elapsed() < Duration::from_millis(200));
    assert_eq!(r["ok"], json!(true));
    assert!(wait_for(2.0, || b.ctl.health()["loop_age_ms"].as_u64().unwrap_or(0) > 800));
    b.settle();
    assert_eq!(b.sim.lock().dev.volume("Music"), 100);
    assert!(snap.get("v").is_some());
}

#[test]
fn formats_envoyes_justes_et_sous_mixages() {
    let b = Bench::new();
    b.act("scene", json!({"name": "streaming"}));
    b.settle();
    let s = b.sim.lock();
    assert!(s.dev.errors.is_empty(), "{:?}", s.dev.errors);
    assert_eq!(s.dev.colour_map.len(), 520);
    assert_eq!(s.dev.monitored_mix, Some(0));
    assert_eq!(s.dev.channel_mixes.as_deref(), Some(&[2u8, 4, 6, 8, 255, 255, 255, 255][..]));
}

#[test]
fn stream_mix_2_sur_firmware_1_3() {
    let b = Bench::with_fw([1, 3, 0, 60]);
    b.act("scene", json!({"name": "streaming"}));
    b.settle();
    let s = b.sim.lock();
    assert!(s.dev.errors.is_empty(), "26 octets de routage : {:?}", s.dev.errors);
    assert_eq!(s.dev.channel_mixes.as_ref().map(|m| m.len()), Some(10));
}

// ---------- mutes ----------
#[test]
fn couper_du_stream_seulement() {
    let b = Bench::new();
    b.act("mute", json!({"channel": "Music", "muted": true, "target": "stream"}));
    b.settle();
    {
        let s = b.sim.lock();
        assert!(!s.dev.is_muted("Music"), "la voie reste active");
        assert!(!s.dev.route_on("Music", "BroadcastMix"));
        assert!(s.dev.route_on("Music", "Headphones"));
    }
    let fader = b.ctl.lock().state.faders.iter().find(|(_, c)| c.as_str() == "Music").map(|(f, _)| f.clone()).unwrap();
    let idx = ["A", "B", "C", "D"].iter().position(|x| *x == fader).unwrap();
    assert_eq!(b.led(["Fader1Mute", "Fader2Mute", "Fader3Mute", "Fader4Mute"][idx]), BTN_ON);
    b.act("mute", json!({"channel": "Music", "muted": false}));
    b.settle();
    assert!(b.sim.lock().dev.route_on("Music", "BroadcastMix"));
}

#[test]
fn micro_coupe_partout_coupe_ses_routes() {
    let b = Bench::new();
    b.act("mute", json!({"channel": "Mic", "muted": true}));
    b.settle();
    let s = b.sim.lock();
    assert!(s.dev.muted[0]);
    assert_eq!(s.dev.effects.get(&0x0158), Some(&1));
    assert!(!s.dev.route_on("Microphone", "ChatMic"));
}

#[test]
fn appui_court_suit_le_mode_du_fader() {
    let b = Bench::new();
    b.act("fader", json!({"fader": "B", "channel": "Music"}));
    b.act("settings", json!({"mute_modes": {"B": "stream"}}));
    b.settle();
    b.sim.press("Fader2Mute", 120.0);
    assert!(wait_for(2.0, || b.ctl.lock().state.partial_of("Music") == Some("stream")));
    assert!(!b.ctl.lock().state.muted("Music"));
    std::thread::sleep(Duration::from_millis(150));
    b.sim.press("Fader2Mute", 120.0);
    assert!(wait_for(2.0, || b.ctl.lock().state.partial_of("Music").is_none()));
}

#[test]
fn appui_long_coupe_partout_et_clignote() {
    let b = Bench::new();
    b.act("fader", json!({"fader": "B", "channel": "Music"}));
    b.act("settings", json!({"mute_modes": {"B": "stream"}}));
    b.settle();
    b.sim.button_down("Fader2Mute");
    assert!(wait_for(2.0, || b.ctl.lock().state.muted("Music")));
    b.sim.button_up("Fader2Mute");
    b.settle();
    assert!(b.ctl.lock().state.muted("Music"), "le relachement apres un appui long ne reactive pas");
    assert_eq!(b.led("Fader2Mute"), BTN_FLASH);
    assert!(b.sim.lock().dev.is_muted("Music"));
}

#[test]
fn bouton_micro_appuyer_pour_parler() {
    let b = Bench::new();
    b.act("settings", json!({"mic_button": {"mode": "ptt", "target": "all"}}));
    b.settle();
    assert!(b.ctl.lock().state.muted("Mic"));
    b.sim.button_down("MicrophoneMute");
    assert!(wait_for(2.0, || !b.ctl.lock().state.muted("Mic")));
    b.settle();
    assert!(!b.sim.lock().dev.muted[0]);
    b.sim.button_up("MicrophoneMute");
    assert!(wait_for(2.0, || b.ctl.lock().state.muted("Mic")));
}

#[test]
fn bouton_micro_maintenir_pour_couper() {
    let b = Bench::new();
    b.act("settings", json!({"mic_button": {"mode": "hold", "target": "chat"}}));
    b.sim.button_down("MicrophoneMute");
    assert!(wait_for(2.0, || b.ctl.lock().state.partial_of("Mic") == Some("chat")));
    b.settle();
    {
        let s = b.sim.lock();
        assert!(!s.dev.route_on("Microphone", "ChatMic"));
        assert!(s.dev.route_on("Microphone", "BroadcastMix"));
    }
    b.sim.button_up("MicrophoneMute");
    assert!(wait_for(2.0, || b.ctl.lock().state.partial_of("Mic").is_none()));
}

#[test]
fn faders_physiques() {
    let b = Bench::new();
    b.sim.move_fader(2, 77);
    let c = b.ctl.lock().state.faders["C"].clone();
    assert!(wait_for(2.0, || b.ctl.lock().state.volume(&c) == 77));
    // une valeur lue une seule fois ne change rien
    let a = b.ctl.lock().state.faders["A"].clone();
    let before = b.ctl.lock().state.volume(&a);
    {
        let mut g = b.ctl.lock();
        for v in [128u8, 255, 128, 128, 128, 128] {
            g.handle_inputs(0, [v, 128, 77, 128], Instant::now());
        }
    }
    assert_eq!(b.ctl.lock().state.volume(&a), before);
    // faders lus a 0 depuis la connexion : position inconnue
    {
        let mut g = b.ctl.lock();
        g.prev_pos = None;
        g.faders_seen = false;
        for _ in 0..5 {
            g.handle_inputs(0, [0; 4], Instant::now());
        }
        assert_eq!(g.live.fader_positions, [None; 4]);
    }
}

#[test]
fn fader_physique_applique_le_volume_au_mixeur() {
    let mut b = Bench::new();
    b.stop_loop();
    let c = b.ctl.lock().state.faders["A"].clone();
    b.ctl.lock().dirty.clear();
    {
        let mut g = b.ctl.lock();
        for v in [90u8, 91, 90, 91, 90] {
            g.handle_inputs(0, [v, 128, 128, 128], Instant::now());
        }
        assert_eq!(g.state.volume(&c), 90 as i32, "le fader bouge, le volume suit");
        assert!(g.dirty.contains(&rustudio_engine::controller::Key::Volume(rustudio_engine::controller::ch(&c))),
            "le nouveau volume doit partir vers la GoXLR : {:?}", g.dirty);
    }
}

// ---------- routage ----------
#[test]
fn vod_sans_musique() {
    let b = Bench::new();
    b.act("settings", json!({"vod": {"mode": "no-music"}}));
    b.settle();
    {
        let s = b.sim.lock();
        assert_eq!(s.dev.route_on("Game", "Sampler"), s.dev.route_on("Game", "BroadcastMix"));
        assert!(s.dev.route_on("Game", "Sampler"));
        assert!(!s.dev.route_on("Music", "Sampler"));
    }
    b.act("mute", json!({"channel": "Game", "muted": true, "target": "stream"}));
    b.settle();
    assert!(!b.sim.lock().dev.route_on("Game", "Sampler"), "coupe du stream = coupe aussi de la VOD");
}

#[test]
fn scenes_ne_touchent_ni_voix_ni_retour() {
    let b = Bench::new();
    b.act("mic-preset", json!({"name": "studio"}));
    assert!(b.ctl.lock().state.fx.gate && b.ctl.lock().state.fx.comp);
    b.act("fx", json!({"comp": false}));
    b.settle();
    assert_eq!(b.sim.lock().dev.effects.get(&0x013C), Some(&0), "compresseur coupe : 1:1");
    assert_eq!(b.ctl.lock().state.mic_preset, "custom");
    b.act("fx", json!({"eq": false, "gate": false, "deesser": false}));
    assert_eq!(b.ctl.lock().state.mic_preset, "brut");
    b.settle();
    assert_eq!(b.sim.lock().dev.effects.get(&0x0011), Some(&-59), "gate ouvert");
    b.act("route", json!({"input": "Microphone", "output": "Headphones", "enabled": true}));
    let fx = b.ctl.lock().state.fx;
    b.act("scene", json!({"name": "gaming"}));
    assert_eq!(b.ctl.lock().state.fx, fx);
    assert!(b.ctl.lock().state.route("Microphone", "Headphones"));
}

#[test]
fn m_ecouter_puis_retour() {
    let b = Bench::new();
    b.act("route", json!({"input": "Microphone", "output": "Headphones", "enabled": false}));
    b.act("volume", json!({"channel": "MicMonitor", "value": 0}));
    b.settle();
    b.act("mic-listen", json!({"seconds": 0.4, "level": 120}));
    assert!(wait_for(2.0, || {
        let s = b.sim.lock();
        s.dev.volume("MicMonitor") == 120 && s.dev.route_on("Microphone", "Headphones")
    }));
    assert!(b.ctl.live_snapshot(false)["listening"].as_f64().unwrap() > 0.0);
    assert!(wait_for(3.0, || {
        let s = b.sim.lock();
        s.dev.volume("MicMonitor") == 0 && !s.dev.route_on("Microphone", "Headphones")
    }));
    assert_eq!(b.ctl.live_snapshot(false)["listening"], json!(0.0));
    assert_eq!(b.ctl.lock().state.volume("MicMonitor"), 0, "l'etat enregistre n'a pas bouge");
}

// ---------- analyses micro ----------
#[test]
fn gain_auto() {
    let b = Bench::new();
    {
        let mut s = b.sim.lock();
        s.dev.mic_input = Some("dynamic".into());
        s.dev.talking = true;
    }
    {
        let mut g = b.ctl.lock();
        g.state.mic = rustudio_core::state::Mic { kind: "dynamic".into(), gain_db: 40 };
        g.mark(&[rustudio_engine::controller::Key::Mic], true);
    }
    b.settle();
    b.act("auto-gain", json!({"seconds": 1.2}));
    let r = b.job_done(6.0);
    let new = r["new_gain_db"].as_i64().unwrap();
    assert!((52..=64).contains(&new), "{new}");
    b.settle();
    assert_eq!(*b.sim.lock().dev.gains.get("dynamic").unwrap() as i64, new);
}

#[test]
fn detection_micro_jack() {
    let b = Bench::new();
    b.sim.lock().dev.mic_input = Some("jack".into());
    b.act("detect-mic", json!({}));
    let r = b.job_done(10.0);
    assert_eq!(r["detected"], "jack", "{r}");
    b.settle();
    assert_eq!(b.sim.lock().dev.mic_type, b.ctl.lock().state.mic.kind, "le type d'origine est remis apres l'analyse");
}

fn voice_setup(b: &Bench, body: Value, timeout: f64) -> Value {
    let done = Arc::new(AtomicBool::new(false));
    let (d2, ctl, sim) = (done.clone(), b.ctl.clone(), b.sim.clone());
    let speaker = std::thread::spawn(move || {
        while !d2.load(Ordering::SeqCst) {
            let phase = ctl.lock().job.as_ref().and_then(|j| j.get("phase").and_then(|p| p.as_str()).map(String::from));
            sim.lock().dev.talking = matches!(phase.as_deref(), Some("detect") | Some("voice"));
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    b.act("voice-setup", body);
    let r = b.job_done(timeout);
    done.store(true, Ordering::SeqCst);
    speaker.join().unwrap();
    r
}

#[test]
fn regler_ma_voix_micro_statique() {
    let b = Bench::new();
    b.sim.lock().dev.mic_input = Some("condenser".into());
    {
        let mut g = b.ctl.lock();
        g.state.mic = rustudio_core::state::Mic { kind: "dynamic".into(), gain_db: 30 };
        g.mark(&[rustudio_engine::controller::Key::Mic], true);
    }
    b.settle();
    let res = voice_setup(&b, json!({"voice_seconds": 2.5, "silence_seconds": 1.2, "apply": true}), 40.0);
    b.settle();
    let (st, cal) = (b.ctl.lock().state.clone(), res["cal"].clone());
    assert_eq!((res["type"].as_str().unwrap(), st.mic.kind.as_str(), st.mic_preset.as_str()), ("condenser", "condenser", "studio"));
    assert!(st.fx.gate && st.fx.comp, "« appliquer » allume les traitements du preregle");
    assert!(b.sim.lock().dev.phantom, "48 V allume pour le micro statique");
    assert_eq!(*b.sim.lock().dev.gains.get("condenser").unwrap(), st.mic.gain_db);
    let peak = cal["voice_peak_db"].as_f64().unwrap();
    assert!((-14.5..=-9.5).contains(&peak), "{cal}");
    let gate = st.dsp.gate.threshold as f64;
    assert!(cal["noise_peak_db"].as_f64().unwrap() + 3.0 <= gate && gate <= cal["voice_low_db"].as_f64().unwrap(), "{gate} {cal}");
    assert!((st.dsp.comp.threshold as f64) < peak);
    assert!(res["notes"].as_array().unwrap().iter().any(|n| n["t"].as_str().unwrap().to_lowercase().contains("bruit de fond")));
    // les seuils suivent le gain : 6 dB de plus, gate 6 dB plus haut
    b.act("mic", json!({"gain_db": st.mic.gain_db + 6}));
    assert_eq!(b.ctl.lock().state.dsp.gate.threshold, (-10).min(st.dsp.gate.threshold + 6));
}

#[test]
fn regler_ma_voix_n_active_rien() {
    let b = Bench::new();
    b.sim.lock().dev.mic_input = Some("condenser".into());
    {
        let mut g = b.ctl.lock();
        g.state.mic = rustudio_core::state::Mic { kind: "condenser".into(), gain_db: 30 };
        g.mark(&[rustudio_engine::controller::Key::Mic], true);
    }
    b.settle();
    let res = voice_setup(&b, json!({"voice_seconds": 2.5, "silence_seconds": 1.2}), 40.0);
    let st = b.ctl.lock().state.clone();
    assert!(!st.fx.any());
    assert_eq!(st.mic_preset, "brut");
    assert_eq!(res["fx"], json!(st.fx));
    let fx: std::collections::HashMap<u32, i32> = b.ctl.lock().dsp_lists().1.into_iter().collect();
    assert_eq!((fx[&0x0011], fx[&0x013C], fx[&0x0140], fx[&0x000B]), (-59, 0, 0, 0), "rien de traite");
    b.act("fx", json!({"gate": true}));
    let g = b.ctl.lock().state.dsp.gate.clone();
    assert!((g.threshold as f64) <= res["cal"]["voice_low_db"].as_f64().unwrap() - 10.0 + 1.0);
    assert!(-(rustudio_core::dsp::gate_attenuation(g.attenuation) as f64) <= rustudio_core::dsp::GATE_SOFT_DB);
}

#[test]
fn regler_ma_voix_qui_sature() {
    let b = Bench::new();
    {
        let mut s = b.sim.lock();
        s.dev.mic_input = Some("dynamic".into());
        s.dev.voice_db = -2.0; // voix tres forte a 40 dB : l'entree sature
    }
    {
        let mut g = b.ctl.lock();
        g.state.mic = rustudio_core::state::Mic { kind: "dynamic".into(), gain_db: 45 };
        g.mark(&[rustudio_engine::controller::Key::Mic], true);
    }
    b.settle();
    let res = voice_setup(&b, json!({"detect": false, "voice_seconds": 2.0, "silence_seconds": 1.0}), 40.0);
    let peak = res["cal"]["voice_peak_db"].as_f64().unwrap();
    assert!((-14.5..=-9.5).contains(&peak), "{res}");
    assert!(res["gain_db"].as_i64().unwrap() < 35);
}

// ---------- scenes automatiques, persistance ----------
#[test]
fn scenes_automatiques() {
    let b = Bench::new();
    let c = &b.ctl;
    let set = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<std::collections::HashSet<_>>();
    b.act("auto-scenes", json!({"enabled": true, "rules": [{"app": "obs64.exe", "scene": "streaming"}], "fallback": "ecoute"}));
    c.lock().auto_seen = false; // comme au demarrage du moteur
    assert_eq!(c.auto_scene_tick(&set(&["obs64.exe"])), None, "OBS deja ouvert au demarrage : on ne touche a rien");
    assert_eq!(c.auto_scene_tick(&set(&[])).as_deref(), Some("ecoute"), "OBS ferme : scene de repli");
    assert_eq!(c.auto_scene_tick(&set(&["OBS64.EXE"])).as_deref(), Some("streaming"));
    assert_eq!(c.lock().state.scene.as_deref(), Some("streaming"));
    assert_eq!(c.auto_scene_tick(&set(&["obs64.exe", "spotify.exe"])), None);
    assert_eq!(c.auto_scene_tick(&set(&["spotify.exe"])).as_deref(), Some("ecoute"));
    b.act("auto-scenes", json!({"enabled": false}));
    assert_eq!(c.auto_scene_tick(&set(&["obs64.exe"])), None);
}

#[test]
fn etat_sauve_et_relu() {
    let b = Bench::new();
    // premier lancement : l'etat s'ecrit sans attendre un changement
    b.ctl.save_if_dirty();
    assert!(b.tmp.join("state.json").exists(), "état du premier lancement non écrit");
    b.act("volume", json!({"channel": "Chat", "value": 33}));
    b.act("settings", json!({"mic_button": {"mode": "hold"}}));
    b.ctl.save_if_dirty();
    let v: Value = serde_json::from_str(&std::fs::read_to_string(b.tmp.join("state.json")).unwrap()).unwrap();
    let st = rustudio_core::state::normalize_state(&v);
    assert_eq!(st.volume("Chat"), 33);
    assert_eq!(st.mic_button.mode, "hold");
}

// ---------- egaliseur du casque (simule) ----------
#[test]
fn egaliseur_installation_simulee_puis_live() {
    let b = Bench::new();
    let c = &b.ctl;
    let cfg = |n: &str| std::fs::read_to_string(b.tmp.join("eqapo-sim/config").join(n)).unwrap_or_default();
    assert_eq!(c.snapshot()["hp_eq"]["apo"], json!(false));
    b.act("hp-eq", json!({"headphone": "Sennheiser HD 560S", "style": "basses"}));
    assert_eq!(c.lock().state.hp_eq.bands, rustudio_core::hpeq::style("basses").unwrap().bands.to_vec());
    b.act("hp-eq-setup", json!({}));
    assert!(wait_for(5.0, || !c.eqapo.setup_running()));
    let st = c.snapshot()["hp_eq"].clone();
    assert!(st["setup"]["error"].is_null(), "{st}");
    assert!(st["apo"] == json!(true) && st["endpoints"]["Game"] == json!(true) && st["active"] == json!(true), "{st}");
    assert!(c.lock().state.hp_eq.enabled);
    assert!(cfg("config.txt").contains("Include: rustudio.txt"));
    assert!(cfg("rustudio.txt").contains("Preamp: -"));
    let set = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<std::collections::HashSet<_>>();
    c.hp_tick(Some(&set(&["obs64.exe", "discord.exe"])));
    assert!(!cfg("rustudio.txt").contains("Filter"), "OBS ouvert : coupe");
    assert_eq!(c.snapshot()["hp_eq"]["live"], json!(true));
    c.hp_tick(Some(&set(&["discord.exe"])));
    assert!(cfg("rustudio.txt").contains("Filter"));
    b.act("hp-eq-remove", json!({}));
    assert!(wait_for(5.0, || !c.eqapo.setup_running()));
    assert_eq!(c.snapshot()["hp_eq"]["endpoints"]["Game"], json!(false));
    assert!(!cfg("rustudio.txt").contains("Filter"));
}

#[test]
fn egaliseur_profils() {
    let b = Bench::new();
    let c = &b.ctl;
    b.act("hp-eq", json!({"bands": [1, 2, 3, 0, 0, 0, 0, 0, 0, 9]}));
    assert_eq!(c.lock().state.hp_eq.style, None, "bande touchee a la main : reglage perso");
    b.act("hp-eq-save", json!({"name": "FPS"}));
    assert_eq!(c.lock().state.hp_eq.style.as_deref(), Some("custom:FPS"));
    b.act("hp-eq", json!({"style": "neutre"}));
    b.act("hp-eq", json!({"style": "custom:FPS"}));
    assert_eq!(c.lock().state.hp_eq.bands[9], 9);
    let st = rustudio_core::state::normalize_state(&c.lock().state.to_value());
    assert_eq!(st.hp_eq.custom["FPS"][0], 1);
    b.act("hp-eq-delete", json!({"name": "FPS"}));
    assert!(!c.lock().state.hp_eq.custom.contains_key("FPS"));
    assert_eq!(c.lock().state.hp_eq.style, None);
}

// ---------- anti-echo ----------
#[test]
fn verifications_anti_echo() {
    let b = Bench::new();
    let ok = |id: &str| -> bool {
        rustudio_engine::library::echo_check(&b.ctl)["checks"].as_array().unwrap().iter().find(|c| c["id"] == id).unwrap()["ok"] == json!(true)
    };
    assert!(ok("chat-loop") && ok("mic-speakers") && ok("monitor"));
    b.act("route", json!({"input": "Chat", "output": "ChatMic", "enabled": true}));
    b.act("route", json!({"input": "Microphone", "output": "LineOut", "enabled": true}));
    assert!(!ok("chat-loop"));
    assert!(!ok("mic-speakers"));
    let fix = rustudio_engine::library::echo_check(&b.ctl)["checks"][0]["fix"].clone();
    b.act(fix["action"].as_str().unwrap(), fix["body"].clone());
    assert!(ok("chat-loop"), "le bouton Corriger repare");
}

// ---------- HTTP ----------
#[test]
fn http_routes_et_protections() {
    let b = Bench::new();
    let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
    let port = server.server_addr().to_ip().unwrap().port();
    let (c, s) = (b.ctl.clone(), server.clone());
    std::thread::spawn(move || rustudio_engine::http::serve(c, s, 2));
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(3)).build();
    let url = |p: &str| format!("http://127.0.0.1:{port}{p}");
    let code = |r: Result<ureq::Response, ureq::Error>| match r {
        Ok(r) => r.status(),
        Err(ureq::Error::Status(c, _)) => c,
        Err(e) => panic!("{e}"),
    };
    assert_eq!(code(agent.post(&url("/api/volume")).send_json(json!({"channel": "Mic", "value": 1}))), 403, "POST sans en-tete");
    assert_eq!(code(agent.get(&url("/api/state")).set("Host", "evil.example").call()), 403, "hote etranger");
    assert_eq!(code(agent.get(&url("/api/health")).call()), 200);
    assert_eq!(code(agent.get(&url("/api/live?level=1")).call()), 200);
    let st: Value = agent.get(&url("/api/state")).call().unwrap().into_json().unwrap();
    assert!(st["meta"].get("mute_targets").is_some());
    assert_eq!(st["meta"]["mic_presets"].as_object().unwrap().keys().next().map(|s| s.as_str()), Some("brut"), "Naturel en premier");
    let r: Value = agent.post(&url("/api/mute")).set("X-Rustudio", "1").send_json(json!({"channel": "Music", "muted": true, "target": "stream"})).unwrap().into_json().unwrap();
    assert_eq!(r["ok"], json!(true));
    assert_eq!(code(agent.post(&url("/api/nimporte")).set("X-Rustudio", "1").send_json(json!({}))), 404);
    assert_eq!(code(agent.post(&url("/api/volume")).set("X-Rustudio", "1").send_json(json!({"channel": "Zzz", "value": 1}))), 400);
    assert_eq!(code(agent.post(&url("/api/mic-listen")).set("X-Rustudio", "1").send_json(json!({"seconds": 0}))), 200);
    assert_eq!(code(agent.get(&url("/api/processes")).call()), 200);
    assert_eq!(code(agent.get(&url("/api/library")).call()), 200);
    let html = agent.get(&url("/")).call().unwrap().into_string().unwrap();
    assert!(html.contains("<html"), "interface servie");
    assert_eq!(code(agent.post(&url("/api/sim/press")).set("X-Rustudio", "1").send_json(json!({"button": "Bleep"}))), 200);
}

#[test]
fn test_du_son_route_sample_vers_le_stream_seulement() {
    let b = Bench::new();
    let c = &b.ctl;
    let before = c.lock().effective_routing("Samples");
    assert!(before["Headphones"], "routage d'usine : Sample dans le casque");
    b.act("sound-test", json!({"seconds": 30}));
    let r = c.lock().effective_routing("Samples");
    assert!(r["BroadcastMix"] && !r["Headphones"] && !r["ChatMic"] && !r["LineOut"], "{r:?}");
    assert!(wait_for(3.0, || b.sim.lock().dev.route_on("Samples", "BroadcastMix") && !b.sim.lock().dev.route_on("Samples", "Headphones")));
    assert_eq!(c.lock().state.routing["Samples"]["Headphones"], true, "réglages enregistrés intacts");
    b.act("sound-test", json!({"seconds": 0}));
    assert!(wait_for(3.0, || b.sim.lock().dev.route_on("Samples", "Headphones")), "routage remis à la fin du test");
    assert_eq!(c.lock().effective_routing("Samples"), before);
}

// ---------- coupures du son comptees par le pilote ----------
#[test]
fn coupures_usb_comptees_par_le_pilote() {
    let b = Bench::new();
    *b.ctl.stats_interval.lock().unwrap() = Duration::from_millis(5);
    assert!(wait_for(5.0, || b.ctl.drops.lock().unwrap().available == Some(true)), "compteurs du pilote lus");
    assert_eq!(b.ctl.drops.lock().unwrap().total, 0, "rien de perdu tant que la GoXLR suit");
    b.sim.stream_drop(2);
    assert!(wait_for(5.0, || b.ctl.drops.lock().unwrap().total == 1), "coupure vue");
    let j = b.ctl.health()["usb_drops"].clone();
    assert_eq!(j["count"], 1);
    // La boucle USB ne fait aucune sauvegarde : seul le fil de diagnostic ecrit le fichier.
    let f = b.tmp.join("usb-drops.json");
    assert!(!f.exists(), "la boucle USB ne doit pas toucher au disque pour l'historique");
    b.ctl.save_drops();
    assert!(f.exists(), "historique non ecrit dans {}", f.display());
    let disque: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
    assert_eq!(disque["events"].as_array().unwrap().len(), 1);
    assert_eq!(disque["events"][0]["out_ms"], 2.0);
    assert_eq!(j["out_ms"], 2.0);
    assert_eq!(j["in_ms"], 2.0);
    assert_eq!(j["streaming"], true);
    // debranchee puis rebranchee : les compteurs repartent sans fausse coupure
    b.sim.unplug();
    assert!(wait_for(5.0, || !b.ctl.lock().connected));
    b.sim.lock().stream = Some(Default::default());
    b.sim.plug();
    b.settle();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(b.ctl.drops.lock().unwrap().total, 1);
}
