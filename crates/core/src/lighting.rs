//! Couleurs, themes et voyants des boutons.
use crate::consts::*;
use crate::state::State;
use std::collections::HashMap;

pub fn darken(hexcol: &str, factor: f64) -> String {
    let v = u32::from_str_radix(hexcol, 16).unwrap_or(0);
    let (r, g, b) = ((v >> 16) & 0xFF, (v >> 8) & 0xFF, v & 0xFF);
    format!("{:02X}{:02X}{:02X}", (r as f64 * factor) as u32, (g as f64 * factor) as u32, (b as f64 * factor) as u32)
}

/// ordre appareil : B, G, R, 0
pub fn colour_bytes(hexcol: &str) -> [u8; 4] {
    let v = u32::from_str_radix(hexcol.trim_start_matches('#'), 16).unwrap_or(0);
    [(v & 0xFF) as u8, ((v >> 8) & 0xFF) as u8, ((v >> 16) & 0xFF) as u8, 0]
}

/// Ecrit les couleurs du theme courant dans l'etat (le theme « voies » suit les faders).
pub fn apply_theme(st: &mut State) {
    let Some(th) = theme(&st.lighting.theme) else { return };
    for (i, f) in FADERS.iter().enumerate() {
        let top = if th.per_channel {
            channel_colour(st.fader_channel(f)).to_string()
        } else if let Some(fs) = th.faders {
            fs[i].to_string()
        } else {
            th.top.to_string()
        };
        let bottom = if th.bottom.is_empty() { darken(&top, 0.16) } else { th.bottom.to_string() };
        st.colours.insert(format!("FadeMeter{}", i + 1), vec![bottom, top.clone()]);
        st.colours.insert(format!("Fader{}Mute", i + 1), vec![top.clone(), darken(&top, 0.3)]);
    }
    let accent = th.accent.to_string();
    st.colours.insert("Global".into(), vec![accent.clone(), accent.clone()]);
    st.colours.insert("Accent".into(), vec![accent.clone(), accent.clone()]);
    let mic = if th.per_channel { channel_colour("Mic").to_string() } else { accent.clone() };
    st.colours.insert("MicrophoneMute".into(), vec![mic, "000000".into()]);
    st.colours.insert("Bleep".into(), vec![accent, "000000".into()]);
}

pub fn theme_preview(st: &State, name: &str) -> Vec<String> {
    let Some(th) = theme(name) else { return vec![] };
    if th.per_channel {
        return FADERS.iter().map(|f| channel_colour(st.fader_channel(f)).to_string()).collect();
    }
    if let Some(fs) = th.faders {
        return fs.iter().map(|s| s.to_string()).collect();
    }
    vec![th.top.to_string(); 4]
}

/// Carte couleurs de 520 octets envoyee a la Mini.
pub fn colour_map(st: &State) -> Vec<u8> {
    let mut data = vec![0u8; 520];
    for (t, start, count) in COLOUR_TARGETS {
        let mut cols: Vec<String> = st.colours.get(t).cloned().unwrap_or_else(|| vec!["000000".into(); 2]);
        if let Some(n) = t.strip_prefix("FadeMeter") {
            let i: usize = n.parse::<usize>().unwrap_or(1) - 1;
            if st.muted(st.fader_channel(FADERS[i])) {
                cols = vec!["000000".into(), "000000".into()]; // comme goxlr-utility sur Mini : fader coupe = eteint
            }
        }
        for i in 0..count {
            let pos = start * 4 + i * 4;
            let c = cols.get(i).map(|s| s.as_str()).unwrap_or("000000");
            data[pos..pos + 4].copy_from_slice(&colour_bytes(c));
        }
    }
    data
}

fn led(st: &State, blink: &HashMap<String, bool>, ch: &str, mic_button: bool) -> u8 {
    let muted = st.muted(ch) || st.partial_of(ch).is_some();
    if !muted {
        return BTN_DIM;
    }
    if (st.muted(ch) && blink.get(ch).copied().unwrap_or(false)) || (mic_button && st.mic_button.blink) {
        return BTN_FLASH;
    }
    BTN_ON
}

pub fn button_states(st: &State, blink: &HashMap<String, bool>, bleep: bool) -> Vec<u8> {
    let mut arr = vec![BTN_DIM; 24];
    for (i, b) in FADER_BUTTONS.iter().enumerate() {
        arr[button_bit(b).unwrap() as usize] = led(st, blink, st.fader_channel(FADERS[i]), false);
    }
    arr[button_bit("MicrophoneMute").unwrap() as usize] = led(st, blink, "Mic", true);
    arr[button_bit("Bleep").unwrap() as usize] = if bleep { BTN_ON } else { BTN_DIM };
    arr
}
