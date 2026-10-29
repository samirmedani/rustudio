//! Ce que lit l'interface : etat complet (/api/state), vue en direct (/api/live), diagnostic (/api/health).
use crate::config::{now_epoch, VERSION};
use crate::controller::{AutoActive, Ctl};
use rustudio_core::consts::*;
use rustudio_core::dsp::{mic_type_preset, MIC_PRESETS};
use rustudio_core::hpeq::{HEADPHONES, HP_BANDS, HP_CHANNELS, HP_Q, HP_STYLES};
use rustudio_core::{lighting, scenes};
use serde_json::{json, Map, Value};
use std::time::{Duration, Instant};

fn live_json(l: &crate::controller::Live) -> Value {
    json!({"fader_positions": l.fader_positions, "mic_db": l.mic_db, "pressed": l.pressed, "encoders": l.encoders})
}

impl Ctl {
    pub fn health_status(&self, g: &crate::controller::Inner) -> &'static str {
        if g.connected {
            return if self.heartbeat_age() > Duration::from_secs(5) { "blocked" } else { "ok" };
        }
        match g.present {
            Some(false) => "absent",
            None if g.connects == 0 => "starting",
            _ => "error",
        }
    }

    pub fn update_snapshot(&self) -> Value {
        let reminded = self.lock().state.prefs.update_reminded.clone();
        self.updater.snapshot(reminded)
    }

    pub fn live_snapshot(&self, want_level: bool) -> Value {
        let brief = self.updater.brief();
        let v = self.version();
        let mut g = self.lock();
        if want_level {
            g.level_until = Some(Instant::now() + Duration::from_secs(3));
        }
        let s = &g.state;
        let listening = g.listen_until.map(|t| (t.saturating_duration_since(Instant::now()).as_secs_f64() * 10.0).round() / 10.0).unwrap_or(0.0);
        json!({"v": v, "boot": self.boot_id, "connected": g.connected, "live": live_json(&g.live),
               "job": g.job.clone().map(Value::Object), "mic": {"muted": s.muted("Mic"), "partial": s.partial_of("Mic")},
               "health": self.health_status(&g), "sim": self.simulated(), "osd": s.prefs.osd, "scene": s.scene,
               "app_version": VERSION, "update": brief, "usb": g.usb, "listening": listening,
               "sound_test": g.sound_test_until.is_some()})
    }

    pub fn health(&self) -> Value {
        let info = self.dev_info.lock().unwrap().clone();
        let g = self.lock();
        let now = Instant::now();
        json!({
            "status": self.health_status(&g), "version": VERSION, "pid": std::process::id(), "simulated": self.simulated(),
            "supervised": self.cfg.supervised, "engine_restarts": self.cfg.engine_restarts,
            "connected": g.connected, "present": g.present, "serial": info.serial, "firmware": info.firmware,
            "engine_uptime_s": (now_epoch() - self.started).round(),
            "device_uptime_s": g.connected_since.map(|t| (now_epoch() - t).round()),
            "loop_age_ms": self.heartbeat_age().as_millis() as u64,
            "usb_call_ms": info.busy_since.map(|b| now.duration_since(b).as_millis() as u64),
            "usb_stage": info.stage, "usb": g.usb, "reconnects": g.reconnects, "command_index": info.count,
            "stats": info.stats, "pending": g.dirty.len(), "last_error": g.last_error,
            "errors": g.errors.iter().map(|(t, m)| json!({"t": t, "msg": m})).collect::<Vec<_>>(),
            "auto_scene": g.auto_note, "log": crate::log::recent(30),
        })
    }

    pub fn snapshot(&self) -> Value {
        let update = self.update_snapshot();
        let info = self.dev_info.lock().unwrap().clone();
        let v = self.version();
        let hp_live = self.lock().hp_live;
        let hp = self.eqapo.status(hp_live);
        let g = self.lock();
        let mix2 = g.connected && g.mix2;
        let auto_active = match &g.auto_active {
            AutoActive::On(a, s) => json!([a, s]),
            _ => Value::Null,
        };
        json!({
            "version": VERSION, "v": v, "connected": g.connected, "serial": info.serial, "firmware": info.firmware,
            "mix2": mix2, "error": if g.connected { Value::Null } else { json!(g.last_error) },
            "health": self.health_status(&g), "usb": g.usb, "sim": self.simulated(), "supervised": self.cfg.supervised,
            "update": update, "hp_eq": hp, "state": g.state.to_value(), "live": live_json(&g.live),
            "job": g.job.clone().map(Value::Object),
            "blink": g.blink.iter().filter(|(_, v)| **v).map(|(k, v)| (k.clone(), json!(v))).collect::<Map<_, _>>(),
            "auto": {"active": auto_active, "note": g.auto_note},
            "meta": meta(&g.state, mix2),
        })
    }
}

pub fn meta(st: &rustudio_core::State, mix2: bool) -> Value {
    let map = |pairs: &[(&str, &str)]| pairs.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<Map<_, _>>();
    json!({
        "channels": CHANNELS, "faders": FADERS, "inputs": INPUTS.iter().map(|(n, _, _)| *n).collect::<Vec<_>>(),
        "outputs": OUTPUTS.iter().map(|(n, _, _)| *n).filter(|o| mix2 || *o != "StreamMix2").collect::<Vec<_>>(),
        "vod_output": rustudio_core::routing::vod_output(mix2),
        "channel_colours": map(&CHANNEL_COLOURS), "mic_types": MIC_TYPES,
        "recommended_gain": RECOMMENDED_GAIN.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<Map<_, _>>(),
        "mic_type_preset": MIC_TYPES.iter().map(|t| (t.to_string(), json!(mic_type_preset(t)))).collect::<Map<_, _>>(),
        "mute_targets": MUTE_TARGETS.iter().map(|(k, _)| *k).collect::<Vec<_>>(), "mic_button_modes": MIC_BUTTON_MODES,
        "channel_inputs": IN_CHANNEL.iter().map(|(i, c)| (c.to_string(), json!(i))).collect::<Map<_, _>>(),
        "mic_presets": MIC_PRESETS.iter().map(|p| (p.key.to_string(), json!({"label": p.label, "desc": p.desc, "dsp": (p.dsp)()}))).collect::<Map<_, _>>(),
        "scenes": scenes::SCENES.iter().map(|s| (s.key.to_string(), json!({"label": s.label, "desc": s.desc, "faders": s.faders_map(), "mic_preset": s.mic_preset}))).collect::<Map<_, _>>(),
        "themes": THEMES.iter().map(|t| (t.key.to_string(), json!({"label": t.label, "preview": lighting::theme_preview(st, t.key)}))).collect::<Map<_, _>>(),
        "animations": ANIMATIONS.iter().map(|(k, _)| *k).collect::<Vec<_>>(),
        "fader_styles": FADER_STYLES.iter().map(|(k, _, _)| *k).collect::<Vec<_>>(),
        "eq_bands": EQ_BANDS.iter().map(|(k, _, _)| *k).collect::<Vec<_>>(), "ratios": RATIOS,
        "known_apps": KNOWN_APPS.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<Map<_, _>>(),
        "hp_bands": HP_BANDS, "hp_q": HP_Q, "hp_channels": HP_CHANNELS,
        "hp_styles": HP_STYLES.iter().map(|s| (s.key.to_string(), json!({"label": s.label, "desc": s.desc, "bands": s.bands}))).collect::<Map<_, _>>(),
        "headphones": HEADPHONES.iter().map(|(n, f)| (n.to_string(), json!(f.iter().map(|(k, f0, g, q)| json!([k, f0, g, q])).collect::<Vec<_>>()))).collect::<Map<_, _>>(),
        "device": {"id": "goxlr-mini", "name": "GoXLR Mini", "vendor": "TC-Helicon", "module": "goxlr-mini", "official": true},
    })
}
