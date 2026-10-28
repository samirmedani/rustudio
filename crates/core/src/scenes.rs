//! Scenes integrees : faders et routage (la voix et le retour casque restent a toi).
use crate::consts::OUTPUTS;
use indexmap::IndexMap;

pub type Routing = IndexMap<String, IndexMap<String, bool>>;

/// Routage de base, avec des exceptions par entree.
pub fn routing(over: &[(&str, &[(&str, bool)])]) -> Routing {
    let on = |outs: &[&str]| -> IndexMap<String, bool> {
        OUTPUTS.iter().map(|(o, _, _)| (o.to_string(), outs.contains(o))).collect()
    };
    let mut base: Routing = IndexMap::new();
    base.insert("Microphone".into(), on(&["BroadcastMix", "ChatMic", "Sampler", "StreamMix2"]));
    base.insert("Chat".into(), on(&["Headphones", "BroadcastMix", "LineOut", "StreamMix2"]));
    base.insert("Music".into(), on(&["Headphones", "BroadcastMix", "LineOut", "StreamMix2"]));
    base.insert("Game".into(), on(&["Headphones", "BroadcastMix", "LineOut", "StreamMix2"]));
    base.insert("Console".into(), on(&["Headphones", "BroadcastMix", "LineOut", "StreamMix2"]));
    base.insert("LineIn".into(), on(&["Headphones", "BroadcastMix", "LineOut", "StreamMix2"]));
    base.insert("System".into(), on(&["Headphones", "LineOut"]));
    base.insert("Samples".into(), on(&["Headphones", "BroadcastMix", "ChatMic", "LineOut", "StreamMix2"]));
    for (inp, outs) in over {
        if let Some(m) = base.get_mut(*inp) {
            for (o, v) in outs.iter() {
                m.insert(o.to_string(), *v);
            }
        }
    }
    base
}

pub struct Scene {
    pub key: &'static str,
    pub label: &'static str,
    pub desc: &'static str,
    pub faders: [&'static str; 4],
    pub over: &'static [(&'static str, &'static [(&'static str, bool)])],
    pub mic_preset: Option<&'static str>,
}

impl Scene {
    pub fn routing(&self) -> Routing {
        routing(self.over)
    }
    pub fn faders_map(&self) -> IndexMap<String, String> {
        crate::consts::FADERS.iter().zip(self.faders.iter()).map(|(f, c)| (f.to_string(), c.to_string())).collect()
    }
}

pub const SCENES: [Scene; 6] = [
    Scene { key: "streaming", label: "Streaming", desc: "Micro, Discord, musique et jeu. Le stream entend tout sauf tes notifications.",
            faders: ["Mic", "Chat", "Music", "Game"], over: &[], mic_preset: Some("stream") },
    Scene { key: "stream-safe", label: "Stream sans musique", desc: "Comme Streaming, mais la musique reste dans ton casque (anti-DMCA).",
            faders: ["Mic", "Chat", "Music", "Game"], over: &[("Music", &[("BroadcastMix", false), ("StreamMix2", false)])], mic_preset: Some("stream") },
    Scene { key: "gaming", label: "Gaming", desc: "Jeu, Discord, musique et système sous la main. Micro sur le bouton de toux.",
            faders: ["Game", "Chat", "Music", "System"], over: &[], mic_preset: Some("anti-bruit") },
    Scene { key: "discord", label: "Soirée Discord", desc: "Tes amis entendent ta musique en plus de ta voix.",
            faders: ["Mic", "Chat", "Music", "Game"], over: &[("Music", &[("ChatMic", true)])], mic_preset: Some("naturel") },
    Scene { key: "podcast", label: "Podcast", desc: "Micro, invité à distance, habillage musical et jingles.",
            faders: ["Mic", "Chat", "Music", "Sample"], over: &[], mic_preset: Some("podcast") },
    Scene { key: "ecoute", label: "Écoute", desc: "Musique et films d’abord, le micro reste discret.",
            faders: ["Music", "System", "Game", "Chat"], over: &[], mic_preset: None },
];

pub fn scene(name: &str) -> Option<&'static Scene> {
    SCENES.iter().find(|s| s.key == name)
}
