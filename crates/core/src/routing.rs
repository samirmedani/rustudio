//! Routage reellement envoye : mutes partiels, VOD sans musique, « M'écouter ».
use crate::consts::*;
use crate::state::State;
use indexmap::IndexMap;

pub fn vod_output(mix2: bool) -> &'static str {
    if mix2 { "StreamMix2" } else { "Sampler" }
}

pub fn effective_routing(st: &State, inp: &str, mix2: bool, listening: bool) -> IndexMap<String, bool> {
    let mut r = st.routing.get(inp).cloned().unwrap_or_default();
    let vod_on = st.vod.mode == "no-music";
    let vod = vod_output(mix2);
    if vod_on {
        let v = if inp == "Music" { false } else { r.get("BroadcastMix").copied().unwrap_or(false) };
        r.insert(vod.into(), v);
    }
    let ch = in_channel(inp).unwrap_or("Mic");
    if st.muted(ch) {
        if ch == "Mic" {
            for v in r.values_mut() {
                *v = false;
            }
        }
    } else if let Some(t) = st.partial_of(ch) {
        for out in mute_target(t).unwrap_or(&[]) {
            r.insert(out.to_string(), false);
        }
        if t == "stream" && vod_on {
            r.insert(vod.into(), false);
        }
    }
    if inp == "Microphone" && listening && !st.muted("Mic") {
        r.insert("Headphones".into(), true); // « M'écouter » : ta voix dans le casque, quelques secondes
    }
    r
}

/// Octets de routage d'une entree (gauche, droite) : 22 octets, 26 avec Stream Mix 2.
pub fn routing_bytes(outs: &IndexMap<String, bool>, mix2: bool) -> (Vec<u8>, Vec<u8>) {
    let size = if mix2 { 26 } else { 22 };
    let (mut left, mut right) = (vec![0u8; size], vec![0u8; size]);
    for (out, on) in outs {
        if !*on || (out == "StreamMix2" && !mix2) {
            continue;
        }
        if let Some((pl, pr)) = output_pos(out) {
            left[pl] = 0x20;
            right[pr] = 0x20;
        }
    }
    (left, right)
}
