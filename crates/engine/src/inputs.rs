//! GoXLR Mini : boutons physiques, coupures du micro et des voies, filtrage des faders.
use crate::controller::{ch, inp, Inner, Key};
use crate::log::log;
use crate::logf;
use rustudio_core::consts::*;
use std::time::{Duration, Instant};

impl Inner {
    // -- mutes --
    pub fn mute_to(&mut self, c: &str, target: &str) {
        let target = if mute_target(target).is_some() { target } else { "all" };
        if target == "all" || channel_input(c).is_none() {
            self.state.mutes.insert(c.into(), true);
            self.state.partial.insert(c.into(), None);
        } else {
            self.state.mutes.insert(c.into(), false);
            self.state.partial.insert(c.into(), Some(target.into()));
        }
    }
    pub fn unmute(&mut self, c: &str) {
        self.state.mutes.insert(c.into(), false);
        self.state.partial.insert(c.into(), None);
        self.blink.remove(c);
    }
    pub fn mute_changed(&mut self, c: &str) {
        let mut keys = vec![Key::Mute(ch(c)), Key::Colours, Key::Buttons];
        if let Some(i) = channel_input(c) {
            keys.push(Key::Route(inp(i)));
        }
        self.mark(&keys, true);
    }
    pub fn is_muted(&self, c: &str) -> bool {
        self.state.muted(c) || self.state.partial_of(c).is_some()
    }
    fn toggle(&mut self, c: &str, target: &str, who: &str) {
        let what = if self.is_muted(c) {
            self.unmute(c);
            "actif".to_string()
        } else {
            self.mute_to(c, target);
            format!("coupé {}", mute_target_label(if self.state.muted(c) { "all" } else { target }))
        };
        self.mute_changed(c);
        logf!("{who} : {} {what}", channel_label(c));
    }

    // -- boutons et faders physiques --
    pub fn handle_inputs(&mut self, mask: u32, vols: [u8; 4], now: Instant) {
        let pressed = mask & !self.prev_mask;
        let released = self.prev_mask & !mask;
        self.prev_mask = mask;
        for (name, bit) in BUTTON_BITS {
            let b = 1u32 << bit;
            if pressed & b != 0 {
                self.btn.insert(name, (now, false));
                self.on_down(name);
            }
            if released & b != 0 {
                let st = self.btn.remove(name).unwrap_or((now, false));
                self.on_up(name, st.1);
            }
        }
        let hold = Duration::from_millis(self.state.prefs.hold_ms.max(0) as u64);
        let due: Vec<&'static str> =
            self.btn.iter().filter(|(_, (t, held))| !held && now.duration_since(*t) >= hold).map(|(n, _)| *n).collect();
        for name in due {
            if let Some(e) = self.btn.get_mut(name) {
                e.1 = true;
            }
            self.on_hold(name);
        }
        let mut changed = false;
        match self.prev_pos {
            None => {
                // premiere lecture : la reference, rien a appliquer
                self.prev_pos = Some(vols);
                self.pos_history = vols.map(|v| [v; 2]);
                self.faders_seen = self.faders_seen || vols.iter().any(|v| *v != 0);
            }
            Some(mut prev) => {
                for i in 0..4 {
                    let value = fader_filtered(prev[i], &mut self.pos_history[i], vols[i]);
                    if let Some(value) = value {
                        let c = self.state.fader_channel(FADERS[i]).to_string();
                        // Une commande logicielle arrivee pendant cette lecture doit partir avant
                        // d'accepter un nouvel etat du materiel.
                        if self.dirty.contains(&Key::All) || self.dirty.contains(&Key::Faders) || self.dirty.contains(&Key::Volume(ch(&c))) {
                            continue;
                        }
                        prev[i] = value;
                        self.faders_seen = true;
                        if self.state.volume(&c) != value as i32 {
                            self.state.volumes.insert(c, value as i32);
                            changed = true;
                        }
                    }
                }
                self.prev_pos = Some(prev);
            }
        }
        // La Mini applique deja les mouvements physiques a son mixeur. Les renvoyer en
        // commandes Volume peut ecraser un mouvement plus recent et figer le niveau.
        if changed {
            self.version += 1;
            self.save_dirty = true;
        }
        self.live.fader_positions = match (self.faders_seen, self.prev_pos) {
            (true, Some(p)) => p.map(Some),
            _ => [None; 4],
        };
        self.live.pressed = BUTTON_BITS.iter().filter(|(_, b)| mask & (1 << b) != 0).map(|(n, _)| *n).collect();
    }

    fn on_down(&mut self, name: &str) {
        if name == "MicrophoneMute" {
            let mb = self.state.mic_button.clone();
            if mb.mode == "hold" {
                self.mute_to("Mic", &mb.target);
                self.mute_changed("Mic");
            } else if mb.mode == "ptt" {
                self.unmute("Mic");
                self.mute_changed("Mic");
            }
        } else if name == "Bleep" {
            self.bleep = true;
            self.mark(&[Key::Buttons], false);
        }
    }

    fn on_hold(&mut self, name: &str) {
        if let Some(i) = FADER_BUTTONS.iter().position(|b| *b == name) {
            let c = self.state.fader_channel(FADERS[i]).to_string();
            if !self.state.muted(&c) {
                self.mute_to(&c, "all");
                self.blink.insert(c.clone(), true);
                self.mute_changed(&c);
                logf!("Bouton {} maintenu : {} coupé partout", i + 1, channel_label(&c));
            }
        } else if name == "MicrophoneMute" && self.state.mic_button.mode == "toggle" && !self.state.muted("Mic") {
            self.mute_to("Mic", "all");
            self.blink.insert("Mic".into(), true);
            self.mute_changed("Mic");
            log("Bouton micro maintenu : micro coupé partout");
        }
    }

    fn on_up(&mut self, name: &str, held: bool) {
        if let Some(i) = FADER_BUTTONS.iter().position(|b| *b == name) {
            if held {
                return;
            }
            let c = self.state.fader_channel(FADERS[i]).to_string();
            let mode = self.state.mute_modes.get(FADERS[i]).cloned().unwrap_or_else(|| "all".into());
            self.toggle(&c, &mode, &format!("Bouton {}", i + 1));
        } else if name == "MicrophoneMute" {
            let mb = self.state.mic_button.clone();
            match mb.mode.as_str() {
                "toggle" if !held => self.toggle("Mic", &mb.target, "Bouton micro"),
                "hold" => {
                    self.unmute("Mic");
                    self.mute_changed("Mic");
                }
                "ptt" => {
                    self.mute_to("Mic", &mb.target);
                    self.mute_changed("Mic");
                }
                _ => {}
            }
        } else if name == "Bleep" {
            self.bleep = false;
            self.mark(&[Key::Buttons], false);
        }
    }
}

/// Deux unites d'hysteresis eliminent le bruit au repos. Les butees restent exactes.
pub const FADER_DEADZONE: i16 = 2;

/// Mediane des trois dernieres lectures : un pic isole est ignore, un mouvement continu
/// est suivi avec une seule lecture de retard, sans attendre que le fader s'immobilise.
pub fn fader_filtered(prev: u8, history: &mut [u8; 2], value: u8) -> Option<u8> {
    let mut values = [history[0], history[1], value];
    *history = [history[1], value];
    values.sort_unstable();
    let filtered = values[1];
    let delta = (filtered as i16 - prev as i16).abs();
    (delta >= FADER_DEADZONE || (delta != 0 && matches!(filtered, 0 | 255))).then_some(filtered)
}

#[cfg(test)]
mod tests {
    use super::fader_filtered;

    #[test]
    fn mouvement_continu_suivi_avec_une_seule_lecture_de_retard() {
        let mut history = [0; 2];
        let mut previous = 0;
        let trajectory = [20, 50, 90, 130, 180, 220, 255, 255];
        for (i, value) in trajectory.into_iter().enumerate() {
            if let Some(filtered) = fader_filtered(previous, &mut history, value) { previous = filtered; }
            assert_eq!(previous, if i == 0 { 0 } else { trajectory[i - 1] });
        }
        assert_eq!(previous, 255);
    }

    #[test]
    fn pic_isole_ignore() {
        let mut history = [128; 2];
        for value in [255, 128, 128, 0, 128, 128] {
            assert_eq!(fader_filtered(128, &mut history, value), None);
        }
    }

    #[test]
    fn bruit_d_unite_ignore_et_butees_exactes() {
        let mut history = [100; 2];
        for value in [101, 100, 101, 100] { assert_eq!(fader_filtered(100, &mut history, value), None); }
        for (previous, end) in [(254, 255), (1, 0)] {
            let mut history = [previous; 2];
            assert_eq!(fader_filtered(previous, &mut history, end), None);
            assert_eq!(fader_filtered(previous, &mut history, end), Some(end));
        }
    }
}
