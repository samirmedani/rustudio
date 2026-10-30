//! Registre des modules compiles : seule cette liste fait autorite sur ce que Rustudio peut piloter.
//! La bibliotheque en ligne est un catalogue de contributions, pas un chargeur de code.
use serde::Serialize;

#[derive(Clone, Copy, Debug, Serialize)]
pub struct ModulePage {
    pub id: &'static str,
    pub label: &'static str,
    pub icon: &'static str,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct ModuleDescriptor {
    pub id: &'static str,
    pub name: &'static str,
    pub vendor: &'static str,
    pub kind: &'static str,
    pub summary: &'static str,
    pub capabilities: &'static [&'static str],
    pub pages: &'static [ModulePage],
    pub actions: &'static [&'static str],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ModuleStatus {
    Ready,
    Disconnected,
    Starting,
    Error,
    Blocked,
}

/// Etat d'une instance pilotee par un module. Les donnees de configuration restent propres au module.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModuleRuntime {
    pub id: &'static str,
    pub status: ModuleStatus,
    pub connected: bool,
    pub simulated: bool,
    pub firmware: Option<String>,
    pub error: Option<String>,
}

pub const GOXLR_MINI: ModuleDescriptor = ModuleDescriptor {
    id: "goxlr-mini",
    name: "GoXLR Mini",
    vendor: "TC-Helicon",
    kind: "audio-interface",
    summary: "Mixeur, micro, routage, éclairage et scènes avec le pilote officiel GoXLR.",
    capabilities: &["mixer", "microphone", "headphone-eq", "routing", "lighting", "scenes"],
    pages: &[
        ModulePage { id: "mixer", label: "Mixeur", icon: "mixer" },
        ModulePage { id: "mic", label: "Micro", icon: "Mic" },
        ModulePage { id: "hp", label: "Casque", icon: "Headphones" },
        ModulePage { id: "routing", label: "Routage", icon: "routing" },
        ModulePage { id: "light", label: "Éclairage", icon: "light" },
        ModulePage { id: "scenes", label: "Scènes", icon: "scenes" },
    ],
    actions: &[
        "volume", "mute", "fader", "route", "colour", "mic", "raw", "profile", "dsp", "mic-preset", "scene",
        "scene-save", "scene-delete", "theme", "lighting", "detect-mic", "auto-gain", "voice-setup", "mic-listen",
        "job-cancel", "settings", "auto-scenes", "reconnect", "fx", "hp-eq", "hp-eq-save", "hp-eq-delete",
        "hp-eq-setup", "hp-eq-remove", "echo-check", "sound-test",
    ],
};

const INSTALLED: &[ModuleDescriptor] = &[GOXLR_MINI];

pub fn installed() -> &'static [ModuleDescriptor] {
    INSTALLED
}

pub fn find(id: &str) -> Option<&'static ModuleDescriptor> {
    INSTALLED.iter().find(|module| module.id == id)
}
